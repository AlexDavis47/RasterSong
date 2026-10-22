//! Media I/O for RasterSong: probing, frame index, video/audio decode, and export.
//!
//! Everything is reached through the [`MediaBackend`] trait. [`FfmpegBackend`] is the real
//! implementation and the only place FFmpeg is used; no FFmpeg type appears in this crate's
//! public API. [`FakeBackend`] serves synthetic media for testing the layers above.

mod cache;
mod error;
mod export_format;
mod fake;
mod ffmpeg;
mod rotate;
mod samples;
mod types;

pub use cache::AudioCache;
pub use error::MediaError;
pub use export_format::{AvailableFormat, ExportFormat, FormatKind, ProResProfile, Quality};
pub use fake::{FakeBackend, FakeVideo};
pub use ffmpeg::{
    BackendInfo, FfmpegBackend, LibraryInfo, MediaWriter, Version, backend_info, export_formats,
    init, lgpl_url,
};
pub use samples::Samples;
pub use types::{
    AudioClip, AudioOptions, MediaBackend, Rational, Rotation, StreamInfo, StreamKind, VideoFrame,
    VideoInfo, VideoSource,
};
