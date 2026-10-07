use crate::nodes::support::{Mapping, SIGNED_MAX, flip_sign_bit};
use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{InputSpec, Node, OutputSpec, Params, ProcessContext, Signal, TagRule};

/// Audio (`-1..=1`) back to video (`0..=1`). `bugged` misreads the audio again, which undoes the
/// first misread; samples that crossed the seam wrap to the other end of the brightness range.
#[derive(Debug)]
pub struct ToVideo {
    mapping: Mapping,
}

params! { ToVideo {
    MAPPING: Mapping::MAPPING_PARAM,
} }

impl NodeKind for ToVideo {
    const KIND: &'static str = "to_video";
    const SPEC: NodeSpec = NodeSpec::new(Category::Convert)
        .params(Self::PARAMS)
        .inputs(&[InputSpec::required("in")])
        .outputs(&[
            OutputSpec::new("out").tag(TagRule::VIDEO),
        ]);
    const TEST_CONFIGS: &'static [&'static str] =
        &[r#"{ "mapping": "bugged" }"#, r#"{ "mapping": "accurate" }"#];
    const BENCH: Option<&'static str> = Some(r#"{ "mapping": "bugged" }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            mapping: params.choice_as(Self::MAPPING)?,
        })
    }
}

impl ToVideo {
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
    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        for (out, &a) in outputs[0].data.iter_mut().zip(&inputs[0].data) {
            *out = Self::convert(self.mapping, a);
        }
    }
}
