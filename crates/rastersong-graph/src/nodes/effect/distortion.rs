use crate::dsp::db_to_gain;
use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, ProcessContext, Signal};

/// How the driven signal is bent back into `-1..=1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `tanh`: rounds off smoothly.
    Soft,
    /// Clips flat at ±1.
    Hard,
    /// Reflects back from ±1, folding loud parts over.
    Fold,
    /// Wraps around from +1 to -1, like integer overflow.
    Wrap,
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
    drive: f32,
    bias: f32,
    mix: f32,
}

impl Distortion {
    pub const PARAMS: &[ParamSpec] = &[
        ParamSpec::choice(
            "shape",
            "Shape",
            &["soft", "hard", "fold", "wrap"],
            "soft",
            "soft rounds off, hard clips flat, fold reflects loud parts back, wrap jumps from top to bottom",
        ),
        ParamSpec::number(
            "drive",
            "Drive",
            12.0,
            0.0,
            48.0,
            "Gain before shaping; more drive, more distortion",
        )
        .unit("dB"),
        ParamSpec::number(
            "bias",
            "Bias",
            0.0,
            -1.0,
            1.0,
            "Offset added before shaping, for uneven distortion",
        ),
        ParamSpec::number(
            "mix",
            "Mix",
            1.0,
            0.0,
            1.0,
            "0 is the dry input, 1 is only the distorted signal",
        ),
    ];

    pub const SPEC: NodeSpec = NodeSpec::new("Distortion", Category::Effect)
        .describe("Drives the signal into a waveshaper: soft, hard, folding or wrapping")
        .params(Self::PARAMS)
        .per_channel();

    pub fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            shape: match params.choice("shape")? {
                "hard" => Shape::Hard,
                "fold" => Shape::Fold,
                "wrap" => Shape::Wrap,
                _ => Shape::Soft,
            },
            drive: db_to_gain(params.number("drive")?) as f32,
            bias: params.number("bias")? as f32,
            mix: params.number("mix")? as f32,
        })
    }
}

impl Node for Distortion {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        for (out, &x) in outputs[0].data.iter_mut().zip(&inputs[0].data) {
            let wet = self.shape.apply(self.drive * x + self.bias);
            *out = x + (wet - x) * self.mix;
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
        use super::super::test_util::{node, process};
        let mut dry = node("distortion", r#"{ "mix": 0 }"#, 4, 4.0, &[true]);
        let input = vec![0.1, -0.2, 0.3, 0.9];
        assert_eq!(process(dry.as_mut(), std::slice::from_ref(&input)), input);
        let mut hard = node(
            "distortion",
            r#"{ "shape": "hard", "drive": 20 }"#,
            4,
            4.0,
            &[true],
        );
        assert_eq!(process(hard.as_mut(), &[input]), [1.0, -1.0, 1.0, 1.0]);
    }
}
