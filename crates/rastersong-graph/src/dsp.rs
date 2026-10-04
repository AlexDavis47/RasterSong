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

/// Blends `dry` toward `wet`: `amount` 0 is all dry, 1 is all wet.
#[inline]
pub fn mix(dry: f32, wet: f32, amount: f32) -> f32 {
    dry + (wet - dry) * amount
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

/// The response a [`Biquad`] has (RBJ audio EQ cookbook).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BiquadKind {
    LowPass,
    HighPass,
    /// Constant 0 dB gain at the centre frequency.
    BandPass,
    /// Passes every frequency at full level and shifts phase around the centre frequency.
    AllPass,
    /// Boosts or cuts by `gain_db` around the centre frequency.
    Peak {
        gain_db: f64,
    },
    /// Boosts or cuts everything below the corner frequency by `gain_db`.
    LowShelf {
        gain_db: f64,
    },
    /// Boosts or cuts everything above the corner frequency by `gain_db`.
    HighShelf {
        gain_db: f64,
    },
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
        let kind = if high_pass {
            BiquadKind::HighPass
        } else {
            BiquadKind::LowPass
        };
        Self::design(kind, frequency, std::f64::consts::FRAC_1_SQRT_2)
    }

    /// A filter of the given `kind` at `frequency` cycles per sample, with quality factor `q`
    /// (the resonance: higher is narrower, and for low and high pass a peak at the cutoff).
    pub fn design(kind: BiquadKind, frequency: f64, q: f64) -> Self {
        let w = TAU * frequency.clamp(1e-6, 0.49);
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q.max(1e-3));
        let (b, a0, a1, a2) = match kind {
            BiquadKind::LowPass => (
                [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0],
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            BiquadKind::HighPass => (
                [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0],
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            BiquadKind::BandPass => ([alpha, 0.0, -alpha], 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            BiquadKind::AllPass => (
                [1.0 - alpha, -2.0 * cos, 1.0 + alpha],
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            BiquadKind::Peak { gain_db } => {
                let amp = 10f64.powf(gain_db / 40.0);
                (
                    [1.0 + alpha * amp, -2.0 * cos, 1.0 - alpha * amp],
                    1.0 + alpha / amp,
                    -2.0 * cos,
                    1.0 - alpha / amp,
                )
            }
            BiquadKind::LowShelf { gain_db } => {
                let amp = 10f64.powf(gain_db / 40.0);
                let beta = 2.0 * amp.sqrt() * alpha;
                (
                    [
                        amp * ((amp + 1.0) - (amp - 1.0) * cos + beta),
                        2.0 * amp * ((amp - 1.0) - (amp + 1.0) * cos),
                        amp * ((amp + 1.0) - (amp - 1.0) * cos - beta),
                    ],
                    (amp + 1.0) + (amp - 1.0) * cos + beta,
                    -2.0 * ((amp - 1.0) + (amp + 1.0) * cos),
                    (amp + 1.0) + (amp - 1.0) * cos - beta,
                )
            }
            BiquadKind::HighShelf { gain_db } => {
                let amp = 10f64.powf(gain_db / 40.0);
                let beta = 2.0 * amp.sqrt() * alpha;
                (
                    [
                        amp * ((amp + 1.0) + (amp - 1.0) * cos + beta),
                        -2.0 * amp * ((amp - 1.0) + (amp + 1.0) * cos),
                        amp * ((amp + 1.0) + (amp - 1.0) * cos - beta),
                    ],
                    (amp + 1.0) - (amp - 1.0) * cos + beta,
                    2.0 * ((amp - 1.0) - (amp + 1.0) * cos),
                    (amp + 1.0) - (amp - 1.0) * cos - beta,
                )
            }
        };
        Self {
            b: [b[0] / a0, b[1] / a0, b[2] / a0],
            a: [a1 / a0, a2 / a0],
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

    /// Steady-state gain of `filter` for a sine at `frequency` cycles per sample, from the RMS
    /// of the second half of the output (sampling the peak would miss it at some frequencies).
    fn gain_at(mut filter: Biquad, frequency: f64) -> f64 {
        let n = 20_000;
        let (mut sum, mut count) = (0.0, 0);
        for i in 0..n {
            let y = filter.process((TAU * frequency * i as f64).sin());
            if i > n / 2 {
                sum += y * y;
                count += 1;
            }
        }
        (2.0 * sum / f64::from(count)).sqrt()
    }

    #[test]
    fn low_and_high_pass_pass_their_own_side() {
        let low = Biquad::design(BiquadKind::LowPass, 0.05, 0.707);
        assert!((gain_at(low, 0.005) - 1.0).abs() < 0.01);
        assert!(gain_at(low, 0.4) < 0.01);
        let high = Biquad::design(BiquadKind::HighPass, 0.05, 0.707);
        assert!(gain_at(high, 0.005) < 0.01);
        assert!((gain_at(high, 0.4) - 1.0).abs() < 0.01);
        // The cutoff of a Butterworth filter is 3 dB down.
        assert!((gain_at(low, 0.05) - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.01);
    }

    #[test]
    fn resonance_peaks_at_the_cutoff() {
        let flat = Biquad::design(BiquadKind::LowPass, 0.05, 0.707);
        let resonant = Biquad::design(BiquadKind::LowPass, 0.05, 8.0);
        assert!(gain_at(resonant, 0.05) > 4.0 * gain_at(flat, 0.05));
    }

    #[test]
    fn band_pass_peaks_at_the_centre_with_unit_gain() {
        let band = Biquad::design(BiquadKind::BandPass, 0.1, 4.0);
        assert!((gain_at(band, 0.1) - 1.0).abs() < 0.01);
        assert!(gain_at(band, 0.01) < 0.1 && gain_at(band, 0.4) < 0.1);
    }

    #[test]
    fn all_pass_keeps_every_frequency_at_full_level() {
        let all = Biquad::design(BiquadKind::AllPass, 0.1, 1.0);
        for f in [0.01, 0.1, 0.3] {
            assert!((gain_at(all, f) - 1.0).abs() < 0.01, "{f}");
        }
    }

    #[test]
    fn peak_and_shelves_apply_their_gain_where_they_should() {
        let db = |g: f64| 10f64.powf(g / 20.0);
        let peak = Biquad::design(BiquadKind::Peak { gain_db: 9.0 }, 0.1, 2.0);
        assert!((gain_at(peak, 0.1) - db(9.0)).abs() < 0.02);
        assert!((gain_at(peak, 0.005) - 1.0).abs() < 0.02);
        let low = Biquad::design(BiquadKind::LowShelf { gain_db: -6.0 }, 0.05, 0.707);
        assert!((gain_at(low, 0.002) - db(-6.0)).abs() < 0.02);
        assert!((gain_at(low, 0.4) - 1.0).abs() < 0.02);
        let high = Biquad::design(BiquadKind::HighShelf { gain_db: 6.0 }, 0.05, 0.707);
        assert!((gain_at(high, 0.4) - db(6.0)).abs() < 0.03);
        assert!((gain_at(high, 0.002) - 1.0).abs() < 0.02);
    }

    #[test]
    fn butterworth_is_the_low_and_high_pass_design() {
        let reference = Biquad::design(BiquadKind::LowPass, 0.07, std::f64::consts::FRAC_1_SQRT_2);
        let b = Biquad::butterworth(0.07, false);
        assert_eq!((b.b, b.a), (reference.b, reference.a));
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
