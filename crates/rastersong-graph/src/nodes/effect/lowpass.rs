use std::f64::consts::TAU;

use crate::nodes::support::UNBOUNDED_WARMUP;
use crate::nodes::{Category, FreqUnit, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// One-pole low pass filter. The cutoff is in cycles per row by default, so the blur looks the same
/// at any resolution; a signal modulating it moves it in octaves. The filter runs across rows and frames
/// like any audio filter, so its state carries from the end of one row to the next.
#[derive(Debug)]
pub struct Lowpass {
    cutoff: f64,
    unit: FreqUnit,
    /// Cutoff in cycles per sample, set in `prepare`.
    base: f64,
    /// Cycles per sample of one unit of cutoff, set in `prepare`.
    scale: f64,
    /// Whether the cutoff changes from sample to sample.
    modulated: bool,
    /// The lowest cutoff (cycles per sample) modulation can reach, for warmup.
    slowest: f64,
    /// Smoothing coefficient for the unmodulated cutoff.
    coefficient: f32,
    state: f32,
}

params! { Lowpass {
    CUTOFF: ParamSpec::number(
        "cutoff",
        "Cutoff",
        40.0,
        0.01,
        200.0,
        "Cutoff; lower is smoother",
    )
    .exposed()
    .limits(1e-06, 1e9),
    UNIT: FreqUnit::param("Row", "Unit for the cutoff"),
} }

impl NodeKind for Lowpass {
    const KIND: &'static str = "lowpass";
    const SPEC: NodeSpec = NodeSpec::new("Low Pass", Category::Effect)
        .describe("Smooths the signal along rows, a horizontal blur")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "cutoff": 0.7 }"#,
        r#"{ "cutoff": 1.5 }"#,
        r#"{ "cutoff": 3, "unit": "Beat" }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "cutoff": 40 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            cutoff: params.number_at(Self::CUTOFF)?,
            unit: params.choice_as(Self::UNIT)?,
            base: 0.0,
            scale: 1.0,
            modulated: false,
            slowest: 0.0,
            coefficient: 1.0,
            state: 0.0,
        })
    }
}

impl Lowpass {
    /// Smoothing coefficient for a cutoff in cycles per sample.
    fn coefficient(cycles_per_sample: f64) -> f32 {
        (1.0 - (-TAU * cycles_per_sample.min(0.5)).exp()) as f32
    }
}

impl Node for Lowpass {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.scale = self.unit.per_sample(1.0, ctx);
        self.base = self.cutoff * self.scale;
        self.modulated = ctx.modulation(Self::CUTOFF).is_some();
        self.coefficient = Self::coefficient(self.base);
        self.slowest = ctx.param_min(Self::CUTOFF, self.cutoff) * self.scale;
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = &inputs[0].data;
        let mut y = self.state;
        match ctx.param(Self::CUTOFF).filter(|_| self.modulated) {
            Some(cutoff) => {
                for ((out, &x), &c) in outputs[0].data.iter_mut().zip(input).zip(cutoff) {
                    let a = Self::coefficient(f64::from(c) * self.scale);
                    y += a * (x - y);
                    *out = y;
                }
            }
            None => {
                let a = self.coefficient;
                for (out, &x) in outputs[0].data.iter_mut().zip(input) {
                    y += a * (x - y);
                    *out = y;
                }
            }
        }
        self.state = y;
    }

    fn reset(&mut self) {
        self.state = 0.0;
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        // Time for the slowest possible cutoff to settle to within 0.1% (about 7 time constants).
        let slowest = self.slowest;
        if slowest <= 0.0 {
            return UNBOUNDED_WARMUP;
        }
        let settle_samples = 7.0 / (TAU * slowest);
        ((settle_samples / ctx.samples_per_frame() as f64).ceil() as u32)
            .max(1)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    /// A mono block of `len` samples that is one row, so cycles per row are cycles per block.
    fn lowpass(cutoff: f64, input: &[f32]) -> Vec<f32> {
        let mut node = node(
            "lowpass",
            &format!(r#"{{ "cutoff": {cutoff} }}"#),
            input.len(),
            input.len() as f64,
            &[true],
        );
        process_one(node.as_mut(), &[input.to_vec()])
    }

    #[test]
    fn a_step_rises_toward_one() {
        let out = lowpass(4.0, &[1.0; 64]);
        assert!(out[0] > 0.0 && out[0] < 1.0);
        assert!(out.windows(2).all(|w| w[1] >= w[0]), "monotonic");
        assert!((out[63] - 1.0).abs() < 1e-3, "settles at the input");
    }

    #[test]
    fn dc_passes_unchanged_once_settled() {
        let out = lowpass(10.0, &[0.25; 256]);
        assert!((out[255] - 0.25).abs() < 1e-5);
    }

    #[test]
    fn lower_cutoffs_rise_more_slowly() {
        let slow = lowpass(1.0, &[1.0; 64]);
        let fast = lowpass(8.0, &[1.0; 64]);
        assert!(slow[8] < fast[8]);
    }

    #[test]
    fn the_cutoff_matches_a_one_pole_filter() {
        // One cycle per 32-sample row: coefficient 1 - exp(-2π/32).
        let a = 1.0 - (-std::f32::consts::TAU / 32.0).exp();
        let out = lowpass(1.0, &[1.0; 32]);
        assert!((out[0] - a).abs() < 1e-6);
        assert!((out[1] - (a + a * (1.0 - a))).abs() < 1e-6);
    }
}
