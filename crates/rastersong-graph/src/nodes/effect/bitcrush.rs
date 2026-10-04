use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, ProcessContext, Signal};

/// Reduces bit depth: quantizes to `2^bits` levels over `0..=1`. Fractional and modulated
/// bit depths are allowed.
#[derive(Debug)]
pub struct Bitcrush {
    bits: f32,
}

impl Bitcrush {
    pub const SPEC: NodeSpec = NodeSpec::new("Bit Crush", Category::Effect)
        .describe("Reduces bit depth, posterizing the image")
        .params(Self::PARAMS)
        .per_channel();

    pub const PARAMS: &[ParamSpec] = &[ParamSpec::number(
        "bits",
        "Bits",
        4.0,
        1.0,
        24.0,
        "Bit depth; fewer bits means fewer levels",
    )
    .exposed()
    .unit("bits")];

    pub fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            bits: params.number("bits")? as f32,
        })
    }
}

impl Node for Bitcrush {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let bits = ctx.param(0);
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let bits = bits.map_or(self.bits, |b| b[i]).clamp(1.0, 24.0);
            let steps = bits.exp2() - 1.0;
            *out = (x * steps).round() / steps;
        }
    }
}
