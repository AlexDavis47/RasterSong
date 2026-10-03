//! The RasterSong render service: sequential renderer, warmup on seek, frame cache,
//! cancellation and the playback clock.
//!
//! The CLI and GUI talk only to this crate; they never decode or schedule anything themselves.

pub use rastersong_media::{BackendInfo, LibraryInfo, MediaError, Version};

/// Initializes the engine and its media backend, and reports what was loaded.
pub fn init() -> Result<BackendInfo, MediaError> {
    rastersong_media::init()?;
    Ok(rastersong_media::backend_info())
}
