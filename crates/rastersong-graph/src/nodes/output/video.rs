use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Layout, LayoutContext, Node, ProcessContext, Signal};

/// The graph's result. Accepts an RGB signal of the output size, or a mono one, which is shown
/// as grayscale.
#[derive(Debug)]
pub struct Output;

impl Output {
    pub const SPEC: NodeSpec = NodeSpec::new("Output", Category::Output)
        .describe("The rendered result: RGB, or mono shown as grayscale");
}

impl Node for Output {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        let size_matches = (input.width, input.height) == (ctx.output.width, ctx.output.height);
        if size_matches && matches!(input.samples_per_pixel, 1 | 3) {
            Ok(vec![ctx.output])
        } else {
            Err(format!(
                "expects a {} or mono signal of that size, got {input}",
                ctx.output
            ))
        }
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = inputs[0];
        if input.layout.samples_per_pixel == 3 {
            outputs[0].data.copy_from_slice(&input.data);
        } else {
            for (pixel, &value) in outputs[0].data.chunks_mut(3).zip(&input.data) {
                pixel.fill(value);
            }
        }
    }
}
