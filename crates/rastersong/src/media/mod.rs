//! Media module for file I/O, caching, decoding and encoding frames
//!
//! This module provides a public-facing GStreamer API for RasterSong.
//! It handles file I/O, caching, decoding and encoding frames.

pub mod audio;
pub mod gstreamer;
pub mod media_store;
pub mod video;

#[cfg(test)]
mod tests;

// Re-export initialization
pub use gstreamer::{init, is_initialized};

// Re-export identifier types and functions
pub use media_store::{
    MediaFileInfo, MediaId, MediaType, get_file_info, list_media_files, register_audio_file,
    register_video_file,
};

// Re-export video decoder
pub use video::decode_video;

// Re-export audio decoder
pub use audio::decode_audio;
