use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, ProcessContext, Signal};

/// Amplitude modulation: `out = carrier × (1 + depth × modulator)`.
#[derive(Debug)]
pub struct Am {
    depth: f32,
}

impl Am {
    pub const SPEC: NodeSpec = NodeSpec::new("Amplitude Modulation", Category::Effect)
        .describe("Scales the carrier by the modulator")
        .params(Self::PARAMS)
        .per_channel();

    pub const PARAMS: &[ParamSpec] = &[ParamSpec::number(
        "depth",
        "Depth",
        1.0,
        -10.0,
        10.0,
        "How strongly the modulator scales the carrier: carrier × (1 + depth × modulator)",
    )
    .unbounded()];

    pub fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            depth: params.number("depth")? as f32,
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
