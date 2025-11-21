//! Media module for file I/O, caching, decoding and encoding frames
//!
//! This module provides a public-facing FFmpeg API for RasterSong.
//! It handles file I/O, caching, decoding and encoding frames.

pub mod audio_decoder;
pub mod audio_samples;
pub mod ffmpeg;
pub mod frame_cache;
pub mod frame_metadata;
pub mod media_file;
pub mod media_id;
pub mod media_store;
pub mod video_decoder;
pub mod video_frame;

#[cfg(test)]
mod tests;

// Re-export FFmpeg initialization
pub use ffmpeg::{init, is_initialized};

// Re-export types
pub use audio_samples::{AudioFormat, AudioSamples};
pub use media_file::MediaFile;
pub use media_id::MediaId;
pub use video_frame::{PixelFormat, SerializableVideoFrame, VideoFrame};

// Re-export MediaStore
pub use media_store::MediaStore;
