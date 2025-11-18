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

    /// Decode video frames between start and end timestamps
    ///
    /// # Arguments
    /// * `start_time` - Start time in seconds
    /// * `end_time` - End time in seconds
    ///
    /// # Returns
    /// Vector of decoded VideoFrame objects (already in RGBA format)
    pub fn decode_frames(
        &mut self,
        start_time: f64,
        end_time: f64,
    ) -> Result<Vec<super::video_frame::VideoFrame>> {
        use std::collections::HashSet;

        let video_decoder = self
            .video_decoder
            .as_mut()
            .context("No video decoder available")?;

        // Get GOPs needed for this time range
        let gop_ids: HashSet<usize> = {
            let metadata_cache = video_decoder.metadata_cache();
            metadata_cache
                .get_frames_in_range(start_time, end_time)
                .into_iter()
                .map(|f| f.gop_id)
                .collect()
        };

        if gop_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Decode all needed GOPs (they will be cached automatically)
        let mut all_frames = Vec::new();
        for gop_id in gop_ids {
            let gop_frames = video_decoder.decode_gop(&mut self.format_context, gop_id)?;
            all_frames.extend(gop_frames);
        }

        // Filter frames to the requested time range and sort by timestamp
        let mut result: Vec<_> = all_frames
            .into_iter()
            .filter(|f| f.timestamp() >= start_time && f.timestamp() < end_time)
            .collect();

        result.sort_by(|a, b| a.timestamp().partial_cmp(&b.timestamp()).unwrap());

        Ok(result)
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
