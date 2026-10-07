use crate::dsp::mix;
use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, ProcessContext, Signal};

choice! {
    /// Which way a value moves to land on a step.
    pub enum Rounding {
        /// To the nearest step.
        Nearest = "nearest",
        /// Down to the step below.
        Floor = "floor",
        /// Up to the step above.
        Ceil = "ceil",
    }
}

/// Snaps every sample to the nearest multiple of a step size: posterized video, stepped audio.
/// Stateless.
#[derive(Debug)]
pub struct Quantize {
    step: f32,
    offset: f32,
    rounding: Rounding,
    mix: f32,
}

params! { Quantize {
    STEP: ParamSpec::number(
        "step",
        "Step",
        0.125,
        0.001,
        1.0,
        "Size of one step: 0.25 gives the levels 0, 0.25, 0.5, 0.75, 1",
    )
    .exposed()
    .limits(1e-6, 1e6),
    OFFSET: ParamSpec::number(
        "offset",
        "Offset",
        0.0,
        -1.0,
        1.0,
        "Where the steps start: steps sit at offset + n × step",
    )
    .limits(-1e6, 1e6),
    ROUNDING: ParamSpec::choice(
        "rounding",
        "Rounding",
        Rounding::OPTIONS,
        "nearest",
        "nearest picks the closest step, floor the one below, ceil the one above",
    ),
    MIX: ParamSpec::mix(),
} }

impl NodeKind for Quantize {
    const KIND: &'static str = "quantize";
    const SPEC: NodeSpec = NodeSpec::new("Quantize", Category::Effect)
        .describe("Snaps every sample to a grid of evenly spaced levels")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "step": 0.25 }"#,
        r#"{ "step": 0.1, "offset": 0.05, "rounding": "floor" }"#,
        r#"{ "step": 0.3, "rounding": "ceil", "mix": 0.5 }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            step: params.float_at(Self::STEP)?,
            offset: params.float_at(Self::OFFSET)?,
            rounding: params.choice_as(Self::ROUNDING)?,
            mix: params.float_at(Self::MIX)?,
        })
    }
}

impl Node for Quantize {
    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let step = ctx.value(Self::STEP, f64::from(self.step));
        let offset = ctx.value(Self::OFFSET, f64::from(self.offset));
        let amount = ctx.value(Self::MIX, f64::from(self.mix));
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let (step, offset) = (step.at(i).max(1e-6), offset.at(i));
            let n = (x - offset) / step;
            let n = match self.rounding {
                Rounding::Nearest => n.round(),
                Rounding::Floor => n.floor(),
                Rounding::Ceil => n.ceil(),
            };
            *out = mix(x, offset + n * step, amount.at(i));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn quantize(params: &str, input: &[f32]) -> Vec<f32> {
        let mut n = node("quantize", params, input.len(), input.len() as f64, &[true]);
        process_one(n.as_mut(), &[input.to_vec()])
    }

    #[test]
    fn snaps_to_the_nearest_step() {
        let out = quantize(r#"{ "step": 0.25 }"#, &[0.0, 0.1, 0.13, 0.6, 1.0]);
        assert_eq!(out, [0.0, 0.0, 0.25, 0.5, 1.0]);
    }

    #[test]
    fn floor_and_ceil_pick_a_side() {
        let input = [0.1, 0.4, -0.1];
        assert_eq!(
            quantize(r#"{ "step": 0.25, "rounding": "floor" }"#, &input),
            [0.0, 0.25, -0.25]
        );
        assert_eq!(
            quantize(r#"{ "step": 0.25, "rounding": "ceil" }"#, &input),
            [0.25, 0.5, 0.0]
        );
    }

    #[test]
    fn the_offset_moves_the_grid() {
        let out = quantize(r#"{ "step": 0.5, "offset": 0.25 }"#, &[0.3, 0.6, 0.9]);
        assert_eq!(out, [0.25, 0.75, 0.75]);
    }

    #[test]
    fn mix_zero_is_the_dry_signal() {
        let input = [0.13, 0.77];
        assert_eq!(quantize(r#"{ "mix": 0 }"#, &input), input);
    }
}
