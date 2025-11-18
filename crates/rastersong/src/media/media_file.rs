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
    /// video or audio streams found. All metadata is extracted upfront.
    pub fn open(path: PathBuf, id: MediaId) -> Result<Self> {
        // Open the format context
        let format_context = ffmpeg::format::input(&path)
            .with_context(|| format!("Failed to open media file: {:?}", path))?;

        // Try to create video decoder
        let video_decoder = VideoDecoder::new(&format_context).ok();

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
    /// Vector of decoded video frames
    pub fn decode_frames(
        &mut self,
        start_time: f64,
        end_time: f64,
    ) -> Result<Vec<ffmpeg::frame::Video>> {
        let video_decoder = self
            .video_decoder
            .as_mut()
            .context("No video decoder available")?;

        let fps = video_decoder.fps();
        let frame_duration = 1.0 / fps;

        // OPTIMIZATION: Seek ONCE at the start, then decode sequentially
        // This avoids multiple seeks which would each start from the beginning
        let stream = self
            .format_context
            .stream(video_decoder.video_stream_index())
            .context("Video stream not found")?;

        let time_base = f64::from(stream.time_base());
        let seek_target = (start_time / time_base) as i64;

        // Seek backwards from start_time to find nearest keyframe
        let max_gop_size = (10.0 / time_base) as i64; // 10 seconds in time base units
        let seek_start = (seek_target - max_gop_size).max(0);

        self.format_context
            .seek(seek_target, seek_start..seek_target)
            .context("Failed to seek to start time")?;

        video_decoder.flush();

        let mut frames = Vec::new();
        let mut current_time = start_time;

        // Decode frames sequentially without re-seeking
        while current_time < end_time {
            // Read packets and decode until we get the frame at current_time
            let mut decoded_frame = None;

            for (stream, packet) in self.format_context.packets() {
                if stream.index() == video_decoder.video_stream_index() {
                    video_decoder.send_packet(&packet)?;

                    let mut frame = ffmpeg::frame::Video::empty();
                    while video_decoder.receive_frame(&mut frame).is_ok() {
                        let pts = frame.pts().unwrap_or(0);
                        let frame_time = pts as f64 * time_base;

                        // If this frame is at or after our target time, use it
                        if frame_time >= current_time - frame_duration * 0.5 {
                            decoded_frame = Some(frame);
                            break;
                        }
                    }

                    if decoded_frame.is_some() {
                        break;
                    }
                }
            }

            if let Some(frame) = decoded_frame {
                frames.push(frame);
            } else {
                // If we didn't get a frame, we've probably reached the end
                break;
            }

            current_time += frame_duration;
        }

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
