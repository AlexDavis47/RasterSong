// Core RasterSong library
// Video processing and effects

pub mod audio_effects;
pub mod audio_loader;
mod audio_player;
mod media_player;
mod video_decoder;
pub mod video_signal;
pub mod video_signal_processor;

// Re-export the main types
pub use media_player::MediaPlayer;
pub use video_decoder::VideoFrame;
pub use video_signal::{MappingMode, VideoSignalCodec, VideoSignalMetadata};
pub use video_signal_processor::{EffectSettings, InterpolationMode, VideoSignalProcessor};

pub fn init() {
    // Initialize GStreamer
    gstreamer::init().expect("Failed to initialize GStreamer");
}
