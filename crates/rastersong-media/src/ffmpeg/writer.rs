//! Lossless output: FFV1 video with optional PCM audio in Matroska. Used by the CLI and tests;
//! the user-facing export formats (H.264, ProRes) come later.

use std::path::Path;

use ffmpeg::codec::{self, encoder};
use ffmpeg::format::{self, Pixel, Sample};
use ffmpeg::{ChannelLayout, Dictionary, Packet, ffi, frame};
use ffmpeg_next as ffmpeg;

use super::decode_error;
use crate::{AudioClip, MediaError, Rational};

/// Audio is written in chunks of at most this many samples per channel.
const AUDIO_CHUNK: usize = 4096;

/// Writes packed RGB8 frames as lossless FFV1 in a Matroska file, with an optional PCM audio track.
pub struct LosslessWriter {
    output: format::context::Output,
    video: Track<encoder::video::Encoder>,
    frame: frame::Video,
    frame_rate: Rational,
    frames_written: i64,
    audio: Option<AudioTrack>,
}

impl std::fmt::Debug for LosslessWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LosslessWriter")
            .field("frames_written", &self.frames_written)
            .finish_non_exhaustive()
    }
}

struct Track<E> {
    encoder: E,
    stream: usize,
    time_base: ffmpeg::Rational,
}

struct AudioTrack {
    track: Track<encoder::audio::Encoder>,
    /// Interleaved samples to write.
    samples: Vec<i16>,
    channels: usize,
    sample_rate: u32,
    /// Sample frames written so far.
    written: usize,
}

fn error(context: &str) -> impl Fn(ffmpeg::Error) -> MediaError + '_ {
    move |e| MediaError::Decode(format!("{context}: {e}"))
}

impl LosslessWriter {
    pub fn create(
        path: &Path,
        width: u32,
        height: u32,
        frame_rate: Rational,
        audio: Option<&AudioClip>,
    ) -> Result<Self, MediaError> {
        super::init()?;
        let mut output = format::output_as(path, "matroska").map_err(|e| MediaError::Open {
            path: path.to_owned(),
            reason: e.to_string(),
        })?;
        let global_header = output
            .format()
            .flags()
            .contains(format::Flags::GLOBAL_HEADER);

        let codec = encoder::find(codec::Id::FFV1)
            .ok_or_else(|| MediaError::Decode("this FFmpeg build has no FFV1 encoder".into()))?;
        let mut video = codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()
            .map_err(decode_error)?;
        let time_base = ffmpeg::Rational::new(frame_rate.den, frame_rate.num);
        video.set_width(width);
        video.set_height(height);
        video.set_format(Pixel::BGRZ);
        video.set_time_base(time_base);
        video.set_frame_rate(Some(ffmpeg::Rational::new(frame_rate.num, frame_rate.den)));
        if global_header {
            video.set_flags(codec::Flags::GLOBAL_HEADER);
        }
        let mut options = Dictionary::new();
        options.set("level", "3");
        let video = video
            .open_as_with(codec, options)
            .map_err(error("opening the FFV1 encoder"))?;
        let video_stream = {
            let mut stream = output.add_stream(codec).map_err(decode_error)?;
            stream.set_parameters(&video);
            stream.set_time_base(time_base);
            stream.index()
        };

        let audio = audio
            .filter(|clip| !clip.samples.is_empty())
            .map(|clip| add_audio(&mut output, clip, global_header))
            .transpose()?;

        output
            .write_header()
            .map_err(error("writing the file header"))?;
        // The muxer may change stream time bases when writing the header.
        let stream_time_base = |output: &format::context::Output, index| {
            output.stream(index).expect("stream exists").time_base()
        };
        let video = Track {
            time_base: stream_time_base(&output, video_stream),
            encoder: video,
            stream: video_stream,
        };
        let audio = audio.map(|mut a| {
            a.track.time_base = stream_time_base(&output, a.track.stream);
            a
        });

        Ok(Self {
            output,
            video,
            frame: frame::Video::new(Pixel::BGRZ, width, height),
            frame_rate,
            frames_written: 0,
            audio,
        })
    }

    /// Appends one frame of packed RGB8 (`width * height * 3` bytes).
    pub fn write_frame(&mut self, rgb: &[u8]) -> Result<(), MediaError> {
        let (width, height) = (self.frame.width() as usize, self.frame.height() as usize);
        assert_eq!(
            rgb.len(),
            width * height * 3,
            "frame size does not match the writer"
        );
        // The encoder may still hold a reference to the previous frame's buffers.
        // SAFETY: the frame is a valid, allocated video frame.
        let ret = unsafe { ffi::av_frame_make_writable(self.frame.as_mut_ptr()) };
        if ret < 0 {
            return Err(decode_error(ffmpeg::Error::from(ret)));
        }
        // FFV1's 8-bit RGB format is packed B, G, R, unused.
        let stride = self.frame.stride(0);
        let data = self.frame.data_mut(0);
        for (y, src) in rgb.chunks_exact(width * 3).enumerate() {
            let row = &mut data[y * stride..y * stride + width * 4];
            for (out, px) in row
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<3>().0)
            {
                out.copy_from_slice(&[px[2], px[1], px[0], 0]);
            }
        }
        self.frame.set_pts(Some(self.frames_written));
        self.frames_written += 1;
        self.video
            .encoder
            .send_frame(&self.frame)
            .map_err(error("encoding video"))?;
        write_packets(&mut self.output, &mut self.video)?;

        let video_end = self.frames_written as f64 / self.frame_rate.as_f64();
        self.write_audio_until(video_end)
    }

    /// Flushes the encoders and finishes the file. Audio stops where the video does.
    pub fn finish(mut self) -> Result<(), MediaError> {
        self.video
            .encoder
            .send_eof()
            .map_err(error("encoding video"))?;
        write_packets(&mut self.output, &mut self.video)?;
        if let Some(audio) = &mut self.audio {
            audio
                .track
                .encoder
                .send_eof()
                .map_err(error("encoding audio"))?;
            write_packets(&mut self.output, &mut audio.track)?;
        }
        self.output
            .write_trailer()
            .map_err(error("finishing the file"))
    }

    /// Writes audio up to `seconds`, keeping it interleaved with the video.
    fn write_audio_until(&mut self, seconds: f64) -> Result<(), MediaError> {
        let Some(audio) = &mut self.audio else {
            return Ok(());
        };
        let total = audio.samples.len() / audio.channels;
        let target = ((seconds * f64::from(audio.sample_rate)).round() as usize).min(total);
        while audio.written < target {
            let count = (target - audio.written).min(AUDIO_CHUNK);
            let mut frame = frame::Audio::new(
                Sample::I16(format::sample::Type::Packed),
                count,
                ChannelLayout::default(audio.channels as i32),
            );
            frame.set_rate(audio.sample_rate);
            frame.set_pts(Some(audio.written as i64));
            let start = audio.written * audio.channels;
            let src = &audio.samples[start..start + count * audio.channels];
            // Packed audio keeps every channel in plane 0, as little-endian i16.
            for (out, sample) in frame.data_mut(0).as_chunks_mut::<2>().0.iter_mut().zip(src) {
                out.copy_from_slice(&sample.to_le_bytes());
            }
            audio
                .track
                .encoder
                .send_frame(&frame)
                .map_err(error("encoding audio"))?;
            write_packets(&mut self.output, &mut audio.track)?;
            audio.written += count;
        }
        Ok(())
    }
}

fn add_audio(
    output: &mut format::context::Output,
    clip: &AudioClip,
    global_header: bool,
) -> Result<AudioTrack, MediaError> {
    let codec = encoder::find(codec::Id::PCM_S16LE)
        .ok_or_else(|| MediaError::Decode("this FFmpeg build has no PCM encoder".into()))?;
    let mut audio = codec::context::Context::new_with_codec(codec)
        .encoder()
        .audio()
        .map_err(decode_error)?;
    let time_base = ffmpeg::Rational::new(1, clip.sample_rate as i32);
    audio.set_rate(clip.sample_rate as i32);
    audio.set_format(Sample::I16(format::sample::Type::Packed));
    audio.set_channel_layout(ChannelLayout::default(clip.channels as i32));
    audio.set_time_base(time_base);
    if global_header {
        audio.set_flags(codec::Flags::GLOBAL_HEADER);
    }
    let audio = audio
        .open_as(codec)
        .map_err(error("opening the PCM encoder"))?;
    let stream = {
        let mut stream = output.add_stream(codec).map_err(decode_error)?;
        stream.set_parameters(&audio);
        stream.set_time_base(time_base);
        stream.index()
    };
    Ok(AudioTrack {
        track: Track {
            encoder: audio,
            stream,
            time_base,
        },
        samples: clip
            .samples
            .iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16)
            .collect(),
        channels: clip.channels as usize,
        sample_rate: clip.sample_rate,
        written: 0,
    })
}

/// Moves every packet the encoder has ready into the file.
fn write_packets<E>(
    output: &mut format::context::Output,
    track: &mut Track<E>,
) -> Result<(), MediaError>
where
    E: std::ops::DerefMut,
    E::Target: std::ops::DerefMut<Target = encoder::Encoder>,
{
    let encoder: &mut encoder::Encoder = &mut track.encoder;
    let encoder_time_base = encoder.time_base();
    let mut packet = Packet::empty();
    loop {
        match encoder.receive_packet(&mut packet) {
            Ok(()) => {
                packet.set_stream(track.stream);
                packet.rescale_ts(encoder_time_base, track.time_base);
                packet
                    .write_interleaved(output)
                    .map_err(error("writing to the file"))?;
            }
            Err(ffmpeg::Error::Other {
                errno: ffmpeg::util::error::EAGAIN,
            })
            | Err(ffmpeg::Error::Eof) => return Ok(()),
            Err(e) => return Err(error("encoding")(e)),
        }
    }
}
