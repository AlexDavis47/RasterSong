//! Video decoding functionality using FFmpeg

use anyhow::{Context, Result};
use ffmpeg::format::context::Input;
use ffmpeg::media::Type;
use ffmpeg_next as ffmpeg;

/// Video decoder that owns the codec context for a video stream
pub struct VideoDecoder {
    /// Index of the video stream in the format context
    video_stream_index: usize,
    /// FFmpeg video decoder
    decoder: ffmpeg::decoder::Video,
    /// Cached video width
    width: u32,
    /// Cached video height
    height: u32,
    /// Frames per second
    fps: f64,
    /// Duration in seconds
    duration: f64,
}

impl VideoDecoder {
    /// Create a new VideoDecoder from a format context
    ///
    /// Finds the first video stream and creates a decoder for it.
    /// Extracts and caches metadata.
    pub fn new(format_ctx: &Input) -> Result<Self> {
        // Find the first video stream
        let video_stream = format_ctx
            .streams()
            .best(Type::Video)
            .context("No video stream found in file")?;

        let video_stream_index = video_stream.index();

        // Create decoder context from stream
        let context = ffmpeg::codec::context::Context::from_parameters(video_stream.parameters())
            .context("Failed to create codec context from parameters")?;

        let decoder = context
            .decoder()
            .video()
            .context("Failed to create video decoder")?;

        // Extract metadata from decoder
        let width = decoder.width();
        let height = decoder.height();

        // Calculate FPS from frame rate
        let frame_rate = video_stream.avg_frame_rate();
        let fps = f64::from(frame_rate);

        // Get duration
        let duration_value = video_stream.duration();
        let duration = if duration_value > 0 {
            duration_value as f64 * f64::from(video_stream.time_base())
        } else {
            let format_duration = format_ctx.duration();
            if format_duration > 0 {
                format_duration as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE)
            } else {
                0.0
            }
        };

        Ok(VideoDecoder {
            video_stream_index,
            decoder,
            width,
            height,
            fps,
            duration,
        })
    }

    /// Decode a frame at a specific timestamp
    ///
    /// # Arguments
    /// * `format_ctx` - The format context to read packets from
    /// * `timestamp` - Time in seconds to seek to and decode
    pub fn decode_frame(
        &mut self,
        format_ctx: &mut Input,
        timestamp: f64,
    ) -> Result<ffmpeg::frame::Video> {
        // Convert timestamp to stream time base
        let stream = format_ctx
            .stream(self.video_stream_index)
            .context("Video stream not found")?;

        let time_base = f64::from(stream.time_base());
        let seek_target = (timestamp / time_base) as i64;

        // Seek backwards from target to find nearest keyframe
        // Allow seeking up to 10 seconds backwards (typical GOP size is 1-2 seconds)
        // This ensures we find the keyframe without going all the way to the start
        let max_gop_size = (10.0 / time_base) as i64; // 10 seconds in time base units
        let seek_start = (seek_target - max_gop_size).max(0);

        // Seek to the timestamp (will seek to nearest keyframe before the target)
        format_ctx
            .seek(seek_target, seek_start..seek_target)
            .context("Failed to seek to timestamp")?;

        // Flush the decoder after seeking
        self.decoder.flush();

        // Read packets and decode frames until we get close to our target timestamp
        // We need to decode forward from the keyframe to reach the exact frame
        let mut best_frame = ffmpeg::frame::Video::empty();
        let mut found_frame = false;
        let frame_duration = 1.0 / self.fps;

        for (stream, packet) in format_ctx.packets() {
            if stream.index() == self.video_stream_index {
                // Send packet to decoder
                self.decoder.send_packet(&packet)?;

                // Try to receive decoded frames
                let mut decoded_frame = ffmpeg::frame::Video::empty();
                while self.decoder.receive_frame(&mut decoded_frame).is_ok() {
                    // Get the presentation timestamp of the decoded frame
                    let pts = decoded_frame.pts().unwrap_or(0);
                    let frame_time = pts as f64 * time_base;

                    // If this frame is at or after our target timestamp, use it
                    if frame_time >= timestamp - frame_duration * 0.5 {
                        return Ok(decoded_frame);
                    }

                    // Keep this frame as best candidate so far
                    best_frame = decoded_frame;
                    found_frame = true;
                    decoded_frame = ffmpeg::frame::Video::empty();
                }
            }
        }

        // If we found any frame, return the best one
        if found_frame {
            return Ok(best_frame);
        }

        // If we didn't get a frame, try to flush the decoder
        self.decoder.send_eof()?;
        self.decoder
            .receive_frame(&mut best_frame)
            .context("Failed to decode frame at specified timestamp")?;

        Ok(best_frame)
    }

    /// Get the video width in pixels
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get the video height in pixels
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Get the frames per second
    pub fn fps(&self) -> f64 {
        self.fps
    }

    /// Get the duration in seconds
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// Get the video stream index
    pub fn video_stream_index(&self) -> usize {
        self.video_stream_index
    }

    /// Flush the decoder (internal use)
    pub(crate) fn flush(&mut self) {
        self.decoder.flush();
    }

    /// Send a packet to the decoder (internal use)
    pub(crate) fn send_packet(&mut self, packet: &ffmpeg::packet::Packet) -> Result<()> {
        self.decoder.send_packet(packet)?;
        Ok(())
    }

    /// Receive a decoded frame from the decoder (internal use)
    pub(crate) fn receive_frame(&mut self, frame: &mut ffmpeg::frame::Video) -> Result<()> {
        self.decoder.receive_frame(frame)?;
        Ok(())
    }
}
