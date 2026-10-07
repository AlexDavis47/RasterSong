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

/// Stretches a signal of any layout over a picture: the one implementation of "show this signal
/// at the project's size", shared by the Video Output and anything else that has to draw an
/// arbitrary signal.
///
/// A signal with the same number of samples per pixel as the picture is stretched channel by
/// channel. Any other signal is read as a flat run of samples, stretched over the pixels and
/// repeated in every channel (gray).
#[derive(Debug, Default)]
pub struct Stretcher {
    channel: Vec<f32>,
    stretched: Vec<f32>,
}

impl Stretcher {
    /// Fills `dst` (`dst_spp` samples per pixel) from `src` (`src_spp` samples per pixel).
    pub fn stretch(
        &mut self,
        src: &[f32],
        src_spp: usize,
        dst: &mut [f32],
        dst_spp: usize,
        mode: Interpolation,
    ) {
        let pixels = dst.len() / dst_spp.max(1);
        self.stretched.resize(pixels, 0.0);
        let per_channel = src_spp == dst_spp && src_spp > 0;
        let channels = if per_channel { dst_spp } else { 1 };
        for c in 0..channels {
            let source: &[f32] = if per_channel {
                self.channel.clear();
                self.channel
                    .extend(src.iter().skip(c).step_by(src_spp.max(1)));
                &self.channel
            } else {
                src
            };
            resample(source, &mut self.stretched, 1, mode);
            if per_channel {
                for (p, &v) in self.stretched.iter().enumerate() {
                    dst[p * dst_spp + c] = v;
                }
            } else {
                for (pixel, &v) in dst.chunks_mut(dst_spp.max(1)).zip(&self.stretched) {
                    pixel.fill(v);
                }
            }
        }
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

/// The attack and release of a one-pole follower (Envelope, Gate, Compressor, Limiter): the
/// smoothing coefficients for the constant times, and for times a signal modulates per sample.
/// Times are in a unit of `unit_samples` samples; which side counts as attack is up to the node.
#[derive(Debug, Clone, Copy)]
pub struct AttackRelease {
    unit_samples: f64,
    attack: f64,
    release: f64,
}

impl Default for AttackRelease {
    fn default() -> Self {
        Self::new(0.0, 0.0, 1.0)
    }
}

impl AttackRelease {
    pub fn new(attack_time: f64, release_time: f64, unit_samples: f64) -> Self {
        Self {
            unit_samples,
            attack: smoothing_coefficient(attack_time * unit_samples),
            release: smoothing_coefficient(release_time * unit_samples),
        }
    }

    /// The coefficient of a time, in units (negative counts as zero).
    pub fn coefficient(&self, time: f64) -> f64 {
        smoothing_coefficient(time.max(0.0) * self.unit_samples)
    }

    /// The attack coefficient at sample `i`: from the stream when the time is modulated.
    pub fn attack(&self, stream: Option<&[f32]>, i: usize) -> f64 {
        stream.map_or(self.attack, |s| self.coefficient(f64::from(s[i])))
    }

    /// The release coefficient at sample `i`: from the stream when the time is modulated.
    pub fn release(&self, stream: Option<&[f32]>, i: usize) -> f64 {
        stream.map_or(self.release, |s| self.coefficient(f64::from(s[i])))
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
    /// Removes a narrow band around the centre frequency and passes the rest.
    Notch,
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

/// The most Butterworth stages a cascade has (a 48 dB/oct slope).
pub const MAX_STAGES: usize = 4;

/// The quality factor of stage `index` (0-based) of a Butterworth low or high pass of `stages`
/// second-order stages. The last stage has the highest and carries any resonance.
pub fn butterworth_q(stages: usize, index: usize) -> f64 {
    let order = (stages * 2) as f64;
    1.0 / (2.0 * ((2 * index + 1) as f64 * std::f64::consts::PI / (2.0 * order)).cos())
}

/// The sections of a Butterworth low or high pass of `stages` stages (up to [`MAX_STAGES`]) at
/// `frequency` cycles per sample, the rest left default (unused). `q` scales the last, sharpest
/// stage so the flat default of 0.707 is flat at every slope and higher values peak the cutoff.
pub fn butterworth_cascade(
    high_pass: bool,
    stages: usize,
    frequency: f64,
    q: f64,
) -> [Biquad; MAX_STAGES] {
    let kind = if high_pass {
        BiquadKind::HighPass
    } else {
        BiquadKind::LowPass
    };
    let mut sections = [Biquad::default(); MAX_STAGES];
    for (i, section) in sections.iter_mut().take(stages).enumerate() {
        let resonance = if i + 1 == stages {
            q / std::f64::consts::FRAC_1_SQRT_2
        } else {
            1.0
        };
        *section = Biquad::design(kind, frequency, butterworth_q(stages, i) * resonance);
    }
    sections
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
            BiquadKind::Notch => ([1.0, -2.0 * cos, 1.0], 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
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

    /// Switches to `other`'s response, keeping this filter's state so the output doesn't jump.
    /// For filters whose frequency or gain changes while they run.
    pub fn retune(&mut self, other: Self) {
        self.b = other.b;
        self.a = other.a;
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

/// Taps in the Hilbert transformer. Odd, so the centre falls on a whole sample.
pub const HILBERT_TAPS: usize = 65;

/// A Hilbert transformer: for each input sample, the input delayed by [`Hilbert::LATENCY`]
/// samples and its 90° phase-shifted copy, which together form the analytic signal that a
/// frequency shifter rotates. A windowed FIR, so it is exact for frequencies well inside
/// `0..Nyquist` and rolls off near DC and Nyquist.
#[derive(Debug, Clone)]
pub struct Hilbert {
    taps: Vec<f32>,
    history: Vec<f32>,
    /// Index the next sample will be written to.
    write: usize,
}

impl Default for Hilbert {
    fn default() -> Self {
        let m = HILBERT_TAPS / 2;
        let taps = (0..HILBERT_TAPS)
            .map(|k| {
                let j = k as f64 - m as f64;
                if (k + m).is_multiple_of(2) {
                    // Even offsets from the centre are zero in an ideal Hilbert transformer.
                    0.0
                } else {
                    // Blackman window.
                    let w = 0.42 - 0.5 * (TAU * k as f64 / (HILBERT_TAPS - 1) as f64).cos()
                        + 0.08 * (2.0 * TAU * k as f64 / (HILBERT_TAPS - 1) as f64).cos();
                    (2.0 / (std::f64::consts::PI * j) * w) as f32
                }
            })
            .collect();
        Self {
            taps,
            history: vec![0.0; HILBERT_TAPS],
            write: 0,
        }
    }
}

impl Hilbert {
    /// Samples by which both outputs lag the input.
    pub const LATENCY: usize = HILBERT_TAPS / 2;

    /// Takes the next input sample; returns `(real, imaginary)`: the input from
    /// [`Self::LATENCY`] samples ago and its quadrature.
    pub fn push(&mut self, x: f32) -> (f32, f32) {
        let n = self.history.len();
        self.history[self.write] = x;
        self.write = (self.write + 1) % n;
        // Tap k multiplies the sample pushed k samples ago.
        let newest = self.write + n - 1;
        let mut imag = 0.0;
        for k in (0..HILBERT_TAPS).filter(|k| (k + Self::LATENCY) % 2 == 1) {
            imag += self.taps[k] * self.history[(newest - k) % n];
        }
        let real = self.history[(newest - Self::LATENCY) % n];
        (real, imag)
    }

    pub fn reset(&mut self) {
        self.history.fill(0.0);
        self.write = 0;
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
    fn hilbert_turns_a_cosine_into_a_sine() {
        let mut h = Hilbert::default();
        let freq = 0.05;
        let mut worst = 0.0f32;
        for n in 0..400 {
            let x = (TAU * freq * n as f64).cos() as f32;
            let (real, imag) = h.push(x);
            if n > 2 * HILBERT_TAPS {
                let t = n as f64 - Hilbert::LATENCY as f64;
                let want_real = (TAU * freq * t).cos() as f32;
                let want_imag = (TAU * freq * t).sin() as f32;
                worst = worst
                    .max((real - want_real).abs())
                    .max((imag - want_imag).abs());
            }
        }
        assert!(worst < 0.02, "worst error {worst}");
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

/// A radix-2 fast Fourier transform of a fixed power-of-two size, for analysis (the spectrum
/// analyzer) and for any node that works on spectra.
#[derive(Debug, Clone)]
pub struct Fft {
    size: usize,
    /// `cos` and `sin` of `-2πk/size` for `k < size/2`.
    twiddles: Vec<(f32, f32)>,
    window: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
}

impl Fft {
    /// A transform of `size` points, rounded up to a power of two (at least 2).
    pub fn new(size: usize) -> Self {
        let size = size.max(2).next_power_of_two();
        let twiddles = (0..size / 2)
            .map(|k| {
                let a = -TAU * k as f64 / size as f64;
                (a.cos() as f32, a.sin() as f32)
            })
            .collect();
        // Periodic Hann: the usual window for spectrum display.
        let window = (0..size)
            .map(|i| (0.5 - 0.5 * (TAU * i as f64 / size as f64).cos()) as f32)
            .collect();
        Self {
            size,
            twiddles,
            window,
            re: vec![0.0; size],
            im: vec![0.0; size],
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// In-place transform of `re` and `im`, which must be `size` long.
    fn transform(&mut self) {
        let n = self.size;
        let bits = n.trailing_zeros();
        for i in 0..n {
            let j = i.reverse_bits() >> (usize::BITS - bits);
            if j > i {
                self.re.swap(i, j);
                self.im.swap(i, j);
            }
        }
        let mut half = 1;
        while half < n {
            let step = n / (half * 2);
            for start in (0..n).step_by(half * 2) {
                for k in 0..half {
                    let (c, s) = self.twiddles[k * step];
                    let (a, b) = (start + k, start + k + half);
                    let tr = self.re[b] * c - self.im[b] * s;
                    let ti = self.re[b] * s + self.im[b] * c;
                    self.re[b] = self.re[a] - tr;
                    self.im[b] = self.im[a] - ti;
                    self.re[a] += tr;
                    self.im[a] += ti;
                }
            }
            half *= 2;
        }
    }

    /// The magnitude of each frequency bin from 0 to the Nyquist frequency (`size / 2 + 1` bins)
    /// of the Hann-windowed `samples`, scaled so a full-scale sine reads about `1`. Input
    /// shorter than the transform is zero-padded; longer input is cut to its first `size`
    /// samples.
    pub fn magnitudes(&mut self, samples: &[f32], out: &mut Vec<f32>) {
        for (i, (re, im)) in self.re.iter_mut().zip(&mut self.im).enumerate() {
            *re = samples.get(i).copied().unwrap_or(0.0) * self.window[i];
            *im = 0.0;
        }
        self.transform();
        // A Hann window has a coherent gain of one half; a real sine splits across two bins.
        let scale = 4.0 / self.size as f32;
        out.clear();
        out.extend(
            (0..=self.size / 2).map(|k| (self.re[k].hypot(self.im[k]) * scale).min(f32::MAX)),
        );
    }
}

#[cfg(test)]
mod fft_tests {
    use super::*;

    #[test]
    fn a_sine_peaks_in_its_bin_at_its_level() {
        let size = 256;
        let mut fft = Fft::new(size);
        let sine: Vec<f32> = (0..size)
            .map(|i| (0.5 * (TAU * 20.0 * i as f64 / size as f64).sin()) as f32)
            .collect();
        let mut out = Vec::new();
        fft.magnitudes(&sine, &mut out);
        assert_eq!(out.len(), size / 2 + 1);
        let peak = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap();
        assert_eq!(peak.0, 20);
        assert!((*peak.1 - 0.5).abs() < 0.02, "{}", peak.1);
        // Far from the tone there is almost nothing.
        assert!(out[100] < 0.001);
    }

    #[test]
    fn size_rounds_up_and_short_input_is_padded() {
        let mut fft = Fft::new(100);
        assert_eq!(fft.size(), 128);
        let mut out = Vec::new();
        fft.magnitudes(&[1.0; 10], &mut out);
        assert_eq!(out.len(), 65);
        fft.magnitudes(&[], &mut out);
        assert!(out.iter().all(|&m| m == 0.0));
    }
}
