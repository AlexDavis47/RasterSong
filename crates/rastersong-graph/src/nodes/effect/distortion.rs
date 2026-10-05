use crate::dsp::{db_to_gain, mix};
use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, ProcessContext, Signal};

choice! {
    /// How the driven signal is bent back into `-1..=1`.
    pub enum Shape {
        /// `tanh`: rounds off smoothly.
        Soft = "soft",
        /// Clips flat at ±1.
        Hard = "hard",
        /// Reflects back from ±1, folding loud parts over.
        Fold = "fold",
        /// Wraps around from +1 to -1, like integer overflow.
        Wrap = "wrap",
    }
}

impl Shape {
    pub fn apply(self, v: f32) -> f32 {
        match self {
            Self::Soft => v.tanh(),
            Self::Hard => v.clamp(-1.0, 1.0),
            // A triangle wave of v: 0 → 0, 1 → 1, 2 → 0, 3 → -1, 4 → 0, …
            Self::Fold => ((v - 1.0).rem_euclid(4.0) - 2.0).abs() - 1.0,
            Self::Wrap => (v + 1.0).rem_euclid(2.0) - 1.0,
        }
    }
}

/// Waveshaping distortion: `shape(drive × input + bias)`, blended with the dry input.
/// Stateless, so it needs no warmup.
#[derive(Debug)]
pub struct Distortion {
    shape: Shape,
    /// Drive as a gain, converted from dB.
    drive: f32,
    bias: f32,
    mix: f32,
}

params! { Distortion {
    SHAPE: ParamSpec::choice(
        "shape",
        "Shape",
        Shape::OPTIONS,
        "soft",
        "soft rounds off, hard clips flat, fold reflects loud parts back, wrap jumps from top to bottom",
    ),
    DRIVE: ParamSpec::number(
        "drive",
        "Drive",
        12.0,
        0.0,
        48.0,
        "Gain before shaping; more drive, more distortion",
    )
    .unit("dB")
    .exposed()
    .limits(-96.0, 96.0),
    BIAS: ParamSpec::number(
        "bias",
        "Bias",
        0.0,
        -1.0,
        1.0,
        "Offset added before shaping, for uneven distortion",
    )
    .limits(-100.0, 100.0),
    MIX: ParamSpec::number(
        "mix",
        "Mix",
        1.0,
        0.0,
        1.0,
        "0 is the dry input, 1 is only the distorted signal",
    ),
} }

impl NodeKind for Distortion {
    const KIND: &'static str = "distortion";
    const SPEC: NodeSpec = NodeSpec::new("Distortion", Category::Effect)
        .describe("Drives the signal into a waveshaper: soft, hard, folding or wrapping")
        .params(Self::PARAMS)
        .per_channel()
        .expects(crate::Range::Bipolar);
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "shape": "soft", "drive": 18 }"#,
        r#"{ "shape": "hard", "drive": 30, "bias": -0.2 }"#,
        r#"{ "shape": "fold", "drive": 24, "bias": 0.3, "mix": 0.6 }"#,
        r#"{ "shape": "wrap", "drive": 12 }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            shape: params.choice_as(Self::SHAPE)?,
            drive: db_to_gain(params.number_at(Self::DRIVE)?) as f32,
            bias: params.float_at(Self::BIAS)?,
            mix: params.float_at(Self::MIX)?,
        })
    }
}

impl Node for Distortion {
    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let drive = ctx.value(Self::DRIVE, f64::from(self.drive));
        let bias = ctx.value(Self::BIAS, f64::from(self.bias));
        let amount = ctx.value(Self::MIX, f64::from(self.mix));
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let drive = drive.at_with(i, |db| db_to_gain(f64::from(db)) as f32);
            let wet = self.shape.apply(drive * x + bias.at(i));
            *out = mix(x, wet, amount.at(i));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_bend_into_range() {
        assert_eq!(Shape::Hard.apply(3.0), 1.0);
        assert_eq!(Shape::Hard.apply(-0.5), -0.5);
        assert!((Shape::Soft.apply(10.0) - 1.0).abs() < 1e-6);
        for (v, folded) in [
            (0.0, 0.0),
            (0.5, 0.5),
            (1.0, 1.0),
            (1.5, 0.5),
            (2.0, 0.0),
            (3.0, -1.0),
            (-1.5, -0.5),
        ] {
            assert!((Shape::Fold.apply(v) - folded).abs() < 1e-6, "fold {v}");
        }
        for (v, wrapped) in [(0.5, 0.5), (1.5, -0.5), (-1.5, 0.5)] {
            assert!((Shape::Wrap.apply(v) - wrapped).abs() < 1e-6, "wrap {v}");
        }
    }

    #[test]
    fn mix_blends_with_the_dry_signal() {
        use crate::testing::{node, process_one};
        let mut dry = node("distortion", r#"{ "mix": 0 }"#, 4, 4.0, &[true]);
        let input = vec![0.1, -0.2, 0.3, 0.9];
        assert_eq!(
            process_one(dry.as_mut(), std::slice::from_ref(&input)),
            input
        );
        let mut hard = node(
            "distortion",
            r#"{ "shape": "hard", "drive": 20 }"#,
            4,
            4.0,
            &[true],
        );
        assert_eq!(process_one(hard.as_mut(), &[input]), [1.0, -1.0, 1.0, 1.0]);
    }
}
