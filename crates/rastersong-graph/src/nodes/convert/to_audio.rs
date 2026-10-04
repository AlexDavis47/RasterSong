use super::{Mapping, SIGNED_MAX, flip_sign_bit};
use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, ProcessContext, Signal};

/// Video (`0..=1`) to audio (`-1..=1`).
#[derive(Debug)]
pub struct ToAudio {
    mapping: Mapping,
}

impl ToAudio {
    pub const PARAMS: &[ParamSpec] = Mapping::PARAMS;
    pub const SPEC: NodeSpec = NodeSpec::new("Video to Audio", Category::Convert)
        .describe("Video's 0 to 1 as audio's -1 to 1, as written to an 8-bit file")
        .params(Self::PARAMS);

    pub fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            mapping: Mapping::read(params)?,
        })
    }

    pub fn convert(mapping: Mapping, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match mapping {
            Mapping::Accurate => 2.0 * x - 1.0,
            // The prototype's `pixel - 127`, in 8-bit steps. Signed 8-bit tops out one step
            // below 1, which keeps white clear of black after the flip.
            Mapping::Bugged => flip_sign_bit(((255.0 * x - 127.0) / 128.0).min(SIGNED_MAX)),
        }
    }
}

impl Node for ToAudio {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        for (out, &x) in outputs[0].data.iter_mut().zip(&inputs[0].data) {
            *out = Self::convert(self.mapping, x);
        }
    }
}
