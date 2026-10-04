//! The RasterSong render service: sequential renderer, warmup on seek, frame cache,
//! cancellation and the playback clock.
//!
//! The CLI and GUI talk only to this crate; they never decode or schedule anything themselves.

mod cache;
mod clock;
mod error;
mod offline;
mod renderer;
mod service;
pub mod sources;

pub use cache::{CacheKey, Frame, FrameCache};
pub use clock::PlaybackClock;
pub use error::EngineError;
pub use offline::{FrameSink, RenderSettings, RenderedFrame, render};
pub use rastersong_graph::{GraphDesc, GraphError, Registry};
pub use rastersong_media::{
    AudioClip, AudioOptions, BackendInfo, FakeBackend, FakeVideo, FfmpegBackend, LibraryInfo,
    LosslessWriter, MediaBackend, MediaError, Rational, Version,
};
pub use renderer::{AUDIO_SOURCE, OutputSize, RenderInfo, Renderer, VIDEO_SOURCE};
pub use service::{Engine, EngineConfig, EngineStatus, PreviewScale};

/// Initializes the engine and its media backend, and reports what was loaded.
pub fn init() -> Result<BackendInfo, MediaError> {
    rastersong_media::init()?;
    Ok(rastersong_media::backend_info())
}
