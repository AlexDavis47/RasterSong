//! RasterSong - Audio/Video processing library
use anyhow::Result;

pub mod media;

/// Initialize the library
/// This initializes GStreamer and should be called before using any media functionality
pub fn init() -> Result<()> {
    media::init()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        assert!(init().is_ok());
    }
}
