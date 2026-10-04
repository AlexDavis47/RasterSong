//! Effect nodes. They see flat sample streams and never look at layout, except to convert
//! user-facing units (rows, frames, cycles per row) to samples.

use std::f64::consts::TAU;

use crate::desc::Params;
use crate::dsp::DelayLine;
use crate::{InputSpec, Node, PrepareContext, ProcessContext, Signal};

/// Maximum warmup a node with infinite memory reports, in frames.
const MAX_WARMUP_FRAMES: u32 = 120;

/// A length unit for user-facing parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    Rows,
    Frames,
}

impl LengthUnit {
    fn read(params: &mut Params) -> Result<Self, String> {
        Ok(match params.choice("unit", &["rows", "frames"], "rows")? {
            "rows" => Self::Rows,
            _ => Self::Frames,
        })
    }

    fn samples(self, ctx: &PrepareContext) -> f64 {
        match self {
            Self::Rows => ctx.samples_per_row() as f64,
            Self::Frames => ctx.samples_per_frame() as f64,
        }
    }
}

/// Amplitude modulation: `out = carrier × (1 + depth × modulator)`.
#[derive(Debug)]
pub struct Am {
    depth: f32,
}

impl Am {
    pub fn new(params: &mut Params) -> Result<Self, String> {
        Ok(Self {
            depth: params.number("depth", 1.0)? as f32,
        })
    }
}

impl Node for Am {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[
            InputSpec::required("carrier"),
            InputSpec::required("modulator"),
        ];
        INPUTS
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let (carrier, modulator) = (&inputs[0].data, &inputs[1].data);
        for ((out, &c), &m) in outputs[0].data.iter_mut().zip(carrier).zip(modulator) {
            *out = c * (1.0 + self.depth * m);
        }
    }
}

/// A delay line whose time can be modulated: `time + depth × modulation`, in rows or frames.
/// Delaying by a fraction of a row and modulating it with a bass line bends rows into waves.
#[derive(Debug)]
pub struct Delay {
    time: f64,
    depth: f64,
    unit: LengthUnit,
    feedback: f32,
    mix: f32,
    /// Samples per unit, set in `prepare`.
    unit_samples: f64,
    line: DelayLine,
}

impl Delay {
    pub fn new(params: &mut Params) -> Result<Self, String> {
        Ok(Self {
            time: params.number_in("time", 1.0, 0.0, 10_000.0)?,
            depth: params.number_in("depth", 0.0, -10_000.0, 10_000.0)?,
            unit: LengthUnit::read(params)?,
            feedback: params.number_in("feedback", 0.0, 0.0, 0.99)? as f32,
            mix: params.number_in("mix", 1.0, 0.0, 1.0)? as f32,
            unit_samples: 0.0,
            line: DelayLine::default(),
        })
    }

    fn max_delay_samples(&self) -> f64 {
        (self.time + self.depth.abs()) * self.unit_samples
    }
}

impl Node for Delay {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] =
            &[InputSpec::required("in"), InputSpec::optional("modulation")];
        INPUTS
    }

    fn prepare(&mut self, ctx: &PrepareContext) {
        self.unit_samples = self.unit.samples(ctx);
        self.line = DelayLine::new(self.max_delay_samples().ceil() as usize + 1);
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let (input, modulation) = (&inputs[0].data, &inputs[1].data);
        // With feedback, the newest sample can't be read before it is written, so the delay is at
        // least one sample.
        let min_delay = if self.feedback > 0.0 { 1.0 } else { 0.0 };
        for ((out, &x), &m) in outputs[0].data.iter_mut().zip(input).zip(modulation) {
            let delay =
                ((self.time + self.depth * f64::from(m)) * self.unit_samples).max(min_delay);
            let delayed = if self.feedback > 0.0 {
                let delayed = self.line.read(delay - 1.0);
                self.line.push(x + self.feedback * delayed);
                delayed
            } else {
                self.line.push(x);
                self.line.read(delay)
            };
            *out = x + (delayed - x) * self.mix;
        }
    }

    fn reset(&mut self) {
        self.line.reset();
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        let delay_frames = self.max_delay_samples() / ctx.samples_per_frame() as f64;
        // Feedback repeats every delay period; count periods until it has decayed by 60 dB.
        let repeats = if self.feedback > 0.0 {
            (0.001f64.ln() / f64::from(self.feedback).ln()).ceil()
        } else {
            1.0
        };
        ((delay_frames * repeats).ceil() as u32).min(MAX_WARMUP_FRAMES)
    }
}

/// Reduces bit depth: quantizes to `2^bits` levels over `0..=1`. Fractional and modulated
/// bit depths are allowed.
#[derive(Debug)]
pub struct Bitcrush {
    bits: f32,
    depth: f32,
}

impl Bitcrush {
    pub fn new(params: &mut Params) -> Result<Self, String> {
        Ok(Self {
            bits: params.number_in("bits", 4.0, 1.0, 24.0)? as f32,
            depth: params.number_in("depth", 0.0, -24.0, 24.0)? as f32,
        })
    }
}

impl Node for Bitcrush {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] =
            &[InputSpec::required("in"), InputSpec::optional("modulation")];
        INPUTS
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let (input, modulation) = (&inputs[0].data, &inputs[1].data);
        for ((out, &x), &m) in outputs[0].data.iter_mut().zip(input).zip(modulation) {
            let bits = (self.bits + self.depth * m).clamp(1.0, 24.0);
            let steps = bits.exp2() - 1.0;
            *out = (x * steps).round() / steps;
        }
    }
}

/// One-pole low pass filter. The cutoff is in cycles per row, so the blur looks the same at any
/// resolution; modulation shifts it by `depth` octaves per unit. The filter runs across rows and
/// frames like any audio filter, so its state carries from the end of one row to the next.
#[derive(Debug)]
pub struct Lowpass {
    cutoff: f64,
    depth: f64,
    /// Cutoff in cycles per sample, set in `prepare`.
    base: f64,
    modulated: bool,
    /// Smoothing coefficient for the unmodulated cutoff.
    coefficient: f32,
    state: f32,
}

impl Lowpass {
    pub fn new(params: &mut Params) -> Result<Self, String> {
        Ok(Self {
            cutoff: params.number_in("cutoff", 40.0, 0.0, 1e9)?,
            depth: params.number_in("depth", 0.0, -16.0, 16.0)?,
            base: 0.0,
            modulated: false,
            coefficient: 1.0,
            state: 0.0,
        })
    }

    /// Smoothing coefficient for a cutoff in cycles per sample.
    fn coefficient(cycles_per_sample: f64) -> f32 {
        (1.0 - (-TAU * cycles_per_sample.min(0.5)).exp()) as f32
    }
}

impl Node for Lowpass {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] =
            &[InputSpec::required("in"), InputSpec::optional("modulation")];
        INPUTS
    }

    fn prepare(&mut self, ctx: &PrepareContext) {
        self.base = self.cutoff / ctx.samples_per_row().max(1) as f64;
        self.modulated = ctx.connected[1] && self.depth != 0.0;
        self.coefficient = Self::coefficient(self.base);
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let (input, modulation) = (&inputs[0].data, &inputs[1].data);
        let mut y = self.state;
        if self.modulated {
            for ((out, &x), &m) in outputs[0].data.iter_mut().zip(input).zip(modulation) {
                let a = Self::coefficient(self.base * (self.depth * f64::from(m)).exp2());
                y += a * (x - y);
                *out = y;
            }
        } else {
            let a = self.coefficient;
            for (out, &x) in outputs[0].data.iter_mut().zip(input) {
                y += a * (x - y);
                *out = y;
            }
        }
        self.state = y;
    }

    fn reset(&mut self) {
        self.state = 0.0;
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        // Time for the slowest possible cutoff to settle to within 0.1% (about 7 time constants).
        let slowest = self.base * (-self.depth.abs()).exp2();
        if slowest <= 0.0 {
            return MAX_WARMUP_FRAMES;
        }
        let settle_samples = 7.0 / (TAU * slowest);
        ((settle_samples / ctx.samples_per_frame() as f64).ceil() as u32)
            .clamp(1, MAX_WARMUP_FRAMES)
    }
}

/// A second-order (biquad) filter section, transposed direct form II.
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    /// Butterworth low or high pass (RBJ cookbook) at `frequency` cycles per sample.
    fn butterworth(frequency: f64, high_pass: bool) -> Self {
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

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }

    fn reset(&mut self) {
        self.z = [0.0; 2];
    }
}

/// Splits a signal into low, mid and high bands. Crossovers are in Hz of the input signal's own
/// sample rate (for an audio input, ordinary Hz). Mid is what remains after removing low and high,
/// so the three bands always add back up to the input.
#[derive(Debug)]
pub struct ThreeBand {
    low_hz: f64,
    high_hz: f64,
    low: Biquad,
    high: Biquad,
}

impl ThreeBand {
    pub fn new(params: &mut Params) -> Result<Self, String> {
        let low_hz = params.number_in("low_hz", 250.0, 1.0, 1e6)?;
        let high_hz = params.number_in("high_hz", 4000.0, 1.0, 1e6)?;
        if low_hz >= high_hz {
            return Err(format!(
                "`low_hz` ({low_hz}) must be below `high_hz` ({high_hz})"
            ));
        }
        Ok(Self {
            low_hz,
            high_hz,
            low: Biquad::default(),
            high: Biquad::default(),
        })
    }
}

impl Node for ThreeBand {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn outputs(&self) -> &'static [&'static str] {
        &["low", "mid", "high"]
    }

    fn prepare(&mut self, ctx: &PrepareContext) {
        let rate = ctx.sample_rate();
        self.low = Biquad::butterworth(self.low_hz / rate, false);
        self.high = Biquad::butterworth(self.high_hz / rate, true);
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let [low, mid, high] = outputs else {
            unreachable!()
        };
        for (i, &x) in inputs[0].data.iter().enumerate() {
            let x = f64::from(x);
            let l = self.low.process(x);
            let h = self.high.process(x);
            low.data[i] = l as f32;
            high.data[i] = h as f32;
            mid.data[i] = (x - l - h) as f32;
        }
    }

    fn reset(&mut self) {
        self.low.reset();
        self.high.reset();
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        // The low band settles slowest: allow ten periods of the low crossover.
        let settle_samples = 10.0 * ctx.sample_rate() / self.low_hz;
        ((settle_samples / ctx.samples_per_frame() as f64).ceil() as u32)
            .clamp(1, MAX_WARMUP_FRAMES)
    }
}
