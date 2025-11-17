//! FFmpeg initialization and configuration

use anyhow::Result;
use std::sync::Once;

static INIT: Once = Once::new();

/// Initialize FFmpeg. Safe to call multiple times - will only initialize once.
///
/// This performs global FFmpeg library initialization. Must be called before
/// any FFmpeg operations are performed.
pub fn init() -> Result<()> {
    INIT.call_once(|| {
        // Initialize ffmpeg-next library
        ffmpeg_next::init().expect("Failed to initialize FFmpeg");
    });
    Ok(())
}

/// Check if FFmpeg has been initialized
pub fn is_initialized() -> bool {
    INIT.is_completed()
}
