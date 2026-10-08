//! Media I/O for RasterSong: probing, frame index, video/audio decode and encode.
//!
//! Everything is reached through the [`MediaBackend`] trait. [`FfmpegBackend`] is the real
//! implementation and the only place FFmpeg is used; no FFmpeg type appears in this crate's
//! public API. [`FakeBackend`] serves synthetic media for testing the layers above.

mod cache;
mod error;
mod fake;
mod ffmpeg;
mod rotate;
mod samples;
mod types;

pub use cache::AudioCache;
pub use error::MediaError;
pub use fake::{FakeBackend, FakeVideo};
pub use ffmpeg::{
    BackendInfo, FfmpegBackend, LibraryInfo, LosslessWriter, Version, backend_info, init,
};
pub use samples::Samples;
pub use types::{
    AudioClip, AudioOptions, MediaBackend, Rational, Rotation, VideoFrame, VideoInfo, VideoSource,
};
