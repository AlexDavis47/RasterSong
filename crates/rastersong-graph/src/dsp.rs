//! Building blocks shared by the runtime and the nodes.

use std::f64::consts::TAU;

use crate::Interpolation;

/// Resamples one frame block `src` to `dst.len()` samples.
///
/// `group` keeps runs of output samples together: with `group = 3`, each RGB pixel of the output
/// maps to a single source position, so a mono signal stretched over RGB affects a pixel's R, G
/// and B equally. Positions are centre-aligned, so a block maps onto exactly the same span of
/// time or space at any length. Linear interpolation clamps at the block edges.
pub fn resample(src: &[f32], dst: &mut [f32], group: usize, mode: Interpolation) {
    if src.len() == dst.len() || src.is_empty() {
        if src.len() == dst.len() {
            dst.copy_from_slice(src);
        } else {
            dst.fill(0.0);
        }
        return;
    }
    let groups = dst.len() / group;
    let ratio = src.len() as f64 / groups as f64;
    let last = src.len() - 1;
    for (g, chunk) in dst.chunks_mut(group).enumerate() {
        let pos = (g as f64 + 0.5) * ratio - 0.5;
        let value = match mode {
            Interpolation::Hold => src[(((g as f64 + 0.5) * ratio) as usize).min(last)],
            Interpolation::Linear => {
                let pos = pos.clamp(0.0, last as f64);
                let i = pos as usize;
                let frac = (pos - i as f64) as f32;
                let next = src[(i + 1).min(last)];
                src[i] + (next - src[i]) * frac
            }
        };
        chunk.fill(value);
    }
}

/// A ring buffer delay line with fractional (linearly interpolated) reads.
#[derive(Debug, Clone, Default)]
pub struct DelayLine {
    buffer: Vec<f32>,
    /// Index the next sample will be written to.
    write: usize,
}

impl DelayLine {
    /// A delay line that can read up to `max_delay` samples into the past.
    pub fn new(max_delay: usize) -> Self {
        Self {
            buffer: vec![0.0; max_delay + 2],
            write: 0,
        }
    }

    pub fn max_delay(&self) -> usize {
        self.buffer.len().saturating_sub(2)
    }

    pub fn push(&mut self, sample: f32) {
        self.buffer[self.write] = sample;
        self.write = (self.write + 1) % self.buffer.len();
    }

    /// The sample pushed `delay` samples ago, where 0 is the most recent. Fractional delays
    /// interpolate linearly; delays beyond [`Self::max_delay`] are clamped.
    pub fn read(&self, delay: f64) -> f32 {
        let delay = delay.clamp(0.0, self.max_delay() as f64);
        let whole = delay as usize;
        let frac = (delay - whole as f64) as f32;
        let len = self.buffer.len();
        let newer = self.buffer[(self.write + len - 1 - whole) % len];
        if frac == 0.0 {
            return newer;
        }
        let older = self.buffer[(self.write + len - 2 - whole) % len];
        newer + (older - newer) * frac
    }

    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }
}

/// Decibels to a linear gain.
pub fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// A linear level to decibels; silence is a very large negative number rather than -inf.
pub fn gain_to_db(gain: f64) -> f64 {
    20.0 * gain.max(1e-12).log10()
}

/// Coefficient of a one-pole smoother with a time constant of `samples`: each sample moves
/// `1 - c` of the way to the target. Zero (or less) is instant.
pub fn smoothing_coefficient(samples: f64) -> f64 {
    if samples > 0.0 {
        (-1.0 / samples).exp()
    } else {
        0.0
    }
}

/// A second-order (biquad) filter section, transposed direct form II.
#[derive(Debug, Clone, Copy, Default)]
pub struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    /// Butterworth low or high pass (RBJ cookbook) at `frequency` cycles per sample.
    pub fn butterworth(frequency: f64, high_pass: bool) -> Self {
        let w = TAU * frequency.clamp(1e-6, 0.49);
        let alpha = w.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let cos = w.cos();
        let a0 = 1.0 + alpha;
        let (b0, b1) = if high_pass {
            ((1.0 + cos) / 2.0, -(1.0 + cos))
        } else {
            ((1.0 - cos) / 2.0, 1.0 - cos)
        };
        Self {
            b: [b0 / a0, b1 / a0, b0 / a0],
            a: [-2.0 * cos / a0, (1.0 - alpha) / a0],
            z: [0.0; 2],
        }
    }

    pub fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }

    pub fn reset(&mut self) {
        self.z = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_repeats_samples() {
        let mut dst = [0.0; 6];
        resample(&[1.0, 2.0, 3.0], &mut dst, 1, Interpolation::Hold);
        assert_eq!(dst, [1.0, 1.0, 2.0, 2.0, 3.0, 3.0]);
    }

    #[test]
    fn hold_keeps_pixels_together() {
        // Two mono samples over two RGB pixels: each pixel's R, G, B get the same value.
        let mut dst = [0.0; 6];
        resample(&[1.0, 2.0], &mut dst, 3, Interpolation::Hold);
        assert_eq!(dst, [1.0, 1.0, 1.0, 2.0, 2.0, 2.0]);
    }

    #[test]
    fn linear_ramps_and_clamps() {
        let mut dst = [0.0; 4];
        resample(&[0.0, 1.0], &mut dst, 1, Interpolation::Linear);
        assert_eq!(dst, [0.0, 0.25, 0.75, 1.0]);
    }

    #[test]
    fn downsampling_picks_centres() {
        let mut dst = [0.0; 2];
        resample(&[1.0, 2.0, 3.0, 4.0], &mut dst, 1, Interpolation::Hold);
        assert_eq!(dst, [2.0, 4.0]);
    }

    #[test]
    fn same_length_copies_and_empty_source_is_silence() {
        let mut dst = [9.0; 2];
        resample(&[1.0, 2.0], &mut dst, 1, Interpolation::Linear);
        assert_eq!(dst, [1.0, 2.0]);
        resample(&[], &mut dst, 1, Interpolation::Hold);
        assert_eq!(dst, [0.0, 0.0]);
    }

    #[test]
    fn decibels_convert_both_ways() {
        assert!((db_to_gain(-6.0) - 0.501).abs() < 1e-3);
        assert!((gain_to_db(db_to_gain(-18.0)) + 18.0).abs() < 1e-9);
        assert!(gain_to_db(0.0) < -200.0);
        assert_eq!(smoothing_coefficient(0.0), 0.0);
        assert!((smoothing_coefficient(1.0) - (-1f64).exp()).abs() < 1e-12);
    }

    #[test]
    fn delay_line_reads_whole_and_fractional_delays() {
        let mut line = DelayLine::new(4);
        for x in [1.0, 2.0, 3.0, 4.0] {
            line.push(x);
        }
        assert_eq!(line.read(0.0), 4.0);
        assert_eq!(line.read(3.0), 1.0);
        assert_eq!(line.read(0.5), 3.5);
        assert_eq!(line.read(10.0), line.read(4.0), "clamped to max delay");
        line.reset();
        assert_eq!(line.read(1.0), 0.0);
    }
}
