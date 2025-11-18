//! MediaFile - represents an opened media file with its decoders

use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use std::path::PathBuf;

use super::audio_decoder::AudioDecoder;
use super::media_id::MediaId;
use super::video_decoder::VideoDecoder;

/// MediaFile owns the format context and decoders for a media file
pub struct MediaFile {
    /// Unique identifier for this media file
    id: MediaId,
    /// Path to the media file
    path: PathBuf,
    /// FFmpeg format context (the opened file handle)
    format_context: ffmpeg::format::context::Input,
    /// Video decoder (if video stream exists)
    video_decoder: Option<VideoDecoder>,
    /// Audio decoder (if audio stream exists)
    audio_decoder: Option<AudioDecoder>,
}

impl MediaFile {
    /// Open a media file and create decoders eagerly
    ///
    /// # Arguments
    /// * `path` - Path to the media file
    /// * `id` - MediaId to assign to this file
    ///
    /// Opens the file, probes for streams, and creates decoders for any
    /// video or audio streams found. All metadata is extracted upfront,
    /// including scanning frame metadata for efficient seeking.
    pub fn open(path: PathBuf, id: MediaId) -> Result<Self> {
        // Open the format context
        let mut format_context = ffmpeg::format::input(&path)
            .with_context(|| format!("Failed to open media file: {:?}", path))?;

        // Try to create video decoder and scan metadata
        let mut video_decoder = VideoDecoder::new(&format_context).ok();
        if let Some(ref mut decoder) = video_decoder {
            // Scan entire video to build metadata cache for efficient seeking
            decoder
                .scan_metadata(&mut format_context)
                .with_context(|| format!("Failed to scan video metadata: {:?}", path))?;
        }

        // Try to create audio decoder
        let audio_decoder = AudioDecoder::new(&format_context).ok();

        // At least one decoder should be present
        if video_decoder.is_none() && audio_decoder.is_none() {
            anyhow::bail!("No video or audio streams found in file: {:?}", path);
        }

        Ok(MediaFile {
            id,
            path,
            format_context,
            video_decoder,
            audio_decoder,
        })
    }

    /// Decode a single video frame at a specific timestamp
    ///
    /// # Arguments
    /// * `timestamp` - Time in seconds
    ///
    /// # Returns
    /// Decoded VideoFrame object (already in RGBA format)
    ///
    /// This method checks the LRU cache first. If the frame is not cached,
    /// it decodes the entire GOP containing the frame and stores it in the cache.
    pub fn decode_frame(&mut self, timestamp: f64) -> Result<super::video_frame::VideoFrame> {
        let video_decoder = self
            .video_decoder
            .as_mut()
            .context("No video decoder available")?;

        // Get frame metadata for timestamp
        let frame_metadata = video_decoder
            .metadata_cache()
            .get_frame_by_timestamp(timestamp)
            .context("No frame found at specified timestamp")?;

        let gop_id = frame_metadata.gop_id;
        let frame_number = frame_metadata.frame_number;

        // Check LRU cache first
        if let Some(cached_frame) = video_decoder.get_cached_frame(gop_id, frame_number) {
            return Ok(cached_frame);
        }

        // Frame not in cache, decode the entire GOP
        video_decoder
            .decode_gop(&mut self.format_context, gop_id)
            .with_context(|| {
                format!(
                    "Failed to decode GOP {} for frame at timestamp {}",
                    gop_id, timestamp
                )
            })?;

        // After GOP decoding, retrieve the frame from cache
        video_decoder
            .get_cached_frame(gop_id, frame_number)
            .context("Frame not found in cache after GOP decoding")
    }

    /// Decode video frames between start and end timestamps
    ///
    /// # Arguments
    /// * `start_time` - Start time in seconds
    /// * `end_time` - End time in seconds
    ///
    /// # Returns
    /// Vector of decoded VideoFrame objects (already in RGBA format)
    ///
    /// This method uses decode_frame internally for each frame in the range,
    /// which ensures proper cache utilization.
    pub fn decode_frames(
        &mut self,
        start_time: f64,
        end_time: f64,
    ) -> Result<Vec<super::video_frame::VideoFrame>> {
        // Get all frame timestamps in the requested range (immutable borrow)
        let frame_metadata_list: Vec<_> = {
            let video_decoder = self
                .video_decoder
                .as_ref()
                .context("No video decoder available")?;

            video_decoder
                .metadata_cache()
                .get_frames_in_range(start_time, end_time)
                .into_iter()
                .map(|f| f.timestamp)
                .collect()
        };

        if frame_metadata_list.is_empty() {
            return Ok(Vec::new());
        }

        // Decode each frame using decode_frame (which handles caching)
        let mut frames = Vec::new();
        for timestamp in frame_metadata_list {
            match self.decode_frame(timestamp) {
                Ok(frame) => frames.push(frame),
                Err(e) => {
                    // Log error but continue with other frames
                    eprintln!("Failed to decode frame at timestamp {}: {}", timestamp, e);
                }
            }
        }

        // Sort frames by timestamp to ensure consistent ordering
        frames.sort_by(|a, b| a.timestamp().partial_cmp(&b.timestamp()).unwrap());

        Ok(frames)
    }

    /// Decode audio samples between start and end timestamps
    ///
    /// # Arguments
    /// * `start_time` - Start time in seconds
    /// * `end_time` - End time in seconds
    ///
    /// # Returns
    /// Audio frame containing decoded samples
    pub fn decode_samples(
        &mut self,
        start_time: f64,
        end_time: f64,
    ) -> Result<ffmpeg::frame::Audio> {
        let audio_decoder = self
            .audio_decoder
            .as_mut()
            .context("No audio decoder available")?;

        audio_decoder.decode_samples(&mut self.format_context, start_time, end_time)
    }

    /// Get the MediaId for this file
    pub fn id(&self) -> MediaId {
        self.id
    }

    /// Get the path to this file
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Get the duration of the media file in seconds
    pub fn duration(&self) -> f64 {
        // Use video duration if available, otherwise audio duration
        if let Some(ref video_decoder) = self.video_decoder {
            video_decoder.duration()
        } else if let Some(ref audio_decoder) = self.audio_decoder {
            audio_decoder.duration()
        } else {
            0.0
        }
    }

    /// Check if this file has a video stream
    pub fn has_video(&self) -> bool {
        self.video_decoder.is_some()
    }

    /// Check if this file has an audio stream
    pub fn has_audio(&self) -> bool {
        self.audio_decoder.is_some()
    }

    /// Get video metadata (width, height, fps) if video stream exists
    pub fn video_info(&self) -> Option<(u32, u32, f64)> {
        self.video_decoder
            .as_ref()
            .map(|decoder| (decoder.width(), decoder.height(), decoder.fps()))
    }

    /// Get audio metadata (sample_rate, channels) if audio stream exists
    pub fn audio_info(&self) -> Option<(u32, u16)> {
        self.audio_decoder
            .as_ref()
            .map(|decoder| (decoder.sample_rate(), decoder.channels()))
    }
}
