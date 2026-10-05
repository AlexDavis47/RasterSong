//! Turning decoded media into the graph's per-frame source signals.

use rastersong_graph::{Layout, Signal};
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

/// An audio track as the graph reads it, sampled per frame. Channels stay interleaved as decoded
/// (L, R, L, R, … for stereo): nothing is downmixed, so a graph that wants mono sums or splits
/// the channels itself.
#[derive(Debug, Clone)]
pub struct Modulator {
    /// Interleaved samples, `channels` per frame of audio.
    samples: Vec<f32>,
    channels: usize,
    sample_rate: f64,
}

impl Modulator {
    pub fn new(clip: &AudioClip) -> Self {
        let channels = clip.channels.max(1) as usize;
        let whole = clip.samples.len() / channels * channels;
        Self {
            samples: clip.samples[..whole].to_vec(),
            channels,
            sample_rate: f64::from(clip.sample_rate),
        }
    }

    /// No audio: the modulator is mono silence.
    pub fn silent() -> Self {
        Self {
            samples: Vec::new(),
            channels: 1,
            sample_rate: 48_000.0,
        }
    }

    /// Channels per frame of audio: 1 for mono, 2 for stereo.
    pub fn channels(&self) -> u32 {
        self.channels as u32
    }

    /// Frames of audio (one sample per channel each).
    fn frames(&self) -> usize {
        self.samples.len() / self.channels
    }

    pub fn duration_secs(&self) -> f64 {
        self.frames() as f64 / self.sample_rate
    }

    /// Frames of audio per video frame: the modulator block length at this frame rate. A block
    /// holds this many samples per channel.
    pub fn block_len(&self, frame_rate: f64) -> u32 {
        (self.sample_rate / frame_rate).round().max(1.0) as u32
    }

    /// The graph layout of one block at this frame rate.
    pub fn layout(&self, frame_rate: f64) -> Layout {
        Layout::audio_channels(self.block_len(frame_rate), self.channels())
    }

    /// Sample `channel` of audio frame `i`, or silence outside the clip.
    fn at(&self, i: i64, channel: usize) -> f32 {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.samples.get(i * self.channels + channel))
            .copied()
            .unwrap_or(0.0)
    }

    /// Fills `out` (interleaved, `channels` per frame) with the audio between `start` and `end`
    /// seconds, resampled to `out.len() / channels` frames. Each video frame gets the audio of
    /// exactly its own time span, so audio and video never drift apart, even when the frame rate
    /// doesn't divide the sample rate (or varies). Time outside the clip is silence.
    pub fn fill_block(&self, start: f64, end: f64, out: &mut [f32]) {
        let channels = self.channels;
        let n = (out.len() / channels) as f64;
        // The block lines up with whole source frames one-to-one: copy them.
        let first = start * self.sample_rate;
        let span = (end - start) * self.sample_rate;
        if (span - n).abs() < 1e-6 && (first - first.round()).abs() < 1e-6 {
            let first = first.round() as i64;
            for (k, frame) in out.chunks_exact_mut(channels).enumerate() {
                for (c, out) in frame.iter_mut().enumerate() {
                    *out = self.at(first + k as i64, c);
                }
            }
            return;
        }
        let last = self.frames() as f64 - 1.0;
        for (k, frame) in out.chunks_exact_mut(channels).enumerate() {
            let t = start + (k as f64 + 0.5) / n * (end - start);
            let pos = t * self.sample_rate - 0.5;
            if self.samples.is_empty() || pos < -0.5 || pos > last + 0.5 {
                frame.fill(0.0);
                continue;
            }
            let pos = pos.clamp(0.0, last);
            let i = pos as i64;
            let frac = (pos - i as f64) as f32;
            let next = (i + 1).min(last as i64);
            for (c, out) in frame.iter_mut().enumerate() {
                let (a, b) = (self.at(i, c), self.at(next, c));
                *out = a + (b - a) * frac;
            }
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
    fn aligned_blocks_are_exact_copies() {
        // Values that interpolation would round differently: a copy keeps them bit for bit.
        let samples: Vec<f32> = (0..96_000).map(|i| (i as f32 * 0.37).sin() / 3.0).collect();
        let m = modulator(samples.clone(), 48_000);
        let mut block = vec![0.0; m.block_len(25.0) as usize];
        let start = 37.0 / 25.0;
        m.fill_block(start, start + 1.0 / 25.0, &mut block);
        assert_eq!(block, samples[71_040..71_040 + 1920]);
        // Reaching past either end of the clip reads silence.
        m.fill_block(-1.0 / 25.0, 0.0, &mut block);
        assert!(block.iter().all(|&s| s == 0.0));
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
    fn outside_the_clip_is_silence_and_channels_stay_interleaved() {
        let m = Modulator::new(&AudioClip {
            sample_rate: 4,
            channels: 2,
            samples: vec![1.0, 0.0, 2.0, -1.0, 3.0, -2.0, 4.0, -3.0],
        });
        assert_eq!(m.channels(), 2);
        assert_eq!(m.layout(2.0), Layout::audio_channels(2, 2));
        let mut block = [9.0; 8];
        m.fill_block(0.0, 1.0, &mut block);
        assert_eq!(block, [1.0, 0.0, 2.0, -1.0, 3.0, -2.0, 4.0, -3.0]);
        m.fill_block(5.0, 6.0, &mut block);
        assert_eq!(block, [0.0; 8]);
        // Resampled, each channel is interpolated on its own: 4 frames into 2.
        let mut half = [9.0; 4];
        m.fill_block(0.0, 1.0, &mut half);
        assert_eq!(half, [1.5, -0.5, 3.5, -2.5]);
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
