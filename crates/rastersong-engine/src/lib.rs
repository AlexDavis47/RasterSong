//! The RasterSong render service: sequential renderer, warmup on seek, frame cache,
//! cancellation and the playback clock.
//!
//! The CLI and GUI talk only to this crate; they never decode or schedule anything themselves.

mod cache;
mod clock;
mod error;
mod offline;
pub mod playback;
mod project;
mod renderer;
mod service;
pub mod sources;
mod thumbnails;
pub mod waveform;

pub use cache::{CacheKey, Frame, FrameCache};
pub use clock::PlaybackClock;
pub use error::EngineError;
pub use offline::{FrameSink, RenderSettings, RenderedFrame, render};
pub use project::{PROJECT_EXTENSION, PROJECT_VERSION, Project, ProjectTrack};
pub use rastersong_graph::{
    Category, Channels, Connection, FORMAT_VERSION, GraphDesc, GraphError, Interpolation, ModMode,
    ModScale, Modulation, NodeDesc, NodeType, OutputLevel, ParamKind, ParamLevel, ParamSpec,
    ParamValue, PortHint, Registry,
};
pub use rastersong_media::{
    AudioClip, AudioOptions, BackendInfo, FakeBackend, FakeVideo, FfmpegBackend, LibraryInfo,
    LosslessWriter, MediaBackend, MediaError, Rational, Version, VideoFrame, VideoInfo,
};
pub use renderer::{
    AudioTrack, DEFAULT_AUDIO_TRACK, OutputSize, RenderInfo, Renderer, VIDEO_SOURCE,
};
pub use service::{
    AudioTrackSpec, Engine, EngineConfig, EngineStatus, Failure, LoadedTrack, PreviewScale,
};
pub use thumbnails::{THUMBNAIL_HEIGHT, Thumbnails};
pub use waveform::Waveform;

/// Initializes the engine and its media backend, and reports what was loaded.
pub fn init() -> Result<BackendInfo, MediaError> {
    rastersong_media::init()?;
    Ok(rastersong_media::backend_info())
}
