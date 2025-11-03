// Core RasterSong library
// Video processing and effects

mod audio_player;
mod media_player;
mod video_decoder;

// Re-export the main types
pub use media_player::MediaPlayer;
pub use video_decoder::VideoFrame;

pub fn init() {
    // Initialize GStreamer
    gstreamer::init().expect("Failed to initialize GStreamer");
}
