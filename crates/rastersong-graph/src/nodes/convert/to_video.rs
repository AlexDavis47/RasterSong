use super::{Mapping, SIGNED_MAX, flip_sign_bit};
use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, PortHint, ProcessContext, Signal};

/// Audio (`-1..=1`) back to video (`0..=1`). `bugged` misreads the audio again, which undoes the
/// first misread; samples that crossed the seam wrap to the other end of the brightness range.
#[derive(Debug)]
pub struct ToVideo {
    mapping: Mapping,
}

impl ToVideo {
    pub const PARAMS: &[ParamSpec] = Mapping::PARAMS;
    pub const SPEC: NodeSpec = NodeSpec::new("Audio to Video", Category::Convert)
        .describe("Audio's -1 to 1 back to video's 0 to 1, as read from an 8-bit file")
        .params(Self::PARAMS);

    pub fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            mapping: Mapping::read(params)?,
        })
    }

    pub fn convert(mapping: Mapping, a: f32) -> f32 {
        match mapping {
            Mapping::Accurate => (a.clamp(-1.0, 1.0) + 1.0) / 2.0,
            Mapping::Bugged => {
                let signed = flip_sign_bit(a.clamp(-1.0, SIGNED_MAX));
                ((128.0 * signed + 127.0) / 255.0).max(0.0)
            }
        }
    }
}

impl Node for ToVideo {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn output_hints(&self) -> &'static [PortHint] {
        &[PortHint::AsVideo]
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        for (out, &a) in outputs[0].data.iter_mut().zip(&inputs[0].data) {
            *out = Self::convert(self.mapping, a);
        }
    }
}
