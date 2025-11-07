/// Video Signal Processor
///
/// Manages the complete workflow: decode video → encode to audio signal → apply effects → decode back to video
use crate::audio_effects;
use crate::video_signal::{MappingMode, VideoSignalCodec, VideoSignalMetadata};
use anyhow::Result;

/// Interpolation mode for modulator resampling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpolationMode {
    /// Linear interpolation (fast, good quality)
    Linear,
    /// Sinc interpolation with Lanczos window (slow, highest quality - DAW standard)
    Sinc,
}

/// Configuration for audio modulation effects
#[derive(Debug, Clone)]
pub struct EffectSettings {
    pub am_enabled: bool,
    pub am_depth: f32,

    pub ring_mod_enabled: bool,
    pub ring_mod_mix: f32,

    pub fm_enabled: bool,
    pub fm_depth: f32,

    pub distortion_enabled: bool,
    pub distortion_amount: f32,

    pub bit_crush_enabled: bool,
    pub bit_crush_bits: u8,

    pub lpf_enabled: bool,
    pub lpf_cutoff: f32, // Cutoff frequency in Hz

    pub interpolation: InterpolationMode,
}

impl Default for EffectSettings {
    fn default() -> Self {
        Self {
            am_enabled: false,
            am_depth: 0.5,
            ring_mod_enabled: false,
            ring_mod_mix: 0.5,
            fm_enabled: false,
            fm_depth: 10.0,
            distortion_enabled: false,
            distortion_amount: 0.5,
            bit_crush_enabled: false,
            bit_crush_bits: 4,
            lpf_enabled: false,
            lpf_cutoff: 5000.0,                     // Default 5kHz cutoff
            interpolation: InterpolationMode::Sinc, // Default to highest quality
        }
    }
}

/// Processes video frames with audio modulation effects
pub struct VideoSignalProcessor {
    codec: VideoSignalCodec,
    modulator_audio: Option<Vec<f32>>, // Full precision float modulator (-1.0 to +1.0)
}

impl VideoSignalProcessor {
    /// Create a new processor with video metadata
    pub fn new(width: u32, height: u32, fps: f32, mapping_mode: MappingMode) -> Self {
        let metadata = VideoSignalMetadata::new(width, height, fps, mapping_mode);
        let codec = VideoSignalCodec::new(metadata);

        // Print sync information
        let effective_rate = codec.metadata().effective_sample_rate();
        let speed_mult = codec.metadata().playback_speed_multiplier();
        println!("Video signal effective rate: {:.0} Hz", effective_rate);
        println!("Modulator playback speed multiplier: {:.2}x", speed_mult);
        println!(
            "(Modulator will be stretched {:.2}x to sync with video signal)",
            speed_mult
        );

        Self {
            codec,
            modulator_audio: None,
        }
    }

    /// Load modulator audio (normalized float, mono, 44.1kHz)
    ///
    /// # Arguments
    /// * `audio_data` - Normalized float samples (-1.0 to +1.0, mono, 44.1kHz)
    pub fn load_modulator(&mut self, audio_data: Vec<f32>) {
        self.modulator_audio = Some(audio_data);
    }

    /// Clear modulator audio
    pub fn clear_modulator(&mut self) {
        self.modulator_audio = None;
    }

    /// Process a single RGBA frame with effects at a specific time position
    ///
    /// # Arguments
    /// * `frame_rgba` - The RGBA frame data
    /// * `effects` - Effect settings to apply
    /// * `time_seconds` - Current time position in the video (for syncing modulator)
    pub fn process_frame_at_time(
        &self,
        frame_rgba: &[u8],
        effects: &EffectSettings,
        time_seconds: f32,
    ) -> Result<Vec<u8>> {
        // 1. Encode frame to audio signal
        let mut audio_signal = self.codec.encode_frame(frame_rgba)?;

        // 2. Apply effects with modulator at the correct time position
        if let Some(ref modulator) = self.modulator_audio {
            // Calculate which slice of the modulator to use based on time
            // (filter is applied inside get_modulator_slice at the correct sample rate)
            let modulator_slice =
                self.get_modulator_slice(modulator, &audio_signal, time_seconds, effects);
            audio_signal = self.apply_effects(&audio_signal, &modulator_slice, effects);
        }

        // 3. Decode back to RGBA frame
        self.codec.decode_frame(&audio_signal)
    }

    /// Process a single RGBA frame with effects (uses time 0.0)
    pub fn process_frame(&self, frame_rgba: &[u8], effects: &EffectSettings) -> Result<Vec<u8>> {
        self.process_frame_at_time(frame_rgba, effects, 0.0)
    }

    /// Get the appropriate slice of modulator audio for this time position
    /// Stretches/resamples the modulator to match the video signal's effective sample rate
    ///
    /// Returns normalized float samples (-1.0 to +1.0) matching the length of the video signal
    fn get_modulator_slice(
        &self,
        modulator: &[f32],
        audio_signal: &[u8],
        time_seconds: f32,
        effects: &EffectSettings,
    ) -> Vec<f32> {
        if modulator.is_empty() {
            return Vec::new();
        }

        let samples_needed = audio_signal.len();
        let speed_multiplier = self.metadata().playback_speed_multiplier();
        let sample_rate = self.metadata().sample_rate as f32;

        // Calculate how many samples we need from the ORIGINAL modulator
        // to cover the duration of this video frame
        let original_samples_needed = (samples_needed as f32 / speed_multiplier).ceil() as usize;
        let start_position = (time_seconds * sample_rate) as usize;

        // Extract a contiguous chunk from the original modulator
        // Pad with silence (0.0 = neutral) when past the end, instead of wrapping
        let mut original_chunk = Vec::with_capacity(original_samples_needed);
        for i in 0..original_samples_needed {
            let mod_idx = start_position + i;
            if mod_idx < modulator.len() {
                original_chunk.push(modulator[mod_idx]);
            } else {
                // Past the end of modulator - pad with silence (neutral value = 0.0)
                original_chunk.push(0.0);
            }
        }

        // Apply low pass filter to the contiguous chunk at its TRUE 44.1kHz rate
        // Note: low_pass_filter expects u8, so we need a f32 version or convert temporarily
        // For now, convert to u8 for filtering, then back to f32
        // TODO: Create a f32 version of low_pass_filter for better precision
        let filtered_chunk = if effects.lpf_enabled {
            // Convert to u8 for filtering (temporary, loses precision)
            let u8_chunk: Vec<u8> = original_chunk
                .iter()
                .map(|&s| ((s * 127.0) + 128.0).clamp(0.0, 255.0) as u8)
                .collect();
            let filtered_u8 =
                audio_effects::low_pass_filter(&u8_chunk, effects.lpf_cutoff, sample_rate);
            // Convert back to f32
            filtered_u8
                .iter()
                .map(|&s| ((s as f32 - 128.0) / 127.0).clamp(-1.0, 1.0))
                .collect()
        } else {
            original_chunk
        };

        // Now resample/stretch the filtered chunk to match video signal rate
        // Choice of interpolation method prevents the "staircase" effect
        match effects.interpolation {
            InterpolationMode::Linear => {
                // Linear interpolation (fast, good quality)
                let mut slice = Vec::with_capacity(samples_needed);
                for i in 0..samples_needed {
                    let chunk_position = i as f32 / speed_multiplier;
                    let chunk_idx = chunk_position.floor() as usize;
                    let frac = chunk_position - chunk_idx as f32;

                    if chunk_idx + 1 < filtered_chunk.len() {
                        let sample0 = filtered_chunk[chunk_idx];
                        let sample1 = filtered_chunk[chunk_idx + 1];
                        let interpolated = sample0 + (sample1 - sample0) * frac;
                        slice.push(interpolated.clamp(-1.0, 1.0));
                    } else if chunk_idx < filtered_chunk.len() {
                        slice.push(
                            filtered_chunk[chunk_idx.min(filtered_chunk.len() - 1)]
                                .clamp(-1.0, 1.0),
                        );
                    } else {
                        slice.push(0.0);
                    }
                }
                slice
            }
            InterpolationMode::Sinc => {
                // Lanczos sinc interpolation (slow, highest quality - DAW standard)
                audio_effects::sinc_resample(&filtered_chunk, samples_needed, 3)
            }
        }
    }

    /// Apply all enabled effects to the audio signal
    /// Note: modulator is already filtered in get_modulator_slice if lpf_enabled
    ///
    /// # Arguments
    /// * `signal` - The video signal (8-bit unsigned, 0-255)
    /// * `modulator` - The modulator signal (normalized float, -1.0 to +1.0, 0.0=neutral)
    /// * `effects` - Effect settings to apply
    fn apply_effects(&self, signal: &[u8], modulator: &[f32], effects: &EffectSettings) -> Vec<u8> {
        let mut processed = signal.to_vec();

        // Apply modulation effects using the modulator
        // (modulator is already filtered if lpf_enabled)
        if effects.am_enabled {
            processed =
                audio_effects::amplitude_modulation(&processed, modulator, effects.am_depth);
        }

        if effects.ring_mod_enabled {
            processed = audio_effects::ring_modulation(&processed, modulator, effects.ring_mod_mix);
        }

        if effects.fm_enabled {
            processed =
                audio_effects::frequency_modulation(&processed, modulator, effects.fm_depth);
        }

        // Apply signal processing effects with modulation
        if effects.distortion_enabled {
            processed = audio_effects::distortion_modulated(
                &processed,
                modulator,
                effects.distortion_amount,
            );
        }

        if effects.bit_crush_enabled {
            processed =
                audio_effects::bit_crush_modulated(&processed, modulator, effects.bit_crush_bits);
        }

        processed
    }

    /// Get the codec metadata
    pub fn metadata(&self) -> &VideoSignalMetadata {
        self.codec.metadata()
    }

    /// Check if modulator is loaded
    pub fn has_modulator(&self) -> bool {
        self.modulator_audio.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_frame_no_modulator() {
        let processor = VideoSignalProcessor::new(2, 2, 30.0, MappingMode::Accurate);

        let frame = vec![
            100, 100, 100, 255, 150, 150, 150, 255, 200, 200, 200, 255, 250, 250, 250, 255,
        ];

        let effects = EffectSettings::default();
        let result = processor.process_frame(&frame, &effects).unwrap();

        // Without modulator, should be nearly identical (slight loss in conversion)
        assert_eq!(result.len(), frame.len());
    }

    #[test]
    fn test_process_frame_with_modulator() {
        let mut processor = VideoSignalProcessor::new(2, 2, 30.0, MappingMode::Bugged);

        // Load a simple modulator with strong signal (normalized float: 1.0 = max)
        let modulator = vec![1.0; 100]; // Maximum amplitude
        processor.load_modulator(modulator);

        let frame = vec![
            50, 50, 50, 255, // Dark pixels to see brightening effect
            50, 50, 50, 255, 50, 50, 50, 255, 50, 50, 50, 255,
        ];

        let mut effects = EffectSettings::default();
        effects.am_enabled = true;
        effects.am_depth = 1.0; // Full depth for clear effect

        let result = processor.process_frame(&frame, &effects).unwrap();

        // With AM at full depth and max modulator, result should be brighter
        assert_eq!(result.len(), frame.len());
        // Check that at least one pixel is brighter
        let result_brightness: u32 = result.iter().step_by(4).take(3).map(|&x| x as u32).sum();
        let original_brightness: u32 = frame.iter().step_by(4).take(3).map(|&x| x as u32).sum();
        assert!(
            result_brightness > original_brightness,
            "Expected result to be brighter: {} vs {}",
            result_brightness,
            original_brightness
        );
    }
}
