// Core RasterSong library
// Video processing and effects will go here

pub fn init() {
    // Initialize GStreamer
    gstreamer::init().expect("Failed to initialize GStreamer");
}
