//! Waveform overviews of audio tracks, for drawing in the timeline at any zoom.
//!
//! Built once per decoded file: the mono mix, plus a pyramid of (min, max) peaks over buckets of
//! 256, 1024, 4096, … samples. Drawing a waveform reads the coarsest level that still has at
//! least one bucket per pixel, so it costs about the same at any zoom.

use rastersong_media::AudioClip;

/// Samples per bucket at the finest level of the pyramid.
const BASE_BUCKET: usize = 256;
/// How many buckets of one level make a bucket of the next.
const LEVEL_FACTOR: usize = 4;

/// A (min, max) pair of sample values.
pub type Peak = (f32, f32);

#[derive(Debug, Clone)]
pub struct Waveform {
    sample_rate: f64,
    /// The mono mix, for zooms finer than the first level.
    samples: Vec<f32>,
    /// `levels[k]` holds peaks over buckets of `BASE_BUCKET × LEVEL_FACTOR^k` samples.
    levels: Vec<Vec<Peak>>,
}

impl Waveform {
    /// Mixes `clip` to mono (by averaging its channels) and builds its peak pyramid.
    pub fn new(clip: &AudioClip) -> Self {
        let channels = clip.channels.max(1) as usize;
        let samples: Vec<f32> = clip
            .samples
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        let mut levels: Vec<Vec<Peak>> = vec![
            samples
                .chunks(BASE_BUCKET)
                .map(|chunk| peak_of(chunk.iter().map(|&s| (s, s))))
                .collect(),
        ];
        while levels.last().is_some_and(|l| l.len() > 1) {
            let next = levels
                .last()
                .unwrap()
                .chunks(LEVEL_FACTOR)
                .map(|chunk| peak_of(chunk.iter().copied()))
                .collect();
            levels.push(next);
        }
        Self {
            sample_rate: f64::from(clip.sample_rate.max(1)),
            samples,
            levels,
        }
    }

    pub fn duration_secs(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate
    }

    /// Peaks of `buckets` equal slices of the time from `start` to `end` seconds (relative to
    /// the start of the clip), written to `out`. Time outside the clip is `(0, 0)`.
    pub fn peaks(&self, start: f64, end: f64, buckets: usize, out: &mut Vec<Peak>) {
        out.clear();
        if buckets == 0 || end <= start {
            return;
        }
        let per_bucket = (end - start) * self.sample_rate / buckets as f64;
        // The coarsest level with at least one of its buckets per output bucket.
        let mut level = None;
        let mut size = BASE_BUCKET as f64;
        for k in 0..self.levels.len() {
            if size > per_bucket {
                break;
            }
            level = Some(k);
            size *= LEVEL_FACTOR as f64;
        }
        for b in 0..buckets {
            let t0 = start + (end - start) * b as f64 / buckets as f64;
            let t1 = start + (end - start) * (b + 1) as f64 / buckets as f64;
            let s0 = (t0 * self.sample_rate).floor().max(0.0) as usize;
            let s1 = ((t1 * self.sample_rate).ceil().max(0.0) as usize).min(self.samples.len());
            let peak = if s0 >= s1 {
                (0.0, 0.0)
            } else {
                match level {
                    Some(k) => {
                        let bucket = BASE_BUCKET * LEVEL_FACTOR.pow(k as u32);
                        let peaks = &self.levels[k];
                        let i1 = s1.div_ceil(bucket).min(peaks.len());
                        let i0 = (s0 / bucket).min(i1.saturating_sub(1));
                        peak_of(peaks[i0..i1].iter().copied())
                    }
                    None => peak_of(self.samples[s0..s1].iter().map(|&s| (s, s))),
                }
            };
            out.push(peak);
        }
    }
}

fn peak_of(peaks: impl Iterator<Item = Peak>) -> Peak {
    peaks
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
        .unwrap_or((0.0, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(samples: Vec<f32>, sample_rate: u32) -> AudioClip {
        AudioClip {
            sample_rate,
            channels: 1,
            samples,
        }
    }

    #[test]
    fn peaks_cover_the_requested_time() {
        // One second of silence, then one second of a ±0.5 square wave. (A rate that's a multiple
        // of the bucket size keeps the halves in separate buckets.)
        let mut samples = vec![0.0; 1024];
        samples.extend((0..1024).map(|i| if i % 2 == 0 { 0.5 } else { -0.5 }));
        let waveform = Waveform::new(&clip(samples, 1024));
        assert_eq!(waveform.duration_secs(), 2.0);
        let mut out = Vec::new();
        waveform.peaks(0.0, 2.0, 4, &mut out);
        assert_eq!(out, [(0.0, 0.0), (0.0, 0.0), (-0.5, 0.5), (-0.5, 0.5)]);
        // Zoomed far in (fewer samples per pixel than a bucket) reads the samples themselves.
        waveform.peaks(1.0, 1.004, 4, &mut out);
        assert_eq!(out.len(), 4);
        assert!(
            out.iter()
                .all(|&(lo, hi)| lo.abs() == 0.5 && hi.abs() == 0.5)
        );
    }

    #[test]
    fn coarse_levels_agree_with_the_samples() {
        let samples: Vec<f32> = (0..200_000).map(|i| ((i as f32) * 0.001).sin()).collect();
        let waveform = Waveform::new(&clip(samples.clone(), 48_000));
        let mut out = Vec::new();
        waveform.peaks(0.0, waveform.duration_secs(), 10, &mut out);
        for (b, &(lo, hi)) in out.iter().enumerate() {
            let slice = &samples[b * 20_000..(b + 1) * 20_000];
            let min = slice.iter().copied().fold(f32::INFINITY, f32::min);
            let max = slice.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            // Level buckets may reach a little past the slice, never miss part of it.
            assert!(lo <= min && hi >= max, "bucket {b}");
            assert!(min - lo < 0.02 && hi - max < 0.02, "bucket {b}");
        }
    }

    #[test]
    fn time_outside_the_clip_is_flat() {
        let waveform = Waveform::new(&clip(vec![1.0; 100], 100));
        let mut out = Vec::new();
        waveform.peaks(-2.0, -1.0, 3, &mut out);
        assert_eq!(out, [(0.0, 0.0); 3]);
        waveform.peaks(5.0, 6.0, 2, &mut out);
        assert_eq!(out, [(0.0, 0.0); 2]);
    }
}
