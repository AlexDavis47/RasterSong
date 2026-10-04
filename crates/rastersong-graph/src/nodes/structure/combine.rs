use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Layout, LayoutContext, Node, ProcessContext, Signal};

/// Separate R, G and B signals → RGB. The inverse of [`super::Split`].
#[derive(Debug)]
pub struct Combine;

impl Combine {
    pub const SPEC: NodeSpec = NodeSpec::new("Combine", Category::Structure)
        .describe("Separate R, G and B signals into RGB");
}

impl Node for Combine {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[
            InputSpec::required("r"),
            InputSpec::required("g"),
            InputSpec::required("b"),
        ];
        INPUTS
    }

    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let r = ctx.inputs[0];
        // Rate matching is 1-D; it must never be used to reconcile images of different sizes.
        if r.samples_per_pixel != 1 || ctx.inputs.iter().any(|&l| l != r) {
            return Err(format!(
                "expects three mono signals of the same size, got {}, {}, {}",
                ctx.inputs[0], ctx.inputs[1], ctx.inputs[2]
            ));
        }
        Ok(vec![Layout::rgb(r.width, r.height)])
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let [r, g, b] = inputs else { unreachable!() };
        for (i, pixel) in outputs[0]
            .data
            .as_chunks_mut::<3>()
            .0
            .iter_mut()
            .enumerate()
        {
            pixel[0] = r.data[i];
            pixel[1] = g.data[i];
            pixel[2] = b.data[i];
        }
    }
}
