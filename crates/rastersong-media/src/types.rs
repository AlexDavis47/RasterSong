use std::fmt::Debug;
use std::path::Path;
use std::sync::Arc;

use crate::{MediaError, Samples};

/// A media backend: opens video sources and loads audio. [`crate::FfmpegBackend`] is the real
/// implementation; [`crate::FakeBackend`] produces synthetic media for tests.
pub trait MediaBackend: Send + Sync + Debug {
    /// Opens the file's best video stream.
    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoSource>, MediaError> {
        self.open_video_stream(path, None)
    }

    /// Opens the video stream with index `stream` in the file (as [`StreamInfo::index`] gives
    /// it), or its best video stream for `None`.
    fn open_video_stream(
        &self,
        path: &Path,
        stream: Option<usize>,
    ) -> Result<Box<dyn VideoSource>, MediaError>;

    /// The file's video and audio streams, in file order, from its header alone (no decoding).
    /// Cover pictures, subtitles and data streams are left out.
    fn streams(&self, path: &Path) -> Result<Vec<StreamInfo>, MediaError>;

    /// Decodes an entire audio stream to interleaved `f32`.
    fn load_audio(&self, path: &Path, options: AudioOptions) -> Result<AudioClip, MediaError>;

    /// Decodes an entire audio stream to interleaved `f32`, handing the samples to `sink` in
    /// pieces as they are decoded rather than holding them all, and returns the sample rate and
    /// channel count. Used to write audio to a cache file.
    fn decode_audio(
        &self,
        path: &Path,
        options: AudioOptions,
        sink: &mut dyn FnMut(&[f32]) -> Result<(), MediaError>,
    ) -> Result<(u32, u32), MediaError> {
        let clip = self.load_audio(path, options)?;
        sink(&clip.samples)?;
        Ok((clip.sample_rate, clip.channels))
    }

    /// Whether the file has an audio stream, from its header alone (no decoding). False if the
    /// file can't be opened.
    fn has_audio(&self, path: &Path) -> bool;

    /// Whether the file is there to open: false marks a resource missing.
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

/// Random access to the frames of one video stream, by frame index.
///
/// Frames are numbered `0..frame_count` in presentation order. Callers never see GOPs,
/// keyframes or timestamps. Requesting frames in increasing order is the fast path.
pub trait VideoSource: Send + Debug {
    fn info(&self) -> &VideoInfo;

    /// Presentation time of `index` in seconds, relative to the first frame. For variable
    /// frame rate sources this is the only accurate way to place a frame in time.
    fn frame_time(&self, index: usize) -> f64;

    /// Size of the frames returned by [`Self::frame`]. `None` (the default) is the display size,
    /// [`VideoInfo::width`] × [`VideoInfo::height`].
    fn set_output_size(&mut self, size: Option<(u32, u32)>);

    fn frame(&mut self, index: usize) -> Result<Arc<VideoFrame>, MediaError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    /// Display width, after rotation.
    pub width: u32,
    /// Display height, after rotation.
    pub height: u32,
    pub frame_count: usize,
    /// Average frame rate.
    pub frame_rate: Rational,
    /// Rotation applied to decoded frames so they display upright.
    pub rotation: Rotation,
}

/// One decoded frame as packed 8-bit RGB, rows tightly packed (`width * 3` bytes per row).
#[derive(Clone, PartialEq, Eq)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl VideoFrame {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let i = (y as usize * self.width as usize + x as usize) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }
}

impl Debug for VideoFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VideoFrame({}x{})", self.width, self.height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rational {
    pub num: i32,
    pub den: i32,
}

impl Rational {
    pub const fn new(num: i32, den: i32) -> Self {
        Self { num, den }
    }

    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

/// Clockwise rotation that turns a decoded frame upright.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    #[default]
    None,
    Cw90,
    Cw180,
    Cw270,
}

impl Rotation {
    /// Snaps an angle in degrees (clockwise) to the nearest quarter turn.
    pub fn from_degrees_cw(degrees: f64) -> Self {
        match ((degrees / 90.0).round() as i64).rem_euclid(4) {
            0 => Self::None,
            1 => Self::Cw90,
            2 => Self::Cw180,
            _ => Self::Cw270,
        }
    }

    pub fn swaps_dimensions(self) -> bool {
        matches!(self, Self::Cw90 | Self::Cw270)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioOptions {
    /// Resample to this rate. `None` keeps the source rate.
    pub sample_rate: Option<u32>,
    /// Remix to this many channels. `None` keeps the source channel count.
    pub channels: Option<u32>,
    /// The audio stream to decode, by its index in the file ([`StreamInfo::index`]). `None` is
    /// the file's best audio stream.
    pub stream: Option<usize>,
}

/// One video or audio stream of a media file.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamInfo {
    /// The stream's index in the file, counting every stream.
    pub index: usize,
    pub kind: StreamKind,
    /// The codec's short name, such as `h264` or `aac`.
    pub codec: String,
    /// The `title` tag, if the file names the stream.
    pub title: Option<String>,
    /// The `language` tag, if the file has one.
    pub language: Option<String>,
    /// Whether the file marks the stream as the default of its kind.
    pub default: bool,
}

/// What a [`StreamInfo`] carries, with its basic shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StreamKind {
    Video {
        width: u32,
        height: u32,
        /// Frames per second, 0 when the file doesn't say.
        frame_rate: f64,
    },
    Audio {
        sample_rate: u32,
        channels: u32,
    },
}

impl StreamKind {
    pub fn is_video(&self) -> bool {
        matches!(self, Self::Video { .. })
    }
}

/// A fully decoded audio stream.
#[derive(Clone, PartialEq)]
pub struct AudioClip {
    pub sample_rate: u32,
    pub channels: u32,
    /// Interleaved samples, nominally in `-1.0..=1.0`, in memory or memory-mapped from a cache
    /// file. Cloning a clip shares them.
    pub samples: Samples,
}

impl AudioClip {
    /// Number of sample frames (samples per channel).
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    pub fn duration_secs(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }
}

impl Debug for AudioClip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AudioClip({} Hz, {} ch, {} frames)",
            self.sample_rate,
            self.channels,
            self.frames()
        )
    }
}
