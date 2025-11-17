//! Audio decoding functionality using FFmpeg

use anyhow::{Context, Result};
use ffmpeg::format::context::Input;
use ffmpeg::media::Type;
use ffmpeg_next as ffmpeg;

/// Audio decoder that owns the codec context for an audio stream
pub struct AudioDecoder {
    /// Index of the audio stream in the format context
    audio_stream_index: usize,
    /// FFmpeg audio decoder
    decoder: ffmpeg::decoder::Audio,
    /// Sample rate in Hz
    sample_rate: u32,
    /// Number of channels
    channels: u16,
    /// Duration in seconds
    duration: f64,
}

impl AudioDecoder {
    /// Create a new AudioDecoder from a format context
    ///
    /// Finds the first audio stream and creates a decoder for it.
    /// Extracts and caches metadata.
    pub fn new(format_ctx: &Input) -> Result<Self> {
        // Find the first audio stream
        let audio_stream = format_ctx
            .streams()
            .best(Type::Audio)
            .context("No audio stream found in file")?;

        let audio_stream_index = audio_stream.index();

        // Create decoder context from stream
        let context = ffmpeg::codec::context::Context::from_parameters(audio_stream.parameters())
            .context("Failed to create codec context from parameters")?;

        let decoder = context
            .decoder()
            .audio()
            .context("Failed to create audio decoder")?;

        // Extract metadata
        let sample_rate = decoder.rate();
        let channels = decoder.channels();

        // Get duration - duration() returns i64 directly, not Option
        let duration_value = audio_stream.duration();
        let duration = if duration_value > 0 {
            duration_value as f64 * f64::from(audio_stream.time_base())
        } else {
            let format_duration = format_ctx.duration();
            if format_duration > 0 {
                format_duration as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE)
            } else {
                0.0
            }
        };

        Ok(AudioDecoder {
            audio_stream_index,
            decoder,
            sample_rate,
            channels,
            duration,
        })
    }

    /// Decode audio samples between start and end timestamps
    ///
    /// # Arguments
    /// * `format_ctx` - The format context to read packets from
    /// * `start_time` - Start time in seconds
    /// * `end_time` - End time in seconds
    ///
    /// # Returns
    /// Audio frame containing decoded samples
    pub fn decode_samples(
        &mut self,
        _format_ctx: &mut Input,
        _start_time: f64,
        _end_time: f64,
    ) -> Result<ffmpeg::frame::Audio> {
        // TODO: Implement audio sample decoding
        // For now, return an empty frame as a stub
        Ok(ffmpeg::frame::Audio::empty())
    }

    /// Get the sample rate in Hz
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get the number of audio channels
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Get the duration in seconds
    pub fn duration(&self) -> f64 {
        self.duration
    }
}
