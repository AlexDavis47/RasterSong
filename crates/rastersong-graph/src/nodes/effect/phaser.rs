use std::f64::consts::{PI, TAU};

use crate::dsp::mix;
use crate::nodes::support::UNBOUNDED_WARMUP;
use crate::nodes::{Category, FreqUnit, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// Most allpass stages a phaser can chain.
const MAX_STAGES: usize = 12;

/// Phaser: the signal through a chain of allpass filters, mixed with the dry signal, which puts
/// moving notches in the spectrum. It has no oscillator of its own: wire an Oscillator into
/// `freq` to sweep them.
#[derive(Debug)]
pub struct Phaser {
    stages: usize,
    freq: f64,
    unit: FreqUnit,
    feedback: f32,
    mix: f32,
    /// Cycles per sample of one unit of frequency, set in `prepare`.
    scale: f64,
    /// The lowest frequency (cycles per sample) the sweep reaches, for warmup.
    slowest: f64,
    /// Each stage's state.
    state: [f32; MAX_STAGES],
    /// The last output of the chain, fed back into its input.
    last: f32,
}

params! { Phaser {
    STAGES: ParamSpec::number(
        "stages",
        "Stages",
        4.0,
        1.0,
        12.0,
        "How many allpass filters are chained; every two add a notch",
    )
    .fixed(),
    FREQ: ParamSpec::number(
        "freq",
        "Frequency",
        1000.0,
        20.0,
        20_000.0,
        "Where the notches sit; wire an oscillator in here to sweep them",
    )
    .exposed()
    .limits(1e-06, 1e9)
    .octaves(),
    UNIT: FreqUnit::param("Hertz", "Unit for the frequency"),
    FEEDBACK: ParamSpec::number(
        "feedback",
        "Feedback",
        0.3,
        -0.95,
        0.95,
        "How much of the chain's output is fed back in, which sharpens the notches",
    ),
    MIX: ParamSpec::number(
        "mix",
        "Mix",
        0.5,
        0.0,
        1.0,
        "0 is the dry input, 1 is only the phased signal; around 0.5 gives the deepest notches",
    ),
} }

impl NodeKind for Phaser {
    const KIND: &'static str = "phaser";
    const SPEC: NodeSpec = NodeSpec::new("Phaser", Category::Effect)
        .describe("Sweeps notches through the signal with allpass filters; modulate the frequency")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "stages": 2, "freq": 3, "unit": "Row" }"#,
        r#"{ "stages": 8, "freq": 400, "feedback": -0.8, "mix": 0.7 }"#,
        r#"{ "stages": 12, "freq": 20000, "feedback": 0.95 }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "freq": 1000 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            stages: (params.number_at(Self::STAGES)?.round() as usize).clamp(1, MAX_STAGES),
            freq: params.number_at(Self::FREQ)?,
            unit: params.choice_as(Self::UNIT)?,
            feedback: params.float_at(Self::FEEDBACK)?,
            mix: params.float_at(Self::MIX)?,
            scale: 1.0,
            slowest: 0.0,
            state: [0.0; MAX_STAGES],
            last: 0.0,
        })
    }
}

impl Phaser {
    /// The coefficient of a first-order allpass whose phase is 90° at `cycles_per_sample`.
    fn coefficient(cycles_per_sample: f64) -> f32 {
        let t = (PI * cycles_per_sample.clamp(1e-6, 0.49)).tan();
        ((t - 1.0) / (t + 1.0)) as f32
    }
}

impl Node for Phaser {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.scale = self.unit.per_sample(1.0, ctx);
        self.slowest = ctx.param_min(Self::FREQ, self.freq) * self.scale;
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let freq = ctx.value(Self::FREQ, self.freq);
        let feedback = ctx.value(Self::FEEDBACK, f64::from(self.feedback));
        let amount = ctx.value(Self::MIX, f64::from(self.mix));
        // Without modulation the coefficient is the same for every sample.
        let fixed = ctx
            .param(Self::FREQ)
            .is_none()
            .then(|| Self::coefficient(self.freq * self.scale));
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let a = fixed.unwrap_or_else(|| Self::coefficient(freq.at64(i) * self.scale));
            let mut v = x + feedback.at(i) * self.last;
            for z in &mut self.state[..self.stages] {
                let y = a * v + *z;
                *z = v - a * y;
                v = y;
            }
            self.last = v;
            *out = mix(x, v, amount.at(i));
        }
    }

    fn reset(&mut self) {
        self.state = [0.0; MAX_STAGES];
        self.last = 0.0;
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        // The chain settles about as slowly as a one-pole low pass at the lowest frequency.
        if self.slowest <= 0.0 {
            return UNBOUNDED_WARMUP;
        }
        let settle_samples = 7.0 / (TAU * self.slowest);
        ((settle_samples / ctx.samples_per_frame() as f64).ceil() as u32)
            .max(1)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn run(params: &str, input: &[f32]) -> Vec<f32> {
        let mut n = node("phaser", params, input.len(), input.len() as f64, &[true]);
        process_one(n.as_mut(), &[input.to_vec()])
    }

    #[test]
    fn mix_zero_is_the_dry_signal() {
        let input = [0.2, -0.4, 0.9, 0.1];
        assert_eq!(run(r#"{ "mix": 0 }"#, &input), input);
    }

    #[test]
    fn the_wet_path_keeps_the_energy() {
        // Allpass filters change phase, not level: a long sine keeps its amplitude.
        let input: Vec<f32> = (0..4000)
            .map(|n| (std::f64::consts::TAU * 0.02 * f64::from(n)).sin() as f32)
            .collect();
        let out = run(
            r#"{ "mix": 1, "feedback": 0, "freq": 0.05, "unit": "Row" }"#,
            &input,
        );
        let peak = out[2000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!((peak - 1.0).abs() < 0.02, "peak {peak}");
    }
}
