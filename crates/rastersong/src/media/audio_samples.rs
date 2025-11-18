//! Audio samples wrapper for decoded audio data

use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use serde::{Deserialize, Serialize};

/// Decoded audio samples in a convenient format
#[derive(Clone, Debug)]
pub struct AudioSamples {
    /// Sample rate in Hz
    sample_rate: u32,
    /// Number of channels
    channels: u16,
    /// Audio sample format
    format: AudioFormat,
    /// Interleaved audio data (f32 samples)
    data: Vec<f32>,
    /// Start timestamp in seconds
    start_time: f64,
    /// End timestamp in seconds
    end_time: f64,
}

/// Supported audio formats
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AudioFormat {
    /// 32-bit floating point (standard internal format)
    F32,
    /// 16-bit signed integer
    I16,
}

impl AudioSamples {
    /// Create AudioSamples from an FFmpeg audio frame
    ///
    /// Converts to f32 interleaved format for consistency
    ///
    /// # Arguments
    /// * `frame` - FFmpeg audio frame to convert
    /// * `start_time` - Start timestamp in seconds
    /// * `end_time` - End timestamp in seconds
    pub(crate) fn from_ffmpeg(
        frame: &ffmpeg::frame::Audio,
        start_time: f64,
        end_time: f64,
    ) -> Result<Self> {
        let sample_rate = frame.rate();
        let channels = frame.channels();

        // Convert to f32 interleaved format
        let data = Self::convert_to_f32_interleaved(frame)?;

        Ok(AudioSamples {
            sample_rate,
            channels,
            format: AudioFormat::F32,
            data,
            start_time,
            end_time,
        })
    }

    /// Convert FFmpeg audio frame to f32 interleaved format
    fn convert_to_f32_interleaved(frame: &ffmpeg::frame::Audio) -> Result<Vec<f32>> {
        let channels = frame.channels() as usize;
        let samples = frame.samples();
        let format = frame.format();

        let mut data = Vec::with_capacity(samples * channels);

        // Check if the audio is planar or packed
        let is_planar = format.is_planar();

        if is_planar {
            // Planar: each channel has its own buffer
            // Convert to interleaved
            for i in 0..samples {
                for ch in 0..channels {
                    let sample = Self::extract_sample_f32(frame, ch, i)?;
                    data.push(sample);
                }
            }
        } else {
            // Packed: samples are already interleaved
            for i in 0..(samples * channels) {
                let sample = Self::extract_sample_f32(frame, 0, i)?;
                data.push(sample);
            }
        }

        Ok(data)
    }

    /// Extract a single sample as f32 from an FFmpeg frame
    fn extract_sample_f32(
        frame: &ffmpeg::frame::Audio,
        channel: usize,
        index: usize,
    ) -> Result<f32> {
        let format = frame.format();
        let plane_data = frame.data(channel);

        // Handle different sample formats
        match format {
            ffmpeg::format::Sample::F32(_) => {
                let offset = index * 4;
                let bytes = plane_data
                    .get(offset..offset + 4)
                    .context("Sample index out of bounds")?;
                Ok(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            }
            ffmpeg::format::Sample::I16(_) => {
                let offset = index * 2;
                let bytes = plane_data
                    .get(offset..offset + 2)
                    .context("Sample index out of bounds")?;
                let sample_i16 = i16::from_le_bytes([bytes[0], bytes[1]]);
                // Convert i16 to f32 [-1.0, 1.0]
                Ok(sample_i16 as f32 / 32768.0)
            }
            ffmpeg::format::Sample::I32(_) => {
                let offset = index * 4;
                let bytes = plane_data
                    .get(offset..offset + 4)
                    .context("Sample index out of bounds")?;
                let sample_i32 = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                // Convert i32 to f32 [-1.0, 1.0]
                Ok(sample_i32 as f32 / 2147483648.0)
            }
            _ => anyhow::bail!("Unsupported audio format: {:?}", format),
        }
    }

    /// Get sample rate in Hz
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get number of channels
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Get audio format
    pub fn format(&self) -> &AudioFormat {
        &self.format
    }

    /// Get audio data as slice (interleaved f32 samples)
    ///
    /// For stereo audio, samples are ordered as: [L, R, L, R, ...]
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    /// Get time range (start, end) in seconds
    pub fn time_range(&self) -> (f64, f64) {
        (self.start_time, self.end_time)
    }

    /// Get start time in seconds
    pub fn start_time(&self) -> f64 {
        self.start_time
    }

    /// Get end time in seconds
    pub fn end_time(&self) -> f64 {
        self.end_time
    }

    /// Get duration in seconds
    pub fn duration(&self) -> f64 {
        self.end_time - self.start_time
    }

    /// Get total number of samples (per channel)
    pub fn sample_count(&self) -> usize {
        self.data.len() / self.channels as usize
    }

    /// Get data for a specific channel
    ///
    /// # Arguments
    /// * `channel` - Channel index (0 = left, 1 = right for stereo)
    ///
    /// Returns a Vec with samples for that channel only
    pub fn channel_data(&self, channel: usize) -> Result<Vec<f32>> {
        if channel >= self.channels as usize {
            anyhow::bail!(
                "Channel {} out of range (0-{})",
                channel,
                self.channels - 1
            );
        }

        let mut channel_samples = Vec::with_capacity(self.sample_count());
        for i in 0..self.sample_count() {
            let index = i * self.channels as usize + channel;
            channel_samples.push(self.data[index]);
        }

        Ok(channel_samples)
    }

    /// Get size of audio data in bytes
    pub fn data_size(&self) -> usize {
        self.data.len() * std::mem::size_of::<f32>()
    }
}

