//! Turning decoded media into the graph's per-frame source signals.

use rastersong_graph::{Layout, Signal};
use rastersong_media::{AudioClip, Samples, VideoFrame};

use crate::timeline::Item;

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
    /// Interleaved samples, `channels` per frame of audio, shared with the clip (usually mapped
    /// from the audio cache) rather than copied.
    samples: Samples,
    channels: usize,
    sample_rate: f64,
}

impl Modulator {
    pub fn new(clip: &AudioClip) -> Self {
        Self {
            samples: clip.samples.clone(),
            channels: clip.channels.max(1) as usize,
            sample_rate: f64::from(clip.sample_rate),
        }
    }

    /// No audio: the modulator is mono silence.
    pub fn silent() -> Self {
        Self {
            samples: Samples::default(),
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

    /// Fills `out` (interleaved, `channels` per frame) with the timeline between `start` and
    /// `end` seconds, where `items` place this audio. Each item fades in and out at its edges
    /// ([`Item::gain_at`]) and is mixed over the items listed before it by that gain, so a later
    /// item replaces an earlier one where they overlap, crossfading at its edges; gaps are
    /// silence.
    pub fn fill_items(&self, items: &[Item], start: f64, end: f64, out: &mut [f32]) {
        out.fill(0.0);
        let channels = self.channels;
        let n = out.len() / channels;
        if n == 0 || end <= start {
            return;
        }
        let step = (end - start) / n as f64;
        let duration = self.duration_secs();
        // The output sample a timeline time falls at, limited to the block.
        let sample_at = |t: f64| ((t - start) / step).round().clamp(0.0, n as f64) as usize;
        // The first sample whose centre is at or after `t`, limited to `lo..=hi`.
        let centre_from = |t: f64, lo: usize, hi: usize| {
            ((t - start) / step - 0.5)
                .ceil()
                .clamp(lo as f64, hi as f64) as usize
        };
        let mut faded = Vec::new();
        for item in items.iter().filter(|i| !i.muted) {
            let item_end = item.timeline_end(duration);
            let (from, to) = (sample_at(item.position), sample_at(item_end));
            if from >= to {
                continue;
            }
            // Samples with their centre inside a fade are mixed by their gain; the rest of the
            // item replaces what is below it.
            let fade = item.fade(duration);
            let full_from = centre_from(item.position + fade, from, to);
            let full_to = centre_from(item_end - fade, full_from, to);
            let time = |k: usize| start + k as f64 * step;
            for (a, b) in [(from, full_from), (full_from, full_to), (full_to, to)] {
                if a >= b {
                    continue;
                }
                let (s0, s1) = (item.source_time(time(a)), item.source_time(time(b)));
                if (a, b) == (full_from, full_to) {
                    self.fill_block(s0, s1, &mut out[a * channels..b * channels]);
                    continue;
                }
                faded.resize((b - a) * channels, 0.0);
                self.fill_block(s0, s1, &mut faded);
                let below = &mut out[a * channels..b * channels];
                for (k, (frame, new)) in below
                    .chunks_exact_mut(channels)
                    .zip(faded.chunks_exact(channels))
                    .enumerate()
                {
                    let g = item.gain_at(time(a + k) + step / 2.0, duration) as f32;
                    for (out, &x) in frame.iter_mut().zip(new) {
                        *out += (x - *out) * g;
                    }
                }
            }
        }
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
            samples: samples.into(),
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
            samples: vec![1.0, 0.0, 2.0, -1.0, 3.0, -2.0, 4.0, -3.0].into(),
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
    fn items_place_audio_on_the_timeline() {
        // A 10 Hz ramp, 2 fps: one block is 5 samples, 0.5 s.
        let m = modulator((0..20).map(|i| i as f32).collect(), 10);
        let mut block = [9.0; 5];
        // From 0.2 s in, the file from its sample 3.
        let item = Item {
            start: 0.3,
            ..Item::whole(0.2)
        };
        m.fill_items(&[item], 0.0, 0.5, &mut block);
        assert_eq!(block, [0.0, 0.0, 3.0, 4.0, 5.0]);
        // Out point at 0.5 s of the file: the item ends after two samples, then silence.
        let item = Item {
            end: Some(0.5),
            ..item
        };
        m.fill_items(&[item], 0.0, 0.5, &mut block);
        assert_eq!(block, [0.0, 0.0, 3.0, 4.0, 0.0]);
        // At double speed it is resampled: each output sample reads the middle of its span.
        let item = Item {
            rate: 2.0,
            ..Item::whole(0.0)
        };
        m.fill_items(&[item], 0.5, 1.0, &mut block);
        assert_eq!(block, [10.5, 12.5, 14.5, 16.5, 18.5]);
        // Muted, or no items: silence.
        let muted = Item {
            muted: true,
            ..Item::whole(0.0)
        };
        m.fill_items(&[muted], 0.0, 0.5, &mut block);
        assert_eq!(block, [0.0; 5]);
        m.fill_items(&[], 0.0, 0.5, &mut block);
        assert_eq!(block, [0.0; 5]);
    }

    #[test]
    fn items_fade_in_and_out_at_their_edges() {
        use crate::timeline::EDGE_FADE;
        // 1 kHz: the 5 ms fades take 5 samples.
        let rate = 1000;
        let fade = (EDGE_FADE * f64::from(rate)) as usize;
        let low = modulator(vec![0.25; 1000], rate);
        let mut block = vec![9.0; 100];
        low.fill_items(&[Item::whole(0.02)], 0.0, 0.1, &mut block);
        assert_eq!(block[..20], [0.0; 20]);
        // Each sample's gain is taken at its centre: 0.1, 0.3, … of the way in.
        for k in 0..fade {
            let expected = 0.25 * (k as f32 + 0.5) / fade as f32;
            assert!(
                (block[20 + k] - expected).abs() < 1e-6,
                "{k}: {}",
                block[20 + k]
            );
        }
        assert_eq!(block[20 + fade..], [0.25; 100 - 20 - 5]);
        // At the end of the file it fades out the same way.
        low.fill_items(&[Item::whole(0.0)], 0.95, 1.05, &mut block);
        assert_eq!(block[..45], [0.25; 45]);
        assert!((block[45] - 0.25 * 0.9).abs() < 1e-6, "{}", block[45]);
        assert_eq!(block[50..], [0.0; 50]);
    }

    #[test]
    fn a_later_item_is_mixed_over_an_earlier_one_by_its_fade() {
        // One file, a ramp, placed twice: the second item, from 50 ms, plays the file from its
        // start again. Inside its fade the two crossfade; after it, only the second plays.
        let ramp = modulator((0..1000).map(|i| i as f32).collect(), 1000);
        let items = [Item::whole(0.0), Item::whole(0.05)];
        let mut block = vec![0.0; 100];
        ramp.fill_items(&items, 0.0, 0.1, &mut block);
        assert_eq!(
            block[10..50],
            (10..50).map(|i| i as f32).collect::<Vec<_>>()[..]
        );
        // Sample 50 is 10% of the way through the fade: 90% of the first item's 50, 10% of the
        // second's 0.
        assert!((block[50] - 45.0).abs() < 1e-4, "{}", block[50]);
        assert_eq!(
            block[55..],
            (5..50).map(|i| i as f32).collect::<Vec<_>>()[..]
        );
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
