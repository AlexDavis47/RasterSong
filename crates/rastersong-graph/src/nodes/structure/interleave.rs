use super::expect_rgb;
use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Layout, LayoutContext, Node, ProcessContext, Signal};

/// RGB → one mono carrier with the channels packed in sequence (R, G, B, R, G, B, …), three times
/// as wide. The samples don't change, but downstream nodes now treat each channel value as its own
/// sample: a mono modulator varies across a pixel's R, G and B instead of moving them together.
#[derive(Debug)]
pub struct Interleave;

impl Interleave {
    pub const SPEC: NodeSpec = NodeSpec::new("Interleave", Category::Structure)
        .describe("RGB as one mono carrier, three times as wide (R, G, B, R, G, B, …)");
}

impl Node for Interleave {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        expect_rgb(input)?;
        Ok(vec![Layout::mono(input.width * 3, input.height)])
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        outputs[0].data.copy_from_slice(&inputs[0].data);
    }
}
