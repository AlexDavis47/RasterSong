use std::f64::consts::TAU;

use crate::nodes::support::UNBOUNDED_WARMUP;
use crate::nodes::{Category, NodeKind, NodeSpec, Unit};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// Removes the constant (DC) offset of a signal with a very slow high pass. A one-pole blocker:
/// `y[n] = x[n] - x[n-1] + R * y[n-1]`, with `R` set by the cutoff.
#[derive(Debug)]
pub struct DcFilter {
    cutoff: f64,
    unit: Unit,
    /// Cycles per sample of one unit, set in `prepare`.
    per_sample: f64,
    /// The slowest cutoff in cycles per sample, for warmup.
    slowest: f64,
    previous_in: f64,
    previous_out: f64,
}

params! { DcFilter {
    CUTOFF: ParamSpec::number("cutoff", 10.0, 0.1, 100.0)
        .exposed()
        .limits(1e-06, 1e9),
    UNIT: Unit::freq_param("second"),
} }

impl NodeKind for DcFilter {
    const KIND: &'static str = "dc_filter";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "cutoff": 20, "unit": "second" }"#,
        r#"{ "cutoff": 0.5, "unit": "row" }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            cutoff: params.number_at(Self::CUTOFF)?,
            unit: params.choice_as(Self::UNIT)?,
            per_sample: 0.0,
            slowest: 0.0,
            previous_in: 0.0,
            previous_out: 0.0,
        })
    }
}

impl DcFilter {
    /// The feedback of the blocker for a cutoff in cycles per sample.
    fn feedback(cycles_per_sample: f64) -> f64 {
        (-TAU * cycles_per_sample.clamp(0.0, 0.5)).exp()
    }
}

impl Node for DcFilter {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.per_sample = self.unit.per_sample(1.0, ctx);
        self.slowest = ctx.param_min(Self::CUTOFF, self.cutoff) * self.per_sample;
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let cutoff = ctx.value(Self::CUTOFF, self.cutoff);
        let constant = match cutoff {
            crate::Value::Const(c) => Some(Self::feedback(c * self.per_sample)),
            crate::Value::Stream(_) => None,
        };
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let r = constant.unwrap_or_else(|| Self::feedback(cutoff.at64(i) * self.per_sample));
            let x = f64::from(x);
            let y = x - self.previous_in + r * self.previous_out;
            self.previous_in = x;
            self.previous_out = y;
            *out = y as f32;
        }
    }

    fn reset(&mut self) {
        self.previous_in = 0.0;
        self.previous_out = 0.0;
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        // About seven time constants of 1 / (2π f) to settle within 0.1%.
        let samples = 7.0 / (TAU * self.slowest.max(1e-9));
        if samples.is_finite() {
            ((samples / ctx.samples_per_frame() as f64).ceil() as u32).max(1)
        } else {
            UNBOUNDED_WARMUP
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn run(params: &str, input: Vec<f32>) -> Vec<f32> {
        let len = input.len();
        let mut node = node("dc_filter", params, len, 48_000.0, &[true]);
        for _ in 0..5 {
            process_one(node.as_mut(), std::slice::from_ref(&input));
        }
        process_one(node.as_mut(), &[input])
    }

    #[test]
    fn a_constant_offset_is_removed() {
        let out = run(r#"{ "cutoff": 20 }"#, vec![0.5; 9600]);
        assert!(out.iter().all(|x| x.abs() < 0.01), "{:?}", &out[..4]);
    }

    #[test]
    fn a_tone_well_above_the_cutoff_passes_but_its_offset_goes() {
        let len = 9600;
        let input: Vec<f32> = (0..len)
            .map(|i| 0.3 + (std::f32::consts::TAU * 1000.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect();
        let out = run(r#"{ "cutoff": 20 }"#, input);
        let mean = out.iter().sum::<f32>() / len as f32;
        let peak = out.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(mean.abs() < 0.01, "{mean}");
        assert!((peak - 0.5).abs() < 0.05, "{peak}");
    }
}
