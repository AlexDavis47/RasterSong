/// Video Signal Encoding/Decoding
///
/// Converts video frames to audio samples and back, enabling audio-based
/// processing of video data (datamoshing, audio modulation effects, etc.)
use anyhow::Result;

/// Mapping mode for video-to-audio conversion
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingMode {
    /// Accurate unsigned mapping (0-255 -> 1-255, cleaner)
    Accurate,
    /// Signed mapping (0-255 -> -127 to +127, more glitchy)
    Bugged,
}

/// Metadata for video signal encoding/decoding
#[derive(Debug, Clone)]
pub struct VideoSignalMetadata {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub total_frames: usize,
    pub bytes_per_frame: usize,
    pub sample_rate: u32,
    pub mapping_mode: MappingMode,
}

impl VideoSignalMetadata {
    pub fn new(width: u32, height: u32, fps: f32, mapping_mode: MappingMode) -> Self {
        let bytes_per_frame = (width * height * 3) as usize; // RGB = 3 bytes per pixel

        Self {
            width,
            height,
            fps,
            total_frames: 0,
            bytes_per_frame,
            sample_rate: 44100, // Standard audio sample rate
            mapping_mode,
        }
    }

    /// Calculate the effective sample rate for 1:1 video/audio sync
    /// This is the rate at which the video signal "plays" as audio
    pub fn effective_sample_rate(&self) -> f32 {
        // Samples per frame * frames per second = samples per second
        (self.bytes_per_frame as f32) * self.fps
    }

    /// Calculate the playback speed multiplier needed for sync
    /// This is how much faster the modulator audio needs to play to sync with video signal
    pub fn playback_speed_multiplier(&self) -> f32 {
        self.effective_sample_rate() / self.sample_rate as f32
    }
}

/// Video signal encoder/decoder
pub struct VideoSignalCodec {
    metadata: VideoSignalMetadata,
}

impl VideoSignalCodec {
    /// Create a new codec with the given metadata
    pub fn new(metadata: VideoSignalMetadata) -> Self {
        Self { metadata }
    }

    /// Get the metadata
    pub fn metadata(&self) -> &VideoSignalMetadata {
        &self.metadata
    }

    /// Encode a single RGBA frame to audio samples (8-bit mono)
    ///
    /// # Arguments
    /// * `frame_rgba` - RGBA frame data (width * height * 4 bytes)
    ///
    /// # Returns
    /// * Audio samples (8-bit, mono, RGB only - alpha channel discarded)
    pub fn encode_frame(&self, frame_rgba: &[u8]) -> Result<Vec<u8>> {
        let expected_size = (self.metadata.width * self.metadata.height * 4) as usize;

        if frame_rgba.len() != expected_size {
            anyhow::bail!(
                "Frame size mismatch: expected {} bytes, got {}",
                expected_size,
                frame_rgba.len()
            );
        }

        // Convert RGBA to RGB (discard alpha) and then to audio samples
        let mut audio_samples = Vec::with_capacity(self.metadata.bytes_per_frame);

        for chunk in frame_rgba.chunks_exact(4) {
            let r = chunk[0];
            let g = chunk[1];
            let b = chunk[2];
            // chunk[3] is alpha, we discard it

            // Convert RGB values to audio samples based on mapping mode
            let (r_audio, g_audio, b_audio) = match self.metadata.mapping_mode {
                MappingMode::Accurate => {
                    // Unsigned 8-bit: 0-255 -> 1-255 (with 128 as center)
                    // This avoids DC offset issues and is more "correct"
                    let r = (r as u16 + 1).clamp(1, 255) as u8;
                    let g = (g as u16 + 1).clamp(1, 255) as u8;
                    let b = (b as u16 + 1).clamp(1, 255) as u8;
                    (r, g, b)
                }
                MappingMode::Bugged => {
                    // Signed 8-bit interpreted as unsigned: 0-255 -> -127 to +127
                    // Stored as u8 but will be reinterpreted as i8 later
                    // Creates glitchy effects
                    let r = ((r as i16 - 127).clamp(-127, 127) as i8) as u8;
                    let g = ((g as i16 - 127).clamp(-127, 127) as i8) as u8;
                    let b = ((b as i16 - 127).clamp(-127, 127) as i8) as u8;
                    (r, g, b)
                }
            };

            audio_samples.push(r_audio);
            audio_samples.push(g_audio);
            audio_samples.push(b_audio);
        }

        Ok(audio_samples)
    }

    /// Decode audio samples back to an RGBA frame
    ///
    /// # Arguments
    /// * `audio_samples` - 8-bit mono audio samples (RGB interleaved)
    ///
    /// # Returns
    /// * RGBA frame data (width * height * 4 bytes, alpha set to 255)
    pub fn decode_frame(&self, audio_samples: &[u8]) -> Result<Vec<u8>> {
        if audio_samples.len() != self.metadata.bytes_per_frame {
            anyhow::bail!(
                "Audio sample count mismatch: expected {} samples, got {}",
                self.metadata.bytes_per_frame,
                audio_samples.len()
            );
        }

        let frame_size = (self.metadata.width * self.metadata.height * 4) as usize;
        let mut frame_rgba = Vec::with_capacity(frame_size);

        // Convert audio samples back to RGB, then add alpha
        for chunk in audio_samples.chunks_exact(3) {
            let r_audio = chunk[0];
            let g_audio = chunk[1];
            let b_audio = chunk[2];

            // Convert audio samples back to RGB based on mapping mode
            let (r, g, b) = match self.metadata.mapping_mode {
                MappingMode::Accurate => {
                    // Reverse: 1-255 -> 0-254 (note: 255 maps to 254, slight loss)
                    let r = (r_audio.saturating_sub(1)) as u8;
                    let g = (g_audio.saturating_sub(1)) as u8;
                    let b = (b_audio.saturating_sub(1)) as u8;
                    (r, g, b)
                }
                MappingMode::Bugged => {
                    // Reinterpret u8 as i8, then add 127: -127 to +127 -> 0-254
                    let r = ((r_audio as i8 as i16) + 127).clamp(0, 255) as u8;
                    let g = ((g_audio as i8 as i16) + 127).clamp(0, 255) as u8;
                    let b = ((b_audio as i8 as i16) + 127).clamp(0, 255) as u8;
                    (r, g, b)
                }
            };

            frame_rgba.push(r);
            frame_rgba.push(g);
            frame_rgba.push(b);
            frame_rgba.push(255); // Alpha = fully opaque
        }

        Ok(frame_rgba)
    }

    /// Encode multiple frames to a continuous audio buffer
    pub fn encode_frames(&mut self, frames: &[Vec<u8>]) -> Result<Vec<u8>> {
        let mut audio_buffer = Vec::new();

        for frame in frames {
            let audio_samples = self.encode_frame(frame)?;
            audio_buffer.extend_from_slice(&audio_samples);
        }

        self.metadata.total_frames += frames.len();
        Ok(audio_buffer)
    }

    /// Decode a continuous audio buffer to multiple frames
    pub fn decode_frames(&self, audio_buffer: &[u8]) -> Result<Vec<Vec<u8>>> {
        let samples_per_frame = self.metadata.bytes_per_frame;
        let num_complete_frames = audio_buffer.len() / samples_per_frame;

        let mut frames = Vec::with_capacity(num_complete_frames);

        for frame_idx in 0..num_complete_frames {
            let start = frame_idx * samples_per_frame;
            let end = start + samples_per_frame;
            let frame_samples = &audio_buffer[start..end];

            let frame = self.decode_frame(frame_samples)?;
            frames.push(frame);
        }

        // Handle remaining incomplete frame (if any) by padding with black
        let remaining_samples = audio_buffer.len() % samples_per_frame;
        if remaining_samples > 0 {
            let start = num_complete_frames * samples_per_frame;
            let mut padded_samples = audio_buffer[start..].to_vec();

            // Pad with black RGB values (based on mapping mode)
            let padding_value = match self.metadata.mapping_mode {
                MappingMode::Accurate => 1, // Black in accurate mode (0 -> 1)
                MappingMode::Bugged => 129, // Black in bugged mode (0-127=-127 as u8=129)
            };

            padded_samples.resize(samples_per_frame, padding_value);

            let frame = self.decode_frame(&padded_samples)?;
            frames.push(frame);
        }

        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_accurate() {
        let metadata = VideoSignalMetadata::new(2, 2, 30.0, MappingMode::Accurate);
        let codec = VideoSignalCodec::new(metadata);

        // Create a simple 2x2 RGBA frame (avoid 254-255 which are lossy)
        let frame = vec![
            253, 0, 0, 255, // Bright red
            0, 253, 0, 255, // Bright green
            0, 0, 253, 255, // Bright blue
            200, 150, 100, 255, // Some color
        ];

        let audio = codec.encode_frame(&frame).unwrap();
        let decoded = codec.decode_frame(&audio).unwrap();

        assert_eq!(frame, decoded);
    }

    #[test]
    fn test_encode_decode_bugged() {
        let metadata = VideoSignalMetadata::new(2, 2, 30.0, MappingMode::Bugged);
        let codec = VideoSignalCodec::new(metadata);

        let frame = vec![
            127, 127, 127, 255, // Mid-gray
            0, 0, 0, 255, // Black
            253, 253, 253, 255, // Near-white (avoid 254-255 which are lossy)
            64, 128, 192, 255, // Some color
        ];

        let audio = codec.encode_frame(&frame).unwrap();
        let decoded = codec.decode_frame(&audio).unwrap();

        assert_eq!(frame, decoded);
    }
}
