//! GStreamer initialization and configuration

use anyhow::Result;
use std::sync::Once;

static INIT: Once = Once::new();

/// Initialize GStreamer. Safe to call multiple times - will only initialize once.
pub fn init() -> Result<()> {
    INIT.call_once(|| {
        gstreamer::init().expect("Failed to initialize GStreamer");
    });
    Ok(())
}

/// Check if GStreamer has been initialized
pub fn is_initialized() -> bool {
    INIT.is_completed()
}
