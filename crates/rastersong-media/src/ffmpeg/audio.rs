use std::path::Path;
use std::ptr;

use ffmpeg::util::error::EAGAIN;
use ffmpeg::{decoder, ffi, frame, media};
use ffmpeg_next as ffmpeg;

use super::{decode_error, open_input};
use crate::{AudioClip, AudioOptions, MediaError};

/// Decodes the best audio stream of `path` in full, converted to interleaved `f32`.
pub(crate) fn load_audio(path: &Path, options: AudioOptions) -> Result<AudioClip, MediaError> {
    let mut input = open_input(path)?;
    let stream = input
        .streams()
        .best(media::Type::Audio)
        .ok_or_else(|| MediaError::NoAudioStream(path.to_owned()))?;
    let stream_index = stream.index();
    let mut decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .and_then(|c| c.decoder().audio())
        .map_err(decode_error)?;

    let mut resampler = Resampler::new(
        options.sample_rate.unwrap_or(decoder.rate()),
        options.channels.unwrap_or(u32::from(decoder.channels())),
    )?;
    let mut decoded = frame::Audio::empty();
    for (stream, packet) in input.packets() {
        if stream.index() != stream_index {
            continue;
        }
        if let Err(e) = decoder.send_packet(&packet) {
            tracing::debug!(path = %path.display(), "audio decoder rejected a packet: {e}");
        }
        drain(&mut decoder, &mut decoded, &mut resampler)?;
    }
    decoder.send_eof().map_err(decode_error)?;
    drain(&mut decoder, &mut decoded, &mut resampler)?;
    resampler.finish()
}

fn drain(
    decoder: &mut decoder::Audio,
    decoded: &mut frame::Audio,
    resampler: &mut Resampler,
) -> Result<(), MediaError> {
    loop {
        match decoder.receive_frame(decoded) {
            Ok(()) => resampler.push(decoded)?,
            Err(ffmpeg::Error::Other { errno: EAGAIN }) | Err(ffmpeg::Error::Eof) => return Ok(()),
            Err(e) => tracing::debug!("audio decode error: {e}"),
        }
    }
}

/// Converts decoded audio of any format, layout and rate to interleaved `f32` at a fixed rate
/// and channel count. The converter is rebuilt (after flushing) if the input format changes
/// mid-stream.
struct Resampler {
    swr: *mut ffi::SwrContext,
    /// Sample format and rate the current converter was built for, if any.
    input: Option<(i32, i32)>,
    /// Channel layout the current converter was built for (an owned copy).
    input_layout: ffi::AVChannelLayout,
    out_rate: u32,
    out_layout: ffi::AVChannelLayout,
    samples: Vec<f32>,
}

impl Resampler {
    fn new(out_rate: u32, out_channels: u32) -> Result<Self, MediaError> {
        if out_rate == 0 || out_channels == 0 {
            return Err(MediaError::Decode(format!(
                "invalid audio output: {out_rate} Hz, {out_channels} channels"
            )));
        }
        // SAFETY: zeroed is a valid "unset" AVChannelLayout, and av_channel_layout_default fills it.
        let out_layout = unsafe {
            let mut layout = std::mem::zeroed();
            ffi::av_channel_layout_default(&mut layout, out_channels as i32);
            layout
        };
        Ok(Self {
            swr: ptr::null_mut(),
            input: None,
            // SAFETY: an all-zero AVChannelLayout is the valid empty layout.
            input_layout: unsafe { std::mem::zeroed() },
            out_rate,
            out_layout,
            samples: Vec::new(),
        })
    }

    fn channels(&self) -> usize {
        self.out_layout.nb_channels as usize
    }

    /// Whether `f` is already in the output format (interleaved `f32` at the output rate and
    /// layout), so its samples can be copied without a converter.
    fn passes_through(&self, f: &ffi::AVFrame) -> bool {
        f.format == ffi::AVSampleFormat::AV_SAMPLE_FMT_FLT as i32
            && u32::try_from(f.sample_rate) == Ok(self.out_rate)
            // SAFETY: both layouts are valid.
            && unsafe { ffi::av_channel_layout_compare(&self.out_layout, &f.ch_layout) } == 0
    }

    fn push(&mut self, frame: &frame::Audio) -> Result<(), MediaError> {
        // SAFETY: frame is a valid decoded audio frame and outlives `f`.
        let f = unsafe { &*frame.as_ptr() };
        if self.passes_through(f) {
            // Finish whatever the converter holds from earlier frames in another format.
            self.flush()?;
            self.free();
            let count = f.nb_samples.max(0) as usize * self.channels();
            if count > 0 {
                // SAFETY: packed f32 audio keeps all `nb_samples × channels` samples in data[0].
                let data = unsafe { std::slice::from_raw_parts(f.data[0] as *const f32, count) };
                self.samples.extend_from_slice(data);
            }
            return Ok(());
        }
        let unchanged = self.input == Some((f.format, f.sample_rate))
            // SAFETY: both layouts are valid.
            && unsafe { ffi::av_channel_layout_compare(&self.input_layout, &f.ch_layout) } == 0;
        if !unchanged {
            self.flush()?;
            self.free();
            // SAFETY: all pointers are valid; on failure the error is reported and the
            // (null or partially set up) context is freed by `free`.
            let ret = unsafe {
                let ret = ffi::av_channel_layout_copy(&mut self.input_layout, &f.ch_layout);
                if ret < 0 {
                    return Err(decode_error(ffmpeg::Error::from(ret)));
                }
                let ret = ffi::swr_alloc_set_opts2(
                    &mut self.swr,
                    &self.out_layout,
                    ffi::AVSampleFormat::AV_SAMPLE_FMT_FLT,
                    self.out_rate as i32,
                    &self.input_layout,
                    frame.format().into(),
                    f.sample_rate,
                    0,
                    ptr::null_mut(),
                );
                if ret < 0 {
                    ret
                } else {
                    ffi::swr_init(self.swr)
                }
            };
            if ret < 0 {
                return Err(MediaError::Decode(format!(
                    "could not set up audio conversion: {}",
                    ffmpeg::Error::from(ret)
                )));
            }
            self.input = Some((f.format, f.sample_rate));
        }
        self.convert(f.extended_data as *const *const u8, f.nb_samples)
    }

    /// Runs the converter on `count` input samples per channel, or drains it when `input` is null.
    fn convert(&mut self, input: *const *const u8, count: i32) -> Result<(), MediaError> {
        let channels = self.channels();
        loop {
            // SAFETY: swr is initialized; the output buffer has room for `capacity` samples per
            // channel, which is all swr_convert may write.
            let written = unsafe {
                let capacity = ffi::swr_get_out_samples(self.swr, count).max(0) as usize + 32;
                self.samples.reserve(capacity * channels);
                let out = self.samples.as_mut_ptr().add(self.samples.len()) as *mut u8;
                let written = ffi::swr_convert(self.swr, &out, capacity as i32, input, count);
                if written > 0 {
                    self.samples
                        .set_len(self.samples.len() + written as usize * channels);
                }
                written
            };
            if written < 0 {
                return Err(MediaError::Decode(format!(
                    "audio conversion failed: {}",
                    ffmpeg::Error::from(written)
                )));
            }
            // Input is consumed in one call; flushing may take several.
            if !input.is_null() || written == 0 {
                return Ok(());
            }
        }
    }

    fn flush(&mut self) -> Result<(), MediaError> {
        if self.swr.is_null() {
            return Ok(());
        }
        self.convert(ptr::null(), 0)
    }

    fn free(&mut self) {
        // SAFETY: swr is null or a context allocated by swr_alloc_set_opts2.
        unsafe {
            ffi::swr_free(&mut self.swr);
            ffi::av_channel_layout_uninit(&mut self.input_layout);
        }
        self.input = None;
    }

    fn finish(mut self) -> Result<AudioClip, MediaError> {
        self.flush()?;
        Ok(AudioClip {
            sample_rate: self.out_rate,
            channels: self.channels() as u32,
            samples: std::mem::take(&mut self.samples),
        })
    }
}

impl Drop for Resampler {
    fn drop(&mut self) {
        self.free();
        // SAFETY: out_layout was initialized by av_channel_layout_default.
        unsafe { ffi::av_channel_layout_uninit(&mut self.out_layout) };
    }
}
