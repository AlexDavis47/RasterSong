//! Writes rendered frames and sound to a file in one of the [`ExportFormat`]s. Used by export,
//! the CLI and tests.

use std::path::Path;
use std::sync::OnceLock;

use ffmpeg::codec::{self, encoder};
use ffmpeg::format::{self, Pixel, Sample, sample};
use ffmpeg::{ChannelLayout, Dictionary, Packet, ffi, frame};
use ffmpeg_next as ffmpeg;
use rastersong_lang::tr_args;

use super::scale::Scaler;
use crate::{
    AudioClip, AvailableFormat, ExportFormat, FormatKind, MediaError, ProResProfile, Rational,
};

/// Audio is written in chunks of at most this many samples per channel when the encoder takes
/// any frame size.
const AUDIO_CHUNK: usize = 4096;

/// H.264 encoders, most preferred first. Only those built into the loaded FFmpeg and usable on
/// this machine (driver and hardware present) open.
const H264_ENCODERS: &[&str] = &[
    "h264_nvenc",
    "h264_amf",
    "h264_qsv",
    "h264_videotoolbox",
    "h264_mf",
];

/// How a format is written: container, video encoders to try with their pixel formats, and
/// the audio encoder.
struct Spec {
    muxer: &'static str,
    video_encoders: &'static [&'static str],
    /// Pixel formats to try, most preferred first.
    pixels: &'static [Pixel],
    /// The pixels are RGB; otherwise they are BT.709 limited-range YUV.
    rgb: bool,
    audio_encoder: &'static str,
}

fn spec(format: ExportFormat) -> Spec {
    match format {
        ExportFormat::H264 { .. } => Spec {
            muxer: "mp4",
            video_encoders: H264_ENCODERS,
            pixels: &[Pixel::YUV420P, Pixel::NV12],
            rgb: false,
            audio_encoder: "aac",
        },
        ExportFormat::ProRes { .. } => Spec {
            muxer: "mov",
            video_encoders: &["prores_ks"],
            pixels: &[Pixel::YUV422P10LE],
            rgb: false,
            audio_encoder: "pcm_s24le",
        },
        ExportFormat::Ffv1 => Spec {
            muxer: "matroska",
            video_encoders: &["ffv1"],
            // FFV1's 8-bit RGB format is packed B, G, R, unused.
            pixels: &[Pixel::BGRZ],
            rgb: true,
            audio_encoder: "pcm_s16le",
        },
    }
}

/// Encoder options a format sets beyond size, rate and pixel format.
fn video_options(format: ExportFormat) -> Dictionary<'static> {
    let mut options = Dictionary::new();
    match format {
        ExportFormat::H264 { .. } => {}
        ExportFormat::ProRes { profile } => {
            let name = match profile {
                ProResProfile::Proxy => "proxy",
                ProResProfile::Lt => "lt",
                ProResProfile::Standard => "standard",
                ProResProfile::Hq => "hq",
            };
            options.set("profile", name);
            // Tag the stream as Apple's, which some players require.
            options.set("vendor", "apl0");
        }
        ExportFormat::Ffv1 => options.set("level", "3"),
    }
    options
}

/// Writes packed RGB8 frames and interleaved `f32` sound to a file in an [`ExportFormat`].
pub struct MediaWriter {
    output: format::context::Output,
    video: Track<encoder::video::Encoder>,
    video_encoder: &'static str,
    /// The frame being written, as given.
    rgb: frame::Video,
    /// The frame in the encoder's pixel format.
    frame: frame::Video,
    rgb_output: bool,
    scaler: Scaler,
    frame_rate: Rational,
    frames_written: i64,
    audio: Option<AudioTrack>,
}

impl std::fmt::Debug for MediaWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaWriter")
            .field("video_encoder", &self.video_encoder)
            .field("frames_written", &self.frames_written)
            .finish_non_exhaustive()
    }
}

struct Track<E> {
    encoder: E,
    stream: usize,
    time_base: ffmpeg::Rational,
    /// The duration given to packets the encoder leaves without one, in the encoder's time
    /// base; 0 leaves them. MP4 and QuickTime cut a last frame that has no duration.
    packet_duration: i64,
}

struct AudioTrack {
    track: Track<encoder::audio::Encoder>,
    /// Interleaved samples to write.
    samples: Vec<f32>,
    channels: usize,
    sample_rate: u32,
    format: Sample,
    /// Samples per channel in each frame the encoder takes; `None` takes any size.
    frame_size: Option<usize>,
    /// Sample frames written so far.
    written: usize,
}

fn error(context: &str) -> impl Fn(ffmpeg::Error) -> MediaError + '_ {
    move |e| MediaError::Encode(format!("{context}: {e}"))
}

impl MediaWriter {
    /// Opens `path` for `format`, with the whole sound of `audio` if given.
    pub fn create(
        path: &Path,
        format: ExportFormat,
        width: u32,
        height: u32,
        frame_rate: Rational,
        audio: Option<&AudioClip>,
    ) -> Result<Self, MediaError> {
        let mut writer = Self::open(
            path,
            format,
            width,
            height,
            frame_rate,
            audio
                .filter(|clip| !clip.samples.is_empty())
                .map(|clip| (clip.sample_rate, clip.channels)),
        )?;
        if let Some(clip) = audio {
            writer.push_audio(&clip.samples);
        }
        Ok(writer)
    }

    /// Like [`Self::create`], with an audio track of `sample_rate` and `channels` whose samples
    /// arrive as the render goes, through [`Self::push_audio`]. Push each frame's audio before
    /// writing the frame: audio is written as far as the video has got.
    pub fn create_streaming(
        path: &Path,
        format: ExportFormat,
        width: u32,
        height: u32,
        frame_rate: Rational,
        sample_rate: u32,
        channels: u32,
    ) -> Result<Self, MediaError> {
        Self::open(
            path,
            format,
            width,
            height,
            frame_rate,
            Some((sample_rate, channels)),
        )
    }

    /// FFmpeg's name for the encoder writing the video, e.g. `h264_nvenc`.
    pub fn video_encoder(&self) -> &'static str {
        self.video_encoder
    }

    /// How many threads the video encoder runs on, as FFmpeg reports it once open.
    pub fn video_threads(&self) -> usize {
        // SAFETY: the encoder's context is valid while the writer lives; this is a plain field.
        let count = unsafe { (*self.video.encoder.as_ptr()).thread_count };
        count.max(1) as usize
    }

    /// Adds interleaved samples to the audio track, if there is one. Values are clipped to
    /// `-1..=1` when the format stores integers.
    pub fn push_audio(&mut self, samples: &[f32]) {
        if let Some(audio) = &mut self.audio {
            audio.samples.extend_from_slice(samples);
        }
    }

    fn open(
        path: &Path,
        format: ExportFormat,
        width: u32,
        height: u32,
        frame_rate: Rational,
        audio: Option<(u32, u32)>,
    ) -> Result<Self, MediaError> {
        super::init()?;
        if let Some(problem) = format.kind().size_problem(width, height) {
            return Err(MediaError::Encode(problem));
        }
        let spec = spec(format);
        let mut output = format::output_as(path, spec.muxer).map_err(|e| MediaError::Open {
            path: path.to_owned(),
            reason: e.to_string(),
        })?;
        let global_header = output
            .format()
            .flags()
            .contains(format::Flags::GLOBAL_HEADER);

        let probed = export_formats()
            .iter()
            .find(|f| f.kind == format.kind())
            .map(|f| f.video_encoder);
        let opened = open_video_encoder(format, width, height, frame_rate, global_header, probed)?;
        let video_stream = {
            let mut stream = output.add_stream(opened.codec).map_err(encode_error)?;
            stream.set_parameters(&opened.encoder);
            stream.set_time_base(opened.encoder.time_base());
            stream.index()
        };

        let audio = audio
            .map(|(rate, channels)| {
                add_audio(
                    &mut output,
                    spec.audio_encoder,
                    rate,
                    channels,
                    global_header,
                )
            })
            .transpose()?;

        let mut options = Dictionary::new();
        if matches!(spec.muxer, "mp4" | "mov") {
            // The index goes at the front, so playback can start before the whole file loads.
            options.set("movflags", "+faststart");
        }
        output
            .write_header_with(options)
            .map_err(error("writing the file header"))?;
        // The muxer may change stream time bases when writing the header.
        let stream_time_base = |output: &format::context::Output, index| {
            output.stream(index).expect("stream exists").time_base()
        };
        let video = Track {
            time_base: stream_time_base(&output, video_stream),
            encoder: opened.encoder,
            stream: video_stream,
            // The encoder's time base is one frame.
            packet_duration: 1,
        };
        let audio = audio.map(|mut a| {
            a.track.time_base = stream_time_base(&output, a.track.stream);
            a
        });

        let mut rgb = frame::Video::new(Pixel::RGB24, width, height);
        tag_frame(&mut rgb, true);
        Ok(Self {
            output,
            video,
            video_encoder: opened.name,
            rgb,
            frame: frame::Video::new(opened.pixel, width, height),
            rgb_output: spec.rgb,
            scaler: Scaler::new()?,
            frame_rate,
            frames_written: 0,
            audio,
        })
    }

    /// Appends one frame of packed RGB8 (`width * height * 3` bytes).
    pub fn write_frame(&mut self, rgb: &[u8]) -> Result<(), MediaError> {
        let (width, height) = (self.rgb.width() as usize, self.rgb.height() as usize);
        assert_eq!(
            rgb.len(),
            width * height * 3,
            "frame size does not match the writer"
        );
        let stride = self.rgb.stride(0);
        let data = self.rgb.data_mut(0);
        for (y, src) in rgb.chunks_exact(width * 3).enumerate() {
            data[y * stride..y * stride + width * 3].copy_from_slice(src);
        }
        // The encoder may still hold a reference to the previous frame's buffers.
        // SAFETY: the frame is a valid, allocated video frame.
        let ret = unsafe { ffi::av_frame_make_writable(self.frame.as_mut_ptr()) };
        if ret < 0 {
            return Err(encode_error(ffmpeg::Error::from(ret)));
        }
        tag_frame(&mut self.frame, self.rgb_output);
        self.scaler.scale_into(&self.rgb, &mut self.frame)?;
        self.frame.set_pts(Some(self.frames_written));
        self.frames_written += 1;
        self.video
            .encoder
            .send_frame(&self.frame)
            .map_err(error("encoding video"))?;
        write_packets(&mut self.output, &mut self.video)?;

        self.write_audio_until(self.video_end(), false)
    }

    /// Flushes the encoders and finishes the file. Audio stops where the video does.
    pub fn finish(mut self) -> Result<(), MediaError> {
        self.video
            .encoder
            .send_eof()
            .map_err(error("encoding video"))?;
        write_packets(&mut self.output, &mut self.video)?;
        self.write_audio_until(self.video_end(), true)?;
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

    fn video_end(&self) -> f64 {
        self.frames_written as f64 / self.frame_rate.as_f64()
    }

    /// Writes audio up to `seconds`, keeping it interleaved with the video. An encoder with a
    /// fixed frame size gets whole frames only, unless this is the `last` call.
    fn write_audio_until(&mut self, seconds: f64, last: bool) -> Result<(), MediaError> {
        let Some(audio) = &mut self.audio else {
            return Ok(());
        };
        let total = audio.samples.len() / audio.channels;
        let target = ((seconds * f64::from(audio.sample_rate)).round() as usize).min(total);
        let chunk = audio.frame_size.unwrap_or(AUDIO_CHUNK);
        while audio.written < target {
            let count = (target - audio.written).min(chunk);
            if count < chunk && audio.frame_size.is_some() && !last {
                break;
            }
            let mut frame = frame::Audio::new(
                audio.format,
                count,
                ChannelLayout::default(audio.channels as i32),
            );
            frame.set_rate(audio.sample_rate);
            frame.set_pts(Some(audio.written as i64));
            let start = audio.written * audio.channels;
            fill_audio(
                &mut frame,
                &audio.samples[start..start + count * audio.channels],
                audio.channels,
            );
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

/// A video encoder opened for a format.
struct OpenedVideo {
    encoder: encoder::video::Encoder,
    codec: ffmpeg::Codec,
    name: &'static str,
    pixel: Pixel,
}

/// Opens the first of `format`'s video encoders that works here, trying each pixel format.
/// `first` is tried before the others, e.g. the one [`export_formats`] found.
fn open_video_encoder(
    format: ExportFormat,
    width: u32,
    height: u32,
    frame_rate: Rational,
    global_header: bool,
    first: Option<&'static str>,
) -> Result<OpenedVideo, MediaError> {
    let spec = spec(format);
    // Encoders that can't open here (no driver, no GPU, a size too small) log errors.
    let _quiet = QuietLog::new();
    let mut last_error = None;
    let names = first.into_iter().chain(
        spec.video_encoders
            .iter()
            .copied()
            .filter(|&n| Some(n) != first),
    );
    for name in names {
        let Some(codec) = encoder::find_by_name(name) else {
            continue;
        };
        for &pixel in spec.pixels {
            match configure_video(codec, format, &spec, pixel, width, height, frame_rate).and_then(
                |mut video| {
                    if global_header {
                        video.set_flags(codec::Flags::GLOBAL_HEADER);
                    }
                    video
                        .open_as_with(codec, video_options(format))
                        .map_err(encode_error)
                },
            ) {
                Ok(encoder) => {
                    return Ok(OpenedVideo {
                        encoder,
                        codec,
                        name,
                        pixel,
                    });
                }
                Err(e) => {
                    tracing::debug!(encoder = name, ?pixel, "encoder did not open: {e}");
                    last_error = Some(format!("{name}: {e}"));
                }
            }
        }
    }
    Err(MediaError::Encode(tr_args(
        "error.media.no_encoder",
        &[
            ("format", format.kind().extension()),
            ("reason", last_error.as_deref().unwrap_or("-")),
        ],
    )))
}

fn configure_video(
    codec: ffmpeg::Codec,
    format: ExportFormat,
    spec: &Spec,
    pixel: Pixel,
    width: u32,
    height: u32,
    frame_rate: Rational,
) -> Result<encoder::video::Video, MediaError> {
    use ffmpeg::util::color;

    let mut context = codec::context::Context::new_with_codec(codec);
    // FFmpeg's default is one thread, which leaves the CPU encoders (ProRes, FFV1) on one core.
    // 0 lets it pick a thread per core, splitting frames or slices, whichever the encoder
    // supports; the thread type keeps its default of both. Hardware encoders ignore it.
    // SAFETY: the context is valid and not yet open; this is a plain field.
    unsafe { (*context.as_mut_ptr()).thread_count = 0 };
    let mut video = context.encoder().video().map_err(encode_error)?;
    video.set_width(width);
    video.set_height(height);
    video.set_format(pixel);
    video.set_time_base(ffmpeg::Rational::new(frame_rate.den, frame_rate.num));
    video.set_frame_rate(Some(ffmpeg::Rational::new(frame_rate.num, frame_rate.den)));
    if spec.rgb {
        video.set_colorspace(color::Space::RGB);
        video.set_color_range(color::Range::JPEG);
    } else {
        video.set_colorspace(color::Space::BT709);
        video.set_color_range(color::Range::MPEG);
        video.set_color_primaries(color::Primaries::BT709);
        video.set_color_transfer_characteristic(color::TransferCharacteristic::BT709);
    }
    if let ExportFormat::H264 { quality } = format {
        let fps = frame_rate.as_f64();
        let bits = quality.bits_per_pixel() * f64::from(width * height) * fps;
        video.set_bit_rate(bits.round() as usize);
        // A keyframe every two seconds keeps seeking quick in players and editors.
        video.set_gop((fps * 2.0).round().max(1.0) as u32);
    }
    Ok(video)
}

/// Sets the color tags of `frame`, which tell the scaler how to convert: full-range RGB, or
/// BT.709 limited-range YUV.
fn tag_frame(frame: &mut frame::Video, rgb: bool) {
    // SAFETY: the frame is valid; these are plain fields.
    unsafe {
        let f = &mut *frame.as_mut_ptr();
        if rgb {
            f.colorspace = ffi::AVColorSpace::AVCOL_SPC_RGB;
            f.color_range = ffi::AVColorRange::AVCOL_RANGE_JPEG;
        } else {
            f.colorspace = ffi::AVColorSpace::AVCOL_SPC_BT709;
            f.color_range = ffi::AVColorRange::AVCOL_RANGE_MPEG;
            f.color_primaries = ffi::AVColorPrimaries::AVCOL_PRI_BT709;
            f.color_trc = ffi::AVColorTransferCharacteristic::AVCOL_TRC_BT709;
        }
    }
}

/// The sample formats audio can be written in, most preferred first.
const SAMPLE_FORMATS: [Sample; 6] = [
    Sample::F32(sample::Type::Packed),
    Sample::F32(sample::Type::Planar),
    Sample::I32(sample::Type::Packed),
    Sample::I32(sample::Type::Planar),
    Sample::I16(sample::Type::Packed),
    Sample::I16(sample::Type::Planar),
];

fn add_audio(
    output: &mut format::context::Output,
    name: &str,
    sample_rate: u32,
    channels: u32,
    global_header: bool,
) -> Result<AudioTrack, MediaError> {
    let codec = encoder::find_by_name(name).ok_or_else(|| {
        MediaError::Encode(tr_args(
            "error.media.no_encoder",
            &[("format", name), ("reason", "-")],
        ))
    })?;
    let supported: Vec<Sample> = codec
        .audio()
        .map_err(encode_error)?
        .formats()
        .map(Iterator::collect)
        .unwrap_or_default();
    let format = SAMPLE_FORMATS
        .into_iter()
        .find(|f| supported.is_empty() || supported.contains(f))
        .ok_or_else(|| MediaError::Encode(format!("{name}: no supported sample format")))?;
    let mut audio = codec::context::Context::new_with_codec(codec)
        .encoder()
        .audio()
        .map_err(encode_error)?;
    let time_base = ffmpeg::Rational::new(1, sample_rate as i32);
    audio.set_rate(sample_rate as i32);
    audio.set_format(format);
    audio.set_channel_layout(ChannelLayout::default(channels as i32));
    audio.set_time_base(time_base);
    // Lossy codecs: 96 kb/s per channel. PCM ignores it.
    audio.set_bit_rate(96_000 * channels as usize);
    if global_header {
        audio.set_flags(codec::Flags::GLOBAL_HEADER);
    }
    let audio = audio.open_as(codec).map_err(error(name))?;
    let any_size = codec
        .capabilities()
        .contains(codec::Capabilities::VARIABLE_FRAME_SIZE);
    let frame_size = match audio.frame_size() as usize {
        0 => None,
        _ if any_size => None,
        n => Some(n),
    };
    let stream = {
        let mut stream = output.add_stream(codec).map_err(encode_error)?;
        stream.set_parameters(&audio);
        stream.set_time_base(time_base);
        stream.index()
    };
    Ok(AudioTrack {
        track: Track {
            encoder: audio,
            stream,
            time_base,
            packet_duration: 0,
        },
        samples: Vec::new(),
        channels: channels as usize,
        sample_rate,
        format,
        frame_size,
        written: 0,
    })
}

/// Copies interleaved `src` into `frame` in the frame's sample format.
fn fill_audio(frame: &mut frame::Audio, src: &[f32], channels: usize) {
    fn put<const N: usize>(
        frame: &mut frame::Audio,
        src: &[f32],
        channels: usize,
        planar: bool,
        bytes: impl Fn(f32) -> [u8; N],
    ) {
        if planar {
            for channel in 0..channels {
                let plane = frame.data_mut(channel);
                for (out, &s) in plane
                    .as_chunks_mut::<N>()
                    .0
                    .iter_mut()
                    .zip(src.iter().skip(channel).step_by(channels))
                {
                    *out = bytes(s);
                }
            }
        } else {
            for (out, &s) in frame.data_mut(0).as_chunks_mut::<N>().0.iter_mut().zip(src) {
                *out = bytes(s);
            }
        }
    }
    let int = |s: f32, max: f64| (f64::from(s.clamp(-1.0, 1.0)) * max).round();
    match frame.format() {
        Sample::F32(kind) => put(frame, src, channels, kind == sample::Type::Planar, |s| {
            s.to_le_bytes()
        }),
        Sample::I32(kind) => put(frame, src, channels, kind == sample::Type::Planar, |s| {
            (int(s, f64::from(i32::MAX)) as i32).to_le_bytes()
        }),
        Sample::I16(kind) => put(frame, src, channels, kind == sample::Type::Planar, |s| {
            (int(s, f64::from(i16::MAX)) as i16).to_le_bytes()
        }),
        other => unreachable!("sample format {other:?} is not in SAMPLE_FORMATS"),
    }
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
                if packet.duration() == 0 {
                    packet.set_duration(track.packet_duration);
                }
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

/// Silences FFmpeg's log while it lives.
struct QuietLog(Option<ffmpeg::util::log::Level>);

impl QuietLog {
    fn new() -> Self {
        let level = ffmpeg::util::log::get_level().ok();
        ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Quiet);
        Self(level)
    }
}

impl Drop for QuietLog {
    fn drop(&mut self) {
        if let Some(level) = self.0 {
            ffmpeg::util::log::set_level(level);
        }
    }
}

fn encode_error(e: ffmpeg::Error) -> MediaError {
    MediaError::Encode(e.to_string())
}

/// The export formats the loaded FFmpeg can write on this machine, each with the video encoder
/// that will write it. Probed once, by opening each format's encoders at a small size.
pub fn export_formats() -> &'static [AvailableFormat] {
    static FORMATS: OnceLock<Vec<AvailableFormat>> = OnceLock::new();
    FORMATS.get_or_init(|| {
        if super::init().is_err() {
            return Vec::new();
        }
        FormatKind::ALL
            .into_iter()
            .filter_map(|kind| {
                open_video_encoder(
                    kind.default_format(),
                    256,
                    256,
                    Rational::new(30, 1),
                    false,
                    None,
                )
                .ok()
                .map(|opened| AvailableFormat {
                    kind,
                    video_encoder: opened.name,
                })
            })
            .collect()
    })
}
