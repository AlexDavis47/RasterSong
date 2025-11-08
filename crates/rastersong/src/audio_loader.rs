/// Audio file loading and conversion
/// 
/// Loads audio files (WAV, etc.) and converts them to normalized float samples for modulation

use anyhow::{Context, Result};
use std::path::Path;

/// Load audio file and convert to normalized float samples (mono, 44.1kHz)
/// 
/// # Arguments
/// * `path` - Path to audio file (currently supports WAV)
/// 
/// # Returns
/// * Vector of normalized float samples (-1.0 to +1.0, mono, 44.1kHz)
///   These maintain full precision regardless of source bit depth
pub fn load_audio_file<P: AsRef<Path>>(path: P) -> Result<Vec<f32>> {
    let path = path.as_ref();
    
    // Check file extension
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();
    
    match extension.as_str() {
        "wav" => load_wav_file(path),
        _ => anyhow::bail!(
            "Unsupported audio format: {}. Currently only WAV files are supported.",
            extension
        ),
    }
}

/// Load WAV file and convert to normalized float samples (mono, 44.1kHz)
fn load_wav_file<P: AsRef<Path>>(path: P) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path.as_ref())
        .context("Failed to open WAV file")?;
    
    let spec = reader.spec();
    println!("WAV file: {} channels, {} Hz, {} bits", 
             spec.channels, spec.sample_rate, spec.bits_per_sample);
    
    // Read all samples and convert to f32
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            reader.samples::<f32>()
                .collect::<Result<Vec<_>, _>>()
                .context("Failed to read float samples")?
        }
        hound::SampleFormat::Int => {
            match spec.bits_per_sample {
                8 => {
                    // 8-bit WAV is unsigned (0-255), but hound reads as i8 (-128 to 127)
                    // Convert: i8 value -> unsigned interpretation -> normalized float
                    reader.samples::<i8>()
                        .map(|s| s.map(|s| {
                            let unsigned = s as u8; // Reinterpret as unsigned
                            (unsigned as f32 - 128.0) / 128.0
                        }))
                        .collect::<Result<Vec<_>, _>>()
                        .context("Failed to read 8-bit samples")?
                },
                16 => reader.samples::<i16>()
                    .map(|s| s.map(|s| s as f32 / 32768.0))
                    .collect::<Result<Vec<_>, _>>()
                    .context("Failed to read 16-bit samples")?,
                24 => reader.samples::<i32>()
                    .map(|s| s.map(|s| s as f32 / 8388608.0))
                    .collect::<Result<Vec<_>, _>>()
                    .context("Failed to read 24-bit samples")?,
                32 => reader.samples::<i32>()
                    .map(|s| s.map(|s| s as f32 / 2147483648.0))
                    .collect::<Result<Vec<_>, _>>()
                    .context("Failed to read 32-bit samples")?,
                _ => anyhow::bail!("Unsupported bit depth: {}", spec.bits_per_sample),
            }
        }
    };
    
    // Convert to mono if stereo/multichannel
    let mono_samples = if spec.channels == 1 {
        samples
    } else {
        println!("Converting {} channels to mono", spec.channels);
        samples
            .chunks(spec.channels as usize)
            .map(|chunk| {
                // Average all channels
                chunk.iter().sum::<f32>() / chunk.len() as f32
            })
            .collect()
    };
    
    // Resample to 44.1kHz if needed
    let resampled = if spec.sample_rate != 44100 {
        println!("Resampling from {} Hz to 44100 Hz", spec.sample_rate);
        resample(&mono_samples, spec.sample_rate, 44100)
    } else {
        mono_samples
    };
    
    // Clamp to valid range and return as f32 (maintains full precision)
    let output: Vec<f32> = resampled
        .iter()
        .map(|&sample| sample.clamp(-1.0, 1.0))
        .collect();
    
    println!("Loaded {} samples (normalized float mono @ 44.1kHz)", output.len());
    
    Ok(output)
}

/// Simple linear resampling
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }
    
    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(output_len);
    
    for i in 0..output_len {
        let src_pos = i as f64 * ratio;
        let src_idx = src_pos as usize;
        let frac = src_pos - src_idx as f64;
        
        if src_idx + 1 < samples.len() {
            // Linear interpolation
            let sample0 = samples[src_idx];
            let sample1 = samples[src_idx + 1];
            let interpolated = sample0 + (sample1 - sample0) * frac as f32;
            output.push(interpolated);
        } else if src_idx < samples.len() {
            output.push(samples[src_idx]);
        }
    }
    
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_resample() {
        let samples = vec![0.0, 1.0, 0.0, -1.0];
        let resampled = resample(&samples, 4, 8);
        
        // Should have roughly 8 samples (double)
        assert!(resampled.len() >= 7 && resampled.len() <= 9);
    }
}

