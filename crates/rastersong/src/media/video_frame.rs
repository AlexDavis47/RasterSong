//! Video frame wrapper for decoded video frames

use anyhow::Result;
use ffmpeg_next as ffmpeg;
use serde::{Deserialize, Serialize};

/// A decoded video frame with pixel data
#[derive(Clone, Debug)]
pub struct VideoFrame {
    /// Frame width in pixels
    width: u32,
    /// Frame height in pixels
    height: u32,
    /// Pixel format
    format: PixelFormat,
    /// Raw pixel data (RGBA format, tightly packed)
    data: Vec<u8>,
    /// Timestamp in seconds
    timestamp: f64,
}

/// Supported pixel formats
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PixelFormat {
    /// RGB (3 bytes per pixel)
    RGB,
    /// RGBA (4 bytes per pixel)
    RGBA,
    /// YUV 4:2:0 planar
    YUV420P,
}

impl VideoFrame {
    /// Create a VideoFrame from an FFmpeg video frame
    ///
    /// This converts the frame to RGBA format for consistency and ease of use.
    /// The conversion is done internally and the resulting data is owned.
    ///
    /// # Arguments
    /// * `frame` - FFmpeg video frame to convert
    /// * `timestamp` - Timestamp in seconds for this frame
    pub(crate) fn from_ffmpeg(frame: &ffmpeg::frame::Video, timestamp: f64) -> Result<Self> {
        let width = frame.width();
        let height = frame.height();

        // Convert to RGBA for consistency
        let data = Self::convert_to_rgba(frame)?;

        Ok(VideoFrame {
            width,
            height,
            format: PixelFormat::RGBA,
            data,
            timestamp,
        })
    }

    /// Convert an FFmpeg frame to RGBA format
    fn convert_to_rgba(frame: &ffmpeg::frame::Video) -> Result<Vec<u8>> {
        use ffmpeg::format::Pixel;
        use ffmpeg::software::scaling::{context::Context, flag::Flags};

        let width = frame.width();
        let height = frame.height();

        // Validate frame dimensions
        if width == 0 || height == 0 {
            anyhow::bail!("Invalid frame dimensions: {}x{}", width, height);
        }

        // Validate frame format
        if frame.format() == Pixel::None {
            anyhow::bail!("Frame has no pixel format");
        }

        // Create a scaler to convert to RGBA
        let mut scaler = Context::get(
            frame.format(),
            width,
            height,
            Pixel::RGBA,
            width,
            height,
            Flags::BILINEAR,
        )?;

        let mut rgb_frame = ffmpeg::frame::Video::empty();
        scaler.run(frame, &mut rgb_frame)?;

        // Copy data into owned Vec (handle stride properly)
        let stride = rgb_frame.stride(0);
        let bytes_per_row = (width as usize) * 4; // RGBA = 4 bytes per pixel
        let mut data = Vec::with_capacity(bytes_per_row * height as usize);

        let frame_data = rgb_frame.data(0);
        for y in 0..height {
            let row_start = (y as usize) * stride;
            let row_end = row_start + bytes_per_row;
            data.extend_from_slice(&frame_data[row_start..row_end]);
        }

        Ok(data)
    }

    /// Get frame dimensions (width, height)
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Get frame width in pixels
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get frame height in pixels
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Get pixel format
    pub fn format(&self) -> &PixelFormat {
        &self.format
    }

    /// Get pixel data as slice
    ///
    /// For RGBA format, this is 4 bytes per pixel, tightly packed row-major order.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Get timestamp in seconds
    pub fn timestamp(&self) -> f64 {
        self.timestamp
    }

    /// Convert frame data to base64 encoding
    ///
    /// Useful for sending to GUI/web interfaces
    pub fn to_base64(&self) -> String {
        use base64::{Engine as _, engine::general_purpose};
        general_purpose::STANDARD.encode(&self.data)
    }

    /// Convert to a serializable format for GUI transfer
    pub fn to_serializable(&self) -> SerializableVideoFrame {
        SerializableVideoFrame {
            width: self.width,
            height: self.height,
            data: self.to_base64(),
            timestamp: self.timestamp,
        }
    }

    /// Get the size of the frame data in bytes
    pub fn data_size(&self) -> usize {
        self.data.len()
    }
}

/// Serializable version of VideoFrame for GUI transfer
///
/// This version uses base64 encoding for the pixel data to enable
/// JSON serialization via Tauri commands.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SerializableVideoFrame {
    /// Frame width in pixels
    pub width: u32,
    /// Frame height in pixels
    pub height: u32,
    /// Base64-encoded RGBA pixel data
    pub data: String,
    /// Timestamp in seconds
    pub timestamp: f64,
}

impl SerializableVideoFrame {
    /// Get frame dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
