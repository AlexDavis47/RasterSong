use std::f64::consts::TAU;

use super::MAX_WARMUP_FRAMES;
use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

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
    pub const SPEC: NodeSpec = NodeSpec::new("Low Pass", Category::Effect)
        .describe("Smooths the signal along rows, a horizontal blur")
        .params(Self::PARAMS)
        .per_channel();

    pub const PARAMS: &[ParamSpec] = &[
        ParamSpec::number(
            "cutoff",
            "Cutoff",
            40.0,
            0.01,
            100_000.0,
            "Cutoff in cycles per row; lower is smoother",
        )
        .unit("cycles/row")
        .limits(1e-06, 1e9),
        ParamSpec::number(
            "depth",
            "Depth",
            0.0,
            -16.0,
            16.0,
            "Octaves the cutoff moves per unit of the modulation input",
        )
        .limits(-64.0, 64.0),
    ];

    pub fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            cutoff: params.number("cutoff")?,
            depth: params.number("depth")?,
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
