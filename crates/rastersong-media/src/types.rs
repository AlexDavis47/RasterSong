use std::fmt::Debug;
use std::path::Path;
use std::sync::Arc;

use crate::MediaError;

/// A media backend: opens video sources and loads audio. [`crate::FfmpegBackend`] is the real
/// implementation; [`crate::FakeBackend`] produces synthetic media for tests.
pub trait MediaBackend: Send + Sync + Debug {
    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoSource>, MediaError>;

    /// Decodes an entire audio stream to interleaved `f32`.
    fn load_audio(&self, path: &Path, options: AudioOptions) -> Result<AudioClip, MediaError>;

    /// Whether the file has an audio stream, from its header alone (no decoding). False if the
    /// file can't be opened.
    fn has_audio(&self, path: &Path) -> bool;
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
}

/// A fully decoded audio stream.
#[derive(Clone, PartialEq)]
pub struct AudioClip {
    pub sample_rate: u32,
    pub channels: u32,
    /// Interleaved samples, nominally in `-1.0..=1.0`.
    pub samples: Vec<f32>,
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
