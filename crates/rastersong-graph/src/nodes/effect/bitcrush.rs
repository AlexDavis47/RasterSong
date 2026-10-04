use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, ProcessContext, Signal};

/// Reduces bit depth: quantizes to `2^bits` levels over `0..=1`. Fractional and modulated
/// bit depths are allowed.
#[derive(Debug)]
pub struct Bitcrush {
    bits: f32,
    depth: f32,
}

impl Bitcrush {
    pub const SPEC: NodeSpec = NodeSpec::new("Bit Crush", Category::Effect)
        .describe("Reduces bit depth, posterizing the image")
        .params(Self::PARAMS)
        .per_channel();

    pub const PARAMS: &[ParamSpec] = &[
        ParamSpec::number(
            "bits",
            "Bits",
            4.0,
            1.0,
            24.0,
            "Bit depth; fewer bits means fewer levels",
        )
        .unit("bits"),
        ParamSpec::number(
            "depth",
            "Depth",
            0.0,
            -24.0,
            24.0,
            "Bits added per unit of the modulation input",
        ),
    ];

    pub fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            bits: params.number("bits")? as f32,
            depth: params.number("depth")? as f32,
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
