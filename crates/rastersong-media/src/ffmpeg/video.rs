use std::path::{Path, PathBuf};
use std::sync::Arc;

use ffmpeg::codec::threading;
use ffmpeg::util::error::EAGAIN;
use ffmpeg::{Packet, decoder, ffi, format, frame, media};
use ffmpeg_next as ffmpeg;

use super::index::{FrameIndex, PacketMeta};
use super::scale::Scaler;
use super::{decode_error, find_stream, open_input};
use crate::rotate::rotate_rgb24;
use crate::{MediaError, Rational, Rotation, VideoFrame, VideoInfo, VideoSource};

/// How many earlier keyframes to try when a seek lands past the one we asked for, before
/// falling back to reopening the file and decoding from the start.
const SEEK_RETRIES: usize = 3;

/// Consecutive decoder errors tolerated before giving up on a stream.
const MAX_DECODE_ERRORS: usize = 16;

pub(crate) struct FfmpegVideoSource {
    path: PathBuf,
    input: format::context::Input,
    stream: usize,
    time_base: ffmpeg::Rational,
    decoder: decoder::Video,
    index: FrameIndex,
    info: VideoInfo,
    output_size: Option<(u32, u32)>,
    scaler: Scaler,
    /// The decoder's most recent output.
    decoded: frame::Video,
    /// Frame number of the decoder's most recent output, or `None` if the decoder needs a seek
    /// before it can produce anything useful.
    position: Option<usize>,
    eof_sent: bool,
    /// Whether seeking by decode timestamp worked last time (see [`Self::seek`]).
    seek_by_dts: bool,
    /// The most recently returned frame, so repeated requests are free.
    last: Option<(usize, Arc<VideoFrame>)>,
}

impl std::fmt::Debug for FfmpegVideoSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FfmpegVideoSource")
            .field("path", &self.path)
            .field("info", &self.info)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl FfmpegVideoSource {
    pub fn open(path: &Path, stream: Option<usize>) -> Result<Self, MediaError> {
        let mut input = open_input(path)?;
        let stream = find_stream(&input, media::Type::Video, stream)
            .ok_or_else(|| MediaError::NoVideoStream(path.to_owned()))?;
        let stream_index = stream.index();
        let time_base = stream.time_base();
        let avg_rate = stream.avg_frame_rate();
        let base_rate = stream.rate();
        let rotation = display_rotation(&stream);

        let mut context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
            .map_err(decode_error)?;
        context.set_threading(threading::Config {
            kind: threading::Type::Frame,
            ..Default::default()
        });
        let decoder = context.decoder().video().map_err(decode_error)?;

        // Corrupt packets (e.g. the last, cut-off packet of a truncated file) are never decoded,
        // so they aren't frames either. `read_packet` skips them the same way.
        let index = FrameIndex::build(input.packets().filter_map(|(s, packet)| {
            (s.index() == stream_index && !packet.is_corrupt()).then(|| PacketMeta {
                pts: packet.pts(),
                dts: packet.dts(),
                key: packet.is_key(),
            })
        }));
        if index.len() == 0 {
            return Err(MediaError::Open {
                path: path.to_owned(),
                reason: rastersong_lang::tr("error.media.no_decodable_frames").into(),
            });
        }

        let frame_rate = [avg_rate, base_rate]
            .into_iter()
            .find(|r| r.numerator() > 0 && r.denominator() > 0)
            .map(|r| Rational::new(r.numerator(), r.denominator()))
            .unwrap_or(Rational::new(30, 1));
        let (width, height) = if rotation.swaps_dimensions() {
            (decoder.height(), decoder.width())
        } else {
            (decoder.width(), decoder.height())
        };

        Ok(Self {
            path: path.to_owned(),
            input,
            stream: stream_index,
            time_base,
            decoder,
            info: VideoInfo {
                width,
                height,
                frame_count: index.len(),
                frame_rate,
                rotation,
            },
            index,
            output_size: None,
            scaler: Scaler::new()?,
            decoded: frame::Video::empty(),
            position: None,
            eof_sent: false,
            seek_by_dts: false,
            last: None,
        })
    }

    /// Reads the next intact packet of our stream. `None` at end of file or on an unrecoverable
    /// read error.
    fn read_packet(&mut self) -> Option<Packet> {
        loop {
            let mut packet = Packet::empty();
            match packet.read(&mut self.input) {
                Ok(()) if packet.stream() == self.stream && !packet.is_corrupt() => {
                    return Some(packet);
                }
                Ok(()) => {}
                Err(ffmpeg::Error::Eof) => return None,
                // A corrupt packet; the demuxer can resync past it.
                Err(ffmpeg::Error::InvalidData) => {}
                Err(e) => {
                    tracing::warn!(path = %self.path.display(), "read error, treating as end of file: {e}");
                    return None;
                }
            }
        }
    }

    fn send(&mut self, packet: &Packet) {
        if let Err(e) = self.decoder.send_packet(packet) {
            tracing::debug!(path = %self.path.display(), "decoder rejected a packet: {e}");
        }
    }

    /// Positions the decoder so that decoding forward reaches every frame from `key` on.
    fn seek(&mut self, key: usize) -> Result<(), MediaError> {
        let target = self.index.keyframe(key).pts;
        for try_key in (key.saturating_sub(SEEK_RETRIES)..=key).rev() {
            let k = self.index.keyframe(try_key);
            // Demuxers seek by presentation or decode time depending on the container. Try the
            // kind that worked last time first.
            let mut timestamps = vec![(k.pts, false)];
            if let Some(dts) = k.dts.filter(|&d| d != k.pts) {
                timestamps.insert(usize::from(!self.seek_by_dts), (dts, true));
            }
            for (ts, by_dts) in timestamps {
                if self.seek_to(ts, target) {
                    self.seek_by_dts = by_dts;
                    return Ok(());
                }
            }
            tracing::debug!(path = %self.path.display(), key, try_key, "seek overshot, retrying earlier");
        }
        self.restart()
    }

    /// Seeks the demuxer to `ts`, accepting the landing spot only if decoding starts at or before
    /// the keyframe shown at `target`. Demuxers seek imprecisely (by index, byte position or
    /// bisection), so this is checked rather than trusted.
    fn seek_to(&mut self, ts: i64, target: i64) -> bool {
        // SAFETY: input is a valid open context and the stream index is in range.
        let ret = unsafe {
            ffi::av_seek_frame(
                self.input.as_mut_ptr(),
                self.stream as i32,
                ts,
                ffi::AVSEEK_FLAG_BACKWARD,
            )
        };
        if ret < 0 {
            return false;
        }
        self.reset_decoder();
        match self.read_keyframe() {
            Some(packet) if packet.pts().or(packet.dts()).is_some_and(|t| t <= target) => {
                self.send(&packet);
                true
            }
            _ => false,
        }
    }

    /// Reopens the file and starts decoding from the first keyframe. Always correct, just slow.
    fn restart(&mut self) -> Result<(), MediaError> {
        tracing::debug!(path = %self.path.display(), "restarting decode from the beginning");
        self.input = open_input(&self.path)?;
        self.reset_decoder();
        let packet = self.read_keyframe().ok_or_else(|| {
            MediaError::Decode(rastersong_lang::tr("error.media.no_keyframe").into())
        })?;
        self.send(&packet);
        Ok(())
    }

    fn reset_decoder(&mut self) {
        self.decoder.flush();
        self.eof_sent = false;
        self.position = None;
    }

    /// Skips forward to the next keyframe packet of our stream.
    fn read_keyframe(&mut self) -> Option<Packet> {
        std::iter::from_fn(|| self.read_packet()).find(|p| p.is_key())
    }

    /// Decodes until the decoder outputs an indexed frame, returning its frame number, or `None`
    /// once the stream is exhausted. Unindexed output (undecodable leading frames) is skipped.
    fn decode_next(&mut self) -> Result<Option<usize>, MediaError> {
        let mut errors = 0;
        loop {
            match self.decoder.receive_frame(&mut self.decoded) {
                Ok(()) => {
                    errors = 0;
                    let pts = self.decoded.pts().or(self.decoded.timestamp());
                    if let Some(frame) = pts.and_then(|t| self.index.frame_at(t)) {
                        self.position = Some(frame);
                        return Ok(Some(frame));
                    }
                }
                Err(ffmpeg::Error::Other { errno: EAGAIN }) => match self.read_packet() {
                    Some(packet) => self.send(&packet),
                    None if !self.eof_sent => {
                        self.decoder.send_eof().map_err(decode_error)?;
                        self.eof_sent = true;
                    }
                    None => return Ok(None),
                },
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(e) => {
                    errors += 1;
                    if errors >= MAX_DECODE_ERRORS {
                        return Err(decode_error(e));
                    }
                    tracing::debug!(path = %self.path.display(), "decode error: {e}");
                }
            }
        }
    }

    /// Converts the decoder's current output to an upright RGB frame at the output size.
    fn convert(&mut self) -> Result<VideoFrame, MediaError> {
        let (width, height) = self
            .output_size
            .unwrap_or((self.info.width, self.info.height));
        let rotation = self.info.rotation;
        let (decoded_width, decoded_height) = if rotation.swaps_dimensions() {
            (height, width)
        } else {
            (width, height)
        };
        let mut data = self
            .scaler
            .convert(&self.decoded, decoded_width, decoded_height)?;
        if rotation != Rotation::None {
            data = rotate_rgb24(
                &data,
                decoded_width as usize,
                decoded_height as usize,
                rotation,
            );
        }
        Ok(VideoFrame {
            width,
            height,
            data,
        })
    }
}

impl VideoSource for FfmpegVideoSource {
    fn info(&self) -> &VideoInfo {
        &self.info
    }

    fn frame_time(&self, index: usize) -> f64 {
        let ticks = self.index.pts(index) - self.index.pts(0);
        ticks as f64 * f64::from(self.time_base)
    }

    fn set_output_size(&mut self, size: Option<(u32, u32)>) {
        if size != self.output_size {
            self.output_size = size;
            self.last = None;
        }
    }

    fn frame(&mut self, index: usize) -> Result<Arc<VideoFrame>, MediaError> {
        let count = self.index.len();
        if index >= count {
            return Err(MediaError::FrameOutOfRange { index, count });
        }
        if let Some((last, frame)) = &self.last
            && *last == index
        {
            return Ok(frame.clone());
        }

        // Decode forward if the decoder is already at or just before the keyframe a seek would
        // land on; a seek would only decode the same frames again. Otherwise seek.
        let key = self.index.start_key(index);
        let key_frame = self.index.keyframe(key).frame;
        let forward = self
            .position
            .is_some_and(|pos| pos < index && pos + 1 >= key_frame);
        if !forward {
            self.seek(key)?;
        }

        loop {
            let Some(decoded) = self.decode_next()? else {
                return Err(MediaError::FrameUnavailable(index));
            };
            if decoded < index {
                continue;
            }
            let frame = Arc::new(self.convert()?);
            self.last = Some((decoded, frame.clone()));
            // Overshooting means the decoder never produced the requested frame.
            return if decoded == index {
                Ok(frame)
            } else {
                Err(MediaError::FrameUnavailable(index))
            };
        }
    }
}

/// Reads the display matrix (e.g. from a phone video) and returns the clockwise rotation that
/// makes frames upright.
fn display_rotation(stream: &format::stream::Stream) -> Rotation {
    // SAFETY: codecpar is valid for the stream's lifetime; side data is checked for presence and size.
    unsafe {
        let par = (*stream.as_ptr()).codecpar;
        let side_data = ffi::av_packet_side_data_get(
            (*par).coded_side_data,
            (*par).nb_coded_side_data,
            ffi::AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX,
        );
        if side_data.is_null() || (*side_data).size < 9 * size_of::<i32>() {
            return Rotation::None;
        }
        let counter_clockwise = ffi::av_display_rotation_get((*side_data).data as *const i32);
        if counter_clockwise.is_nan() {
            Rotation::None
        } else {
            Rotation::from_degrees_cw(-counter_clockwise)
        }
    }
}
