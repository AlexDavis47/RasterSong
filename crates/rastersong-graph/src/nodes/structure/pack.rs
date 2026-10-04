use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Layout, LayoutContext, Node, PortHint, ProcessContext, Signal};

/// A packed mono carrier → RGB. The inverse of [`super::Interleave`].
#[derive(Debug)]
pub struct Pack;

impl Pack {
    pub const SPEC: NodeSpec =
        NodeSpec::new("Pack", Category::Structure).describe("A packed mono carrier back into RGB");
}

impl Node for Pack {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn output_hints(&self) -> &'static [PortHint] {
        &[PortHint::Rgb]
    }

    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        if input.samples_per_pixel != 1 || !input.width.is_multiple_of(3) {
            return Err(format!(
                "expects a mono signal whose width is a multiple of 3, got {input}"
            ));
        }
        Ok(vec![Layout::rgb(input.width / 3, input.height)])
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        outputs[0].data.copy_from_slice(&inputs[0].data);
    }
}
