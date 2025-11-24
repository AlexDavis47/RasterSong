//! Video decoding functionality using FFmpeg

use anyhow::{Context, Result};
use ffmpeg::format::context::Input;
use ffmpeg::media::Type;
use ffmpeg_next as ffmpeg;

use super::frame_cache::FrameCache;
use super::frame_metadata::{FrameMetadata, FrameMetadataCache};
use super::video_frame::VideoFrame;
use std::sync::Arc;

/// Extension trait for FFmpeg Rational to convert to f64
trait TimeBaseExt {
    fn as_f64(&self) -> f64;
}

impl TimeBaseExt for ffmpeg::Rational {
    fn as_f64(&self) -> f64 {
        // FFmpeg Rational is a tuple struct (numerator, denominator)
        self.0 as f64 / self.1 as f64
    }
}

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
    /// Frame metadata cache for efficient seeking
    metadata_cache: FrameMetadataCache,
    /// Time base as f64 (cached for performance)
    time_base: f64,
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

        // Get time_base and cache it
        let time_base = video_stream.time_base().as_f64();

        // Get duration
        let duration_value = video_stream.duration();
        let duration = if duration_value > 0 {
            duration_value as f64 * time_base
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
            metadata_cache: FrameMetadataCache::new(),
            time_base,
        })
    }

    /// Scan the entire video file to build metadata cache
    ///
    /// This should be called once after creating the decoder to enable
    /// efficient seeking and GOP-based decoding.
    pub fn scan_metadata(&mut self, format_ctx: &mut Input) -> Result<()> {
        // Seek to beginning
        format_ctx.seek(0, ..0)?;
        self.decoder.flush();

        let mut frame_number = 0;
        let mut current_gop_id = 0;

        // Iterate through all packets and decode to extract metadata
        for (stream, packet) in format_ctx.packets() {
            if stream.index() == self.video_stream_index {
                // Send packet to decoder
                self.decoder.send_packet(&packet)?;

                // Try to receive decoded frames
                let mut decoded_frame = ffmpeg::frame::Video::empty();
                while self.decoder.receive_frame(&mut decoded_frame).is_ok() {
                    // Use pts for timestamp
                    let pts = decoded_frame.pts().unwrap_or(0);
                    let timestamp = pts as f64 * self.time_base;

                    // Check if this is a keyframe
                    let is_keyframe = decoded_frame.is_key();

                    // Update GOP ID when we hit a new keyframe
                    if is_keyframe && frame_number > 0 {
                        current_gop_id += 1;
                    }

                    // Get file offset if available (from packet position)
                    let file_offset = if packet.position() >= 0 {
                        Some(packet.position() as i64)
                    } else {
                        None
                    };

                    // Add frame metadata to cache
                    self.metadata_cache.add_frame(FrameMetadata {
                        frame_number,
                        pts,
                        timestamp,
                        is_keyframe,
                        gop_id: current_gop_id,
                        file_offset,
                    });

                    frame_number += 1;
                    decoded_frame = ffmpeg::frame::Video::empty();
                }
            }
        }

        // Flush decoder to get any remaining frames
        self.decoder.send_eof()?;
        let mut decoded_frame = ffmpeg::frame::Video::empty();
        while self.decoder.receive_frame(&mut decoded_frame).is_ok() {
            let pts = decoded_frame.pts().unwrap_or(0);
            let timestamp = pts as f64 * self.time_base;
            let is_keyframe = decoded_frame.is_key();

            if is_keyframe && frame_number > 0 {
                current_gop_id += 1;
            }

            self.metadata_cache.add_frame(FrameMetadata {
                frame_number,
                pts,
                timestamp,
                is_keyframe,
                gop_id: current_gop_id,
                file_offset: None,
            });

            frame_number += 1;
            decoded_frame = ffmpeg::frame::Video::empty();
        }

        // Reset decoder state after scanning
        self.decoder.flush();

        Ok(())
    }

    /// Get the metadata cache
    pub fn metadata_cache(&self) -> &FrameMetadataCache {
        &self.metadata_cache
    }


    /// Seek to a specific PTS using the video stream's time base (much more precise than format_ctx.seek)
    fn seek_to_stream_pts(&self, format_ctx: &mut Input, pts: i64) -> Result<()> {
        unsafe {
            use ffmpeg_next::ffi::avformat_seek_file;

            // Using the correct stream index with its time_base is MUCH more accurate
            // than using stream_index=-1 which uses AV_TIME_BASE
            let result = avformat_seek_file(
                format_ctx.as_mut_ptr(),
                self.video_stream_index as i32, // Use video stream index for correct time_base
                i64::MIN,                       // min_ts - allow seeking backward
                pts,                            // target pts in stream time_base units
                pts,                            // max_ts - tight range for precision
                0,                              // flags (could add AVSEEK_FLAG_BACKWARD = 1)
            );

            if result >= 0 {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "Stream PTS seek failed with error code: {}",
                    result
                ))
            }
        }
    }

    /// Try byte-based seeking (not supported by all containers, e.g., MP4)
    fn seek_to_byte_offset(&self, format_ctx: &mut Input, offset: i64) -> Result<()> {
        unsafe {
            use ffmpeg_next::ffi::avformat_seek_file;

            const AVSEEK_FLAG_BYTE: i32 = 2;

            let result = avformat_seek_file(
                format_ctx.as_mut_ptr(),
                -1,               // stream_index (ignored for byte seeking)
                i64::MIN,         // min_ts
                offset,           // target byte offset
                i64::MAX,         // max_ts
                AVSEEK_FLAG_BYTE, // flags - byte-based seeking
            );

            if result >= 0 {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "Byte seek failed with error code: {}",
                    result
                ))
            }
        }
    }

    /// Decode an entire GOP
    ///
    /// Decodes all frames in the specified GOP, converts them to VideoFrame,
    /// and stores them in the shared cache.
    /// Uses the metadata cache to find the keyframe position.
    ///
    /// # Arguments
    /// * `format_ctx` - The format context to read packets from
    /// * `gop_id` - The GOP ID to decode
    /// * `cache` - Shared frame cache to store decoded frames
    ///
    /// # Returns
    /// Vec of VideoFrame for all frames in the GOP
    pub fn decode_gop(
        &mut self,
        format_ctx: &mut Input,
        gop_id: usize,
        cache: &Arc<FrameCache>,
    ) -> Result<Vec<VideoFrame>> {
        use std::collections::HashMap;

        println!("[VideoDecoder::decode_gop] Starting decode for GOP {}", gop_id);
        
        // Check cache first
        if cache.contains_gop(gop_id) {
            println!("[VideoDecoder::decode_gop] GOP {} found in cache", gop_id);
            // GOP is already cached, return frames
            return cache
                .get_gop(gop_id)
                .context("GOP not in cache despite contains_gop being true");
        }

        println!("[VideoDecoder::decode_gop] GOP {} not in cache, decoding...", gop_id);
        
        // Get frames in this GOP from metadata
        let gop_frames = self
            .metadata_cache
            .get_frames_in_gop(gop_id)
            .context("GOP not found in metadata cache")?;

        if gop_frames.is_empty() {
            anyhow::bail!("GOP {} has no frames in metadata cache", gop_id);
        }
        
        println!("[VideoDecoder::decode_gop] GOP {} has {} frames", gop_id, gop_frames.len());

        // Find the keyframe (first frame in GOP)
        let keyframe = gop_frames
            .iter()
            .find(|f| f.is_keyframe)
            .context("No keyframe found in GOP")?;

        let keyframe_pts = keyframe.pts;
        println!("[VideoDecoder::decode_gop] Seeking to keyframe at PTS: {}", keyframe_pts);

        // Try byte-based seeking first if we have the offset (fastest, but not supported by all containers)
        let seek_successful = if let Some(byte_offset) = keyframe.file_offset {
            self.seek_to_byte_offset(format_ctx, byte_offset).is_ok() // TODO: This is failing on mp4, should be functional
        } else {
            false
        };
        println!("[VideoDecoder::decode_gop] Byte seek successful?: {}", seek_successful);

        // If byte seek failed, use stream-specific PTS seeking (much more accurate than format_ctx.seek)
        if !seek_successful {
            println!("[VideoDecoder::decode_gop] Using PTS seek");
            self.seek_to_stream_pts(format_ctx, keyframe_pts)
                .context("Failed to seek to keyframe")?;
        }

        self.decoder.flush();
        println!("[VideoDecoder::decode_gop] Decoder flushed, starting packet decoding");

        // Decode all frames in this GOP
        let mut decoded_video_frames: HashMap<usize, VideoFrame> = HashMap::new();

        for (stream, packet) in format_ctx.packets() {
            if stream.index() == self.video_stream_index {
                self.decoder.send_packet(&packet)?;

                let mut frame = ffmpeg::frame::Video::empty();
                while self.decoder.receive_frame(&mut frame).is_ok() {
                    let pts = frame.pts().unwrap_or(0);
                    let timestamp = pts as f64 * self.time_base;

                    // Find corresponding frame number in metadata
                    if let Some(metadata) = self.metadata_cache.get_frame_by_timestamp(timestamp) {
                        if metadata.gop_id == gop_id {
                            // Convert to VideoFrame immediately
                            let video_frame = VideoFrame::from_ffmpeg(&frame, timestamp)?;
                            decoded_video_frames.insert(metadata.frame_number, video_frame);

                            // If we've decoded all frames in this GOP, break
                            if decoded_video_frames.len() >= gop_frames.len() {
                                break;
                            }
                        } else if metadata.gop_id > gop_id {
                            // We've moved past this GOP, break
                            break;
                        }
                    }

                    frame = ffmpeg::frame::Video::empty();
                }

                if decoded_video_frames.len() >= gop_frames.len() {
                    break;
                }
            }
        }

        println!("[VideoDecoder::decode_gop] Decoded {} frames for GOP {}", decoded_video_frames.len(), gop_id);
        
        // Store in cache
        println!("[VideoDecoder::decode_gop] Storing GOP {} in cache", gop_id);
        cache.store_gop(gop_id, decoded_video_frames.clone());

        // Return as Vec sorted by frame number
        let mut result: Vec<_> = decoded_video_frames.into_iter().collect();
        result.sort_by_key(|(frame_num, _)| *frame_num);
        println!("[VideoDecoder::decode_gop] GOP {} decode complete", gop_id);
        Ok(result.into_iter().map(|(_, frame)| frame).collect())
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
}
