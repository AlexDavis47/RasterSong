//! The RasterSong render service: sequential renderer, warmup on seek, frame cache,
//! cancellation and the playback clock.
//!
//! The CLI and GUI talk only to this crate; they never decode or schedule anything themselves.

mod offline;
pub mod sources;

pub use offline::{
    AUDIO_SOURCE, EngineError, FrameSink, RenderInfo, RenderSettings, RenderedFrame, VIDEO_SOURCE,
    render,
};
pub use rastersong_graph::{GraphDesc, GraphError};
pub use rastersong_media::{
    AudioClip, AudioOptions, BackendInfo, FfmpegBackend, LibraryInfo, LosslessWriter, MediaBackend,
    MediaError, Rational, Version,
};

/// Initializes the engine and its media backend, and reports what was loaded.
pub fn init() -> Result<BackendInfo, MediaError> {
    rastersong_media::init()?;
    Ok(rastersong_media::backend_info())
}
