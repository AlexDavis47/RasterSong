//! Media module for file I/O, caching, decoding and encoding frames
//!
//! This module provides a public-facing FFmpeg API for RasterSong.
//! It handles file I/O, caching, decoding and encoding frames.

pub mod audio_decoder;
pub mod audio_samples;
pub mod ffmpeg;
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

// Re-export MediaStore functions
pub use media_store::{
    MediaFileInfo,
    decode_frames,
    decode_samples,
    get_audio_duration,
    get_audio_info,
    get_file_info,
    get_frame_boundaries,
    get_media_info,
    get_video_duration,
    get_video_info,
    list_media,
    load_media,
    register_audio_file,
    // Backward compatibility exports
    register_video_file,
    remove_media,
    remove_media_file,
};
