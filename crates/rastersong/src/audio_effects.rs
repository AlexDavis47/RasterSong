/// Audio modulation effects for video signal processing
///
/// Applies audio-based modulation to video signal data

/// Amplitude Modulation (AM) - modulates the amplitude of the carrier signal
///
/// # Arguments
/// * `carrier` - The video signal (8-bit unsigned, 0-255)
/// * `modulator` - The modulating audio signal (normalized float, -1.0 to +1.0, 0.0=neutral)
/// * `depth` - Modulation depth (0.0 = no effect, 1.0 = full modulation)
pub fn amplitude_modulation(carrier: &[u8], modulator: &[f32], depth: f32) -> Vec<u8> {
    let depth = depth.clamp(0.0, 1.0);
    let mut output = Vec::with_capacity(carrier.len());

    for i in 0..carrier.len() {
        let carrier_sample = carrier[i] as f32;

        // Get modulator sample (loop if modulator is shorter)
        let mod_sample = if modulator.is_empty() {
            0.0 // Neutral value if no modulator
        } else {
            modulator[i % modulator.len()].clamp(-1.0, 1.0)
        };

        // Modulator is already normalized (-1.0 to +1.0)
        // Apply AM: carrier * (1 + depth * modulator)
        let modulated = carrier_sample * (1.0 + depth * mod_sample);

        output.push(modulated.clamp(0.0, 255.0) as u8);
    }

    output
}

/// Ring Modulation - multiplies video signal by modulator amplitude
///
/// Uses the absolute value of the modulator to multiply the video signal.
///
/// # Arguments
/// * `carrier` - The video signal (8-bit unsigned, 0-255)
/// * `modulator` - The modulating audio signal (normalized float, -1.0 to +1.0, 0.0=neutral)
/// * `mix` - Dry/wet mix (0.0 = dry, 1.0 = fully ring modulated)
pub fn ring_modulation(carrier: &[u8], modulator: &[f32], mix: f32) -> Vec<u8> {
    let mix = mix.clamp(0.0, 1.0);
    let mut output = Vec::with_capacity(carrier.len());

    for i in 0..carrier.len() {
        let carrier_sample = carrier[i] as f32;

        let mod_sample = if modulator.is_empty() {
            0.0 // Neutral value
        } else {
            modulator[i % modulator.len()].clamp(-1.0, 1.0)
        };

        // Take absolute value of modulator: |mod| gives amplitude (0.0 to 1.0)
        // At neutral (0.0): intensity = 0.0, preserve signal
        // At max (±1.0): intensity = 1.0, apply full ring mod
        let mod_intensity = mod_sample.abs();

        // Multiply video signal by modulator intensity
        let ring_mod = if mod_intensity > 0.0 {
            carrier_sample * mod_intensity
        } else {
            carrier_sample // At neutral, preserve original
        };

        // Mix with dry signal
        let mixed = carrier_sample * (1.0 - mix) + ring_mod * mix;

        output.push(mixed.clamp(0.0, 255.0) as u8);
    }

    output
}

/// Frequency Modulation (FM) - uses modulator to vary the "frequency" of carrier
/// For video signals, this creates wavey distortion effects
///
/// # Arguments
/// * `carrier` - The video signal (8-bit unsigned, 0-255)
/// * `modulator` - The modulating audio signal (normalized float, -1.0 to +1.0, 0.0=neutral)
/// * `depth` - Modulation depth (how much to offset the read position)
pub fn frequency_modulation(carrier: &[u8], modulator: &[f32], depth: f32) -> Vec<u8> {
    if carrier.is_empty() {
        return Vec::new();
    }

    let depth = depth.clamp(0.0, 100.0); // Max offset of 100 samples
    let mut output = Vec::with_capacity(carrier.len());

    for i in 0..carrier.len() {
        let mod_sample = if modulator.is_empty() {
            0.0 // Neutral value
        } else {
            modulator[i % modulator.len()].clamp(-1.0, 1.0)
        };

        // Modulator is already normalized (-1.0 to +1.0)
        // Calculate offset based on modulator
        let offset = (mod_sample * depth) as i32;
        let read_pos = (i as i32 + offset).clamp(0, carrier.len() as i32 - 1) as usize;

        output.push(carrier[read_pos]);
    }

    output
}

/// Simple distortion effect
///
/// # Arguments
/// * `signal` - The signal to distort
/// * `amount` - Distortion amount (0.0 = clean, 1.0 = heavy distortion)
pub fn distortion(signal: &[u8], amount: f32) -> Vec<u8> {
    let amount = amount.clamp(0.0, 1.0);
    let mut output = Vec::with_capacity(signal.len());

    for &sample in signal {
        let normalized = (sample as f32 - 128.0) / 128.0;

        // Soft clipping distortion
        let distorted = if amount > 0.0 {
            let drive = 1.0 + (amount * 10.0);
            let driven = normalized * drive;
            driven.tanh() // Soft clipping
        } else {
            normalized
        };

        let output_sample = (distorted * 128.0) + 128.0;
        output.push(output_sample.clamp(0.0, 255.0) as u8);
    }

    output
}

/// Distortion with modulation - amount is modulated by the modulation signal
///
/// Uses the absolute value of the modulator amplitude to control distortion amount.
/// The modulated distortion is then applied to the video signal.
///
/// # Arguments
/// * `signal` - The video signal (8-bit unsigned, 0-255)
/// * `modulator` - The modulating audio signal (normalized float, -1.0 to +1.0, 0.0=neutral)
/// * `amount` - Base distortion amount (0.0 = clean, 1.0 = heavy distortion)
pub fn distortion_modulated(signal: &[u8], modulator: &[f32], amount: f32) -> Vec<u8> {
    let amount = amount.clamp(0.0, 1.0);
    let mut output = Vec::with_capacity(signal.len());

    for i in 0..signal.len() {
        let video_sample = signal[i] as f32;

        // Get modulator sample (loop if modulator is shorter)
        let mod_sample = if modulator.is_empty() {
            0.0 // Neutral value if no modulator
        } else {
            modulator[i % modulator.len()].clamp(-1.0, 1.0)
        };

        // Take absolute value of modulator: |mod| gives amplitude (0.0 to 1.0)
        // At neutral (0.0): intensity = 0.0, no distortion
        // At max (±1.0): intensity = 1.0, full distortion
        let mod_intensity = mod_sample.abs();

        // Modulate the distortion amount based on modulator amplitude
        let modulated_amount = (amount * mod_intensity).clamp(0.0, 1.0);

        // Normalize video signal to -1.0 to +1.0 for processing
        // Note: 128 is treated as zero/neutral, regardless of mapping mode
        let normalized = (video_sample - 128.0) / 128.0;

        // Apply soft clipping distortion with modulated amount
        let distorted = if modulated_amount > 0.0 {
            let drive = 1.0 + (modulated_amount * 10.0);
            let driven = normalized * drive;
            driven.tanh() // Soft clipping
        } else {
            normalized
        };

        // Convert back to 0-255 range
        let distorted_sample = (distorted * 128.0) + 128.0;
        output.push(distorted_sample.clamp(0.0, 255.0) as u8);
    }

    output
}

/// Bit crushing effect - reduces bit depth
///
/// # Arguments
/// * `signal` - The signal to crush
/// * `bits` - Target bit depth (1-8)
pub fn bit_crush(signal: &[u8], bits: u8) -> Vec<u8> {
    let bits = bits.clamp(1, 8);
    let levels = 1 << bits; // 2^bits
    let step = 256.0 / levels as f32;

    signal
        .iter()
        .map(|&sample| {
            let quantized = (sample as f32 / step).floor() * step;
            quantized.clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// Bit crushing effect - reduces bit depth with modulation
///
/// # Arguments
/// * `signal` - The signal to crush (8-bit unsigned, 0-255)
/// * `modulator` - The modulating audio signal (normalized float, -1.0 to +1.0, 0.0=neutral)
/// * `bits` - Base bit depth (1-8)
pub fn bit_crush_modulated(signal: &[u8], modulator: &[f32], bits: u8) -> Vec<u8> {
    let bits = bits.clamp(1, 8);
    let mut output = Vec::with_capacity(signal.len());

    for i in 0..signal.len() {
        let sample = signal[i] as f32;

        // Get modulator sample (loop if modulator is shorter)
        let mod_sample = if modulator.is_empty() {
            0.0 // Neutral value if no modulator
        } else {
            modulator[i % modulator.len()].clamp(-1.0, 1.0)
        };

        // Take absolute value of modulator: |mod| gives amplitude (0.0 to 1.0)
        // At neutral (0.0): intensity = 0.0, use base bits
        // At max (±1.0): intensity = 1.0, use more bits (less crushing)
        let mod_intensity = mod_sample.abs();

        // Modulate the bit depth: neutral = base bits, extreme = more bits
        // Scale: base + (intensity * base * 0.5) gives range from base to 1.5x base
        let modulated_bits = ((bits as f32 * (1.0 + mod_intensity * 0.5)).clamp(1.0, 8.0)) as u8;
        let levels = 1 << modulated_bits; // 2^modulated_bits
        let new_step = 256.0 / levels as f32;

        let quantized = (sample / new_step).floor() * new_step;
        output.push(quantized.clamp(0.0, 255.0) as u8);
    }

    output
}

/// Low Pass Filter - smooths the signal by attenuating high frequencies
///
/// # Arguments
/// * `signal` - The input signal
/// * `cutoff_hz` - Cutoff frequency in Hz (frequencies above this are attenuated)
/// * `sample_rate` - Sample rate of the signal in Hz
pub fn low_pass_filter(signal: &[u8], cutoff_hz: f32, sample_rate: f32) -> Vec<u8> {
    if signal.is_empty() || cutoff_hz <= 0.0 {
        return signal.to_vec();
    }

    let mut output = Vec::with_capacity(signal.len());

    // Calculate filter coefficient (RC filter)
    // α = 2π * fc * dt / (1 + 2π * fc * dt)
    // where dt = 1/sample_rate
    let dt = 1.0 / sample_rate;
    let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz);
    let alpha = dt / (rc + dt);

    // First sample passes through
    let mut prev_output = signal[0] as f32;
    output.push(signal[0]);

    // Apply filter: y[n] = y[n-1] + α * (x[n] - y[n-1])
    for &sample in &signal[1..] {
        let input = sample as f32;
        let filtered = prev_output + alpha * (input - prev_output);
        output.push(filtered.clamp(0.0, 255.0) as u8);
        prev_output = filtered;
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to check if values are within valid range
    // Note: u8 is always 0-255, but this function documents the intent
    fn assert_valid_range(samples: &[u8]) {
        // All u8 values are automatically in valid range (0-255)
        // This function exists for documentation/clarity
        let _ = samples;
    }

    // ========== Format Documentation Tests ==========
    // These tests document the expected formats for signals

    /// Documents the video signal format:
    /// - Format: 8-bit unsigned (u8, 0-255)
    /// - Accurate mode: RGB 0-255 -> Audio 1-255 (black=1, mid-gray=128)
    /// - Bugged mode: RGB 0-255 -> Audio stored as u8 but represents signed -127 to +127
    ///   When interpreted: black (RGB 0) = 129 as u8 (when read as i8 = -127)
    /// - Both modes store as Vec<u8> in effects functions
    #[test]
    fn test_video_signal_format_documentation() {
        // Video signals in effects are always 8-bit unsigned (0-255)
        let video_signal: Vec<u8> = vec![0, 1, 128, 255];

        // All values must be 0-255 (u8 is automatically 0-255)
        let _ = video_signal; // Documented for clarity

        // Black values depend on mapping mode:
        // - Accurate: black (RGB 0) becomes 1 in audio signal
        // - Bugged: black (RGB 0) becomes 129 in audio signal (u8 representation of i8 -127)
        // But in the effects functions, we just see the u8 values
        assert_eq!(video_signal[0], 0); // Could be black in bugged mode context
        assert_eq!(video_signal[1], 1); // Black in accurate mode
        assert_eq!(video_signal[2], 128); // Mid-gray
        assert_eq!(video_signal[3], 255); // White
    }

    /// Documents the modulation signal format:
    /// - Format: Normalized float (f32, -1.0 to +1.0)
    /// - Loaded from WAV files, maintains full precision regardless of source bit depth
    /// - 0.0 = neutral/silence (DC offset)
    /// - -1.0 = minimum amplitude
    /// - +1.0 = maximum amplitude
    /// - NOTE: Modulators are kept as f32 internally for full precision; only video signal uses u8
    #[test]
    fn test_modulation_signal_format_documentation() {
        // Modulation signals are normalized floats (-1.0 to +1.0)
        let modulator: Vec<f32> = vec![-1.0, 0.0, 1.0];

        // All values must be -1.0 to +1.0
        for &sample in &modulator {
            assert!(sample >= -1.0 && sample <= 1.0);
        }

        // Key values:
        assert!((modulator[0] - (-1.0)).abs() < 0.01); // Minimum amplitude
        assert!((modulator[1] - 0.0).abs() < 0.01); // Neutral/silence (DC offset)
        assert!((modulator[2] - 1.0).abs() < 0.01); // Maximum amplitude
    }

    // Helper function to check smoothness (no abrupt jumps > threshold)
    fn assert_smooth(samples: &[u8], threshold: u8) {
        if samples.len() < 2 {
            return;
        }
        for i in 1..samples.len() {
            let diff = if samples[i] > samples[i - 1] {
                samples[i] - samples[i - 1]
            } else {
                samples[i - 1] - samples[i]
            };
            assert!(
                diff <= threshold,
                "Abrupt jump detected: {} -> {} (diff: {})",
                samples[i - 1],
                samples[i],
                diff
            );
        }
    }

    // ========== Amplitude Modulation Tests ==========

    #[test]
    fn test_amplitude_modulation_basic() {
        let carrier = vec![128; 100]; // Mid-gray
        let modulator = vec![1.0; 100]; // Max value (normalized float)

        let result = amplitude_modulation(&carrier, &modulator, 0.5);

        assert_eq!(result.len(), carrier.len());
        assert_valid_range(&result);
        // With full modulator and 50% depth, should brighten
        assert!(result[0] > 128);
    }

    #[test]
    fn test_amplitude_modulation_zero_depth() {
        let carrier = vec![128; 50];
        let modulator = vec![1.0; 50];

        let result = amplitude_modulation(&carrier, &modulator, 0.0);

        // With zero depth, should be unchanged
        assert_eq!(result, carrier);
    }

    #[test]
    fn test_amplitude_modulation_neutral_modulator() {
        let carrier = vec![128; 50];
        let modulator = vec![0.0; 50]; // Neutral (0.0)

        let result = amplitude_modulation(&carrier, &modulator, 1.0);

        // With neutral modulator, should be unchanged
        assert_eq!(result, carrier);
    }

    #[test]
    fn test_amplitude_modulation_empty_modulator() {
        let carrier = vec![128; 50];
        let modulator = vec![];

        let result = amplitude_modulation(&carrier, &modulator, 0.5);

        // Empty modulator should use neutral value (0.0), so no change
        assert_eq!(result, carrier);
    }

    #[test]
    fn test_amplitude_modulation_short_modulator() {
        let carrier = vec![128; 100];
        let modulator = vec![1.0]; // Only 1 sample, should loop

        let result = amplitude_modulation(&carrier, &modulator, 1.0);

        assert_eq!(result.len(), carrier.len());
        // All samples should be affected the same way (looping modulator)
        assert!(result.iter().all(|&x| x == result[0]));
    }

    #[test]
    fn test_amplitude_modulation_smooth_transition() {
        // Create a smooth modulator signal (sine wave)
        let carrier = vec![128; 100];
        let modulator: Vec<f32> = (0..100)
            .map(|i| {
                let phase = (i as f32 / 100.0) * std::f32::consts::PI * 2.0;
                phase.sin() // Normalized float (-1.0 to +1.0)
            })
            .collect();

        let result = amplitude_modulation(&carrier, &modulator, 0.5);

        assert_eq!(result.len(), carrier.len());
        assert_valid_range(&result);
        // Check for smoothness (no abrupt jumps > 10)
        assert_smooth(&result, 10);
    }

    // ========== Ring Modulation Tests ==========

    #[test]
    fn test_ring_modulation_basic() {
        let carrier = vec![200; 50];
        let modulator = vec![0.5; 50]; // Non-neutral value

        let result = ring_modulation(&carrier, &modulator, 1.0);

        assert_eq!(result.len(), carrier.len());
        assert_valid_range(&result);
        assert_ne!(result[0], carrier[0]);
    }

    #[test]
    fn test_ring_modulation_zero_mix() {
        let carrier = vec![200; 50];
        let modulator = vec![0.5; 50];

        let result = ring_modulation(&carrier, &modulator, 0.0);

        // With zero mix, should be unchanged (dry signal)
        assert_eq!(result, carrier);
    }

    #[test]
    fn test_ring_modulation_neutral_modulator() {
        let carrier = vec![200; 50];
        let modulator = vec![0.0; 50]; // Neutral

        let result = ring_modulation(&carrier, &modulator, 1.0);

        // Ring mod with neutral modulator (intensity = 0.0) should preserve carrier
        // At mix=1.0, we use ring_mod which equals carrier when intensity=0
        assert_eq!(result, carrier);

        // Test with mix=0.0 should also preserve
        let result_dry = ring_modulation(&carrier, &modulator, 0.0);
        assert_eq!(result_dry, carrier);
    }

    // ========== Frequency Modulation Tests ==========

    #[test]
    fn test_frequency_modulation_basic() {
        let carrier: Vec<u8> = (0..=255).collect();
        let modulator = vec![0.0; 256]; // Neutral

        let result = frequency_modulation(&carrier, &modulator, 5.0);

        assert_eq!(result.len(), carrier.len());
        assert_valid_range(&result);
    }

    #[test]
    fn test_frequency_modulation_zero_depth() {
        let carrier: Vec<u8> = (0..=100).collect();
        let modulator = vec![1.0; 101];

        let result = frequency_modulation(&carrier, &modulator, 0.0);

        // With zero depth, should be unchanged
        assert_eq!(result, carrier);
    }

    #[test]
    fn test_frequency_modulation_empty_carrier() {
        let carrier = vec![];
        let modulator = vec![0.0; 10];

        let result = frequency_modulation(&carrier, &modulator, 10.0);

        assert_eq!(result.len(), 0);
    }

    // ========== Distortion Tests ==========

    #[test]
    fn test_distortion_basic() {
        let signal = vec![128; 50];

        let result = distortion(&signal, 0.5);

        assert_eq!(result.len(), signal.len());
        assert_valid_range(&result);
    }

    #[test]
    fn test_distortion_zero_amount() {
        let signal = vec![200; 50];

        let result = distortion(&signal, 0.0);

        // With zero distortion, should be unchanged
        assert_eq!(result, signal);
    }

    #[test]
    fn test_distortion_max_amount() {
        let signal = vec![128; 50];

        let result = distortion(&signal, 1.0);

        assert_eq!(result.len(), signal.len());
        assert_valid_range(&result);
    }

    #[test]
    fn test_distortion_boundaries() {
        let signal = vec![0, 128, 255];

        let result = distortion(&signal, 0.5);

        assert_eq!(result.len(), signal.len());
        assert_valid_range(&result);
    }

    // ========== Distortion Modulated Tests ==========

    #[test]
    fn test_distortion_modulated_basic() {
        let signal = vec![128; 50];
        let modulator = vec![1.0; 50]; // Max value (normalized float)

        let result = distortion_modulated(&signal, &modulator, 0.5);

        assert_eq!(result.len(), signal.len());
        assert_valid_range(&result);
    }

    #[test]
    fn test_distortion_modulated_zero_amount() {
        let signal = vec![200; 50];
        let modulator = vec![1.0; 50];

        let result = distortion_modulated(&signal, &modulator, 0.0);

        // With zero base amount, should be unchanged
        assert_eq!(result, signal);
    }

    #[test]
    fn test_distortion_modulated_neutral_modulator() {
        let signal = vec![200; 50]; // Non-neutral signal to see distortion effect
        let modulator = vec![0.0; 50]; // Neutral (0.0)

        let result = distortion_modulated(&signal, &modulator, 0.5);

        // With neutral modulator: abs(0.0) = 0, intensity = 0, so no distortion
        // Result should be unchanged
        assert_eq!(result, signal);

        // Compare with empty modulator (should be same as neutral)
        let result_zero_mod = distortion_modulated(&signal, &[], 0.5);
        assert_eq!(
            result, result_zero_mod,
            "Empty modulator should behave same as neutral"
        );
    }

    #[test]
    fn test_distortion_modulated_max_distortion_with_extreme_modulator() {
        let signal = vec![200; 50];
        let modulator_min = vec![-1.0; 50]; // Minimum value
        let modulator_max = vec![1.0; 50]; // Maximum value

        // Both extremes should give maximum intensity (abs value = 1.0)
        // abs(-1.0) = 1.0 → intensity = 1.0
        // abs(1.0) = 1.0 → intensity = 1.0
        let result_min = distortion_modulated(&signal, &modulator_min, 0.5);
        let result_max = distortion_modulated(&signal, &modulator_max, 0.5);

        // Both should apply distortion (intensity = 1.0, so modulated_amount = 0.5)
        assert_eq!(result_min.len(), signal.len());
        assert_eq!(result_max.len(), signal.len());
        assert_valid_range(&result_min);
        assert_valid_range(&result_max);
        assert_ne!(
            result_min[0], signal[0],
            "Extreme modulator should apply distortion"
        );

        // Both extremes should give similar results (both have intensity ≈ 1.0)
        // Small difference due to 0→128=128 vs 255→128=127, but both normalize to ~1.0
        let diff = (result_min[0] as i16 - result_max[0] as i16).abs();
        assert!(
            diff <= 1,
            "Min and max modulators should give similar intensity"
        );

        // Compare with neutral modulator - should have NO distortion
        let result_neutral = distortion_modulated(&signal, &vec![0.0; 50], 0.5);
        assert_eq!(
            result_neutral, signal,
            "Neutral modulator should have no effect"
        );
        assert_ne!(
            result_min[0], result_neutral[0],
            "Extreme modulator should differ from neutral"
        );
    }

    #[test]
    fn test_distortion_modulated_smooth_transition() {
        // Create smooth modulator
        let signal = vec![128; 100];
        let modulator: Vec<f32> = (0..100)
            .map(|i| {
                let phase = (i as f32 / 100.0) * std::f32::consts::PI * 2.0;
                phase.sin() // Normalized float (-1.0 to +1.0)
            })
            .collect();

        let result = distortion_modulated(&signal, &modulator, 0.5);

        assert_eq!(result.len(), signal.len());
        assert_valid_range(&result);
        // Should be smooth (no abrupt jumps > 5)
        assert_smooth(&result, 5);
    }

    // ========== Bit Crush Tests ==========

    #[test]
    fn test_bit_crush_basic() {
        let signal: Vec<u8> = (0..=255).collect();

        let result = bit_crush(&signal, 4);

        assert_eq!(result.len(), signal.len());
        assert_valid_range(&result);
        // With 4 bits, should have quantization
        assert_ne!(result, signal);
    }

    #[test]
    fn test_bit_crush_8_bits() {
        let signal: Vec<u8> = (0..=255).collect();

        let result = bit_crush(&signal, 8);

        // With 8 bits, should be nearly identical
        assert_eq!(result.len(), signal.len());
    }

    #[test]
    fn test_bit_crush_1_bit() {
        let signal: Vec<u8> = (0..=255).collect();

        let result = bit_crush(&signal, 1);

        assert_eq!(result.len(), signal.len());
        assert_valid_range(&result);
        // With 1 bit, should only have 2 levels
        let unique_values: std::collections::HashSet<u8> = result.iter().cloned().collect();
        assert!(unique_values.len() <= 2);
    }

    // ========== Bit Crush Modulated Tests ==========

    #[test]
    fn test_bit_crush_modulated_basic() {
        let signal: Vec<u8> = (0..=255).collect();
        let modulator = vec![1.0; 256]; // Max value (normalized float)

        let result = bit_crush_modulated(&signal, &modulator, 4);

        assert_eq!(result.len(), signal.len());
        assert_valid_range(&result);
    }

    #[test]
    fn test_bit_crush_modulated_neutral_modulator() {
        let signal: Vec<u8> = (0..=255).collect();
        let modulator = vec![0.0; 256]; // Neutral

        let result = bit_crush_modulated(&signal, &modulator, 4);

        assert_eq!(result.len(), signal.len());
        assert_valid_range(&result);
        // With neutral modulator (intensity = 0.5), should use half the bits
    }

    #[test]
    fn test_bit_crush_modulated_smooth_transition() {
        let signal: Vec<u8> = (0..=100).collect();
        let modulator: Vec<f32> = (0..101)
            .map(|i| {
                let phase = (i as f32 / 101.0) * std::f32::consts::PI * 2.0;
                phase.sin() // Normalized float (-1.0 to +1.0)
            })
            .collect();

        let result = bit_crush_modulated(&signal, &modulator, 4);

        assert_eq!(result.len(), signal.len());
        assert_valid_range(&result);
    }

    // ========== Low Pass Filter Tests ==========

    #[test]
    fn test_low_pass_filter_basic() {
        let signal: Vec<u8> = (0..=255).collect();

        let result = low_pass_filter(&signal, 1000.0, 44100.0);

        assert_eq!(result.len(), signal.len());
        assert_valid_range(&result);
    }

    #[test]
    fn test_low_pass_filter_high_cutoff() {
        let signal: Vec<u8> = (0..=255).collect();

        let result = low_pass_filter(&signal, 20000.0, 44100.0);

        // With very high cutoff, should be nearly unchanged
        assert_eq!(result.len(), signal.len());
        // First sample should be unchanged
        assert_eq!(result[0], signal[0]);
    }

    #[test]
    fn test_low_pass_filter_empty_signal() {
        let signal = vec![];

        let result = low_pass_filter(&signal, 1000.0, 44100.0);

        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_low_pass_filter_zero_cutoff() {
        let signal = vec![128, 200, 100, 255];

        let result = low_pass_filter(&signal, 0.0, 44100.0);

        // With zero cutoff, should return unchanged
        assert_eq!(result, signal);
    }

    #[test]
    fn test_low_pass_filter_smoothness() {
        // Create a signal with sharp transitions
        let mut signal = vec![0; 50];
        signal.extend(vec![255; 50]);
        signal.extend(vec![0; 50]);

        let result = low_pass_filter(&signal, 500.0, 44100.0);

        assert_eq!(result.len(), signal.len());
        assert_valid_range(&result);
        // Low pass filter should smooth transitions
        assert_smooth(&result, 20);
    }
}
