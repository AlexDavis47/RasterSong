//! Turning decoded media into the graph's per-frame source signals.

use rastersong_graph::Signal;
use rastersong_media::{AudioClip, VideoFrame};

/// Decoded RGB8 → video signal in `0.0..=1.0`.
pub fn fill_video(frame: &VideoFrame, out: &mut Signal) {
    for (out, &byte) in out.data.iter_mut().zip(&frame.data) {
        *out = f32::from(byte) / 255.0;
    }
}

/// Graph output in `0.0..=1.0` → RGB8, clamping out-of-range values.
pub fn to_rgb8(signal: &Signal, out: &mut Vec<u8>) {
    out.clear();
    out.extend(
        signal
            .data
            .iter()
            .map(|&x| (x.clamp(0.0, 1.0) * 255.0).round() as u8),
    );
}

/// A mono modulator, sampled per frame.
#[derive(Debug, Clone)]
pub struct Modulator {
    samples: Vec<f32>,
    sample_rate: f64,
}

impl Modulator {
    /// Downmixes `clip` to mono by averaging its channels.
    pub fn new(clip: &AudioClip) -> Self {
        let channels = clip.channels.max(1) as usize;
        let samples = clip
            .samples
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        Self {
            samples,
            sample_rate: f64::from(clip.sample_rate),
        }
    }

    /// Samples per video frame: the modulator block length at this frame rate.
    pub fn block_len(&self, frame_rate: f64) -> u32 {
        (self.sample_rate / frame_rate).round().max(1.0) as u32
    }

    /// Fills `out` with the audio between `start` and `end` seconds, resampled to `out.len()`
    /// samples. Each video frame gets the audio of exactly its own time span, so audio and video
    /// never drift apart, even when the frame rate doesn't divide the sample rate (or varies).
    /// Time outside the clip is silence.
    pub fn fill_block(&self, start: f64, end: f64, out: &mut [f32]) {
        let n = out.len() as f64;
        let last = self.samples.len() as f64 - 1.0;
        for (k, out) in out.iter_mut().enumerate() {
            let t = start + (k as f64 + 0.5) / n * (end - start);
            let pos = t * self.sample_rate - 0.5;
            *out = if self.samples.is_empty() || pos < -0.5 || pos > last + 0.5 {
                0.0
            } else {
                let pos = pos.clamp(0.0, last);
                let i = pos as usize;
                let frac = (pos - i as f64) as f32;
                let next = self.samples[(i + 1).min(last as usize)];
                self.samples[i] + (next - self.samples[i]) * frac
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modulator(samples: Vec<f32>, sample_rate: u32) -> Modulator {
        Modulator::new(&AudioClip {
            sample_rate,
            channels: 1,
            samples,
        })
    }

    #[test]
    fn blocks_cover_their_frame_exactly() {
        // 10 Hz audio ramp, 2 fps video: frame 1 spans samples 5..10.
        let m = modulator((0..20).map(|i| i as f32).collect(), 10);
        assert_eq!(m.block_len(2.0), 5);
        let mut block = [0.0; 5];
        m.fill_block(0.5, 1.0, &mut block);
        assert_eq!(block, [5.0, 6.0, 7.0, 8.0, 9.0]);
    }

    #[test]
    fn non_integer_ratios_resample_without_drift() {
        // 48 kHz at 29.97 fps: 1601.6 samples per frame, rounded to 1602 per block. Frame 1000 still
        // starts exactly where it should.
        let rate = 48_000;
        let fps = 30_000.0 / 1001.0;
        let m = modulator((0..rate * 40).map(|i| i as f32).collect(), rate);
        let mut block = vec![0.0; m.block_len(fps) as usize];
        let start = 1000.0 / fps;
        m.fill_block(start, start + 1.0 / fps, &mut block);
        let expected_start = start * f64::from(rate);
        assert!(
            (f64::from(block[0]) - expected_start).abs() < 1.0,
            "{} vs {expected_start}",
            block[0]
        );
    }

    #[test]
    fn outside_the_clip_is_silence_and_channels_are_averaged() {
        let m = Modulator::new(&AudioClip {
            sample_rate: 4,
            channels: 2,
            samples: vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0],
        });
        let mut block = [9.0; 4];
        m.fill_block(0.0, 1.0, &mut block);
        assert_eq!(block, [0.5; 4]);
        m.fill_block(5.0, 6.0, &mut block);
        assert_eq!(block, [0.0; 4]);
    }

    #[test]
    fn rgb8_round_trips_and_clamps() {
        let frame = VideoFrame {
            width: 2,
            height: 1,
            data: vec![0, 1, 127, 128, 254, 255],
        };
        let mut signal = Signal::zeros(rastersong_graph::Layout::rgb(2, 1));
        fill_video(&frame, &mut signal);
        let mut rgb = Vec::new();
        to_rgb8(&signal, &mut rgb);
        assert_eq!(rgb, frame.data);

        signal.data[0] = -0.5;
        signal.data[1] = 1.5;
        to_rgb8(&signal, &mut rgb);
        assert_eq!(&rgb[..2], [0, 255]);
    }
}
