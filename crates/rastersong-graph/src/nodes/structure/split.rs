use super::expect_rgb;
use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Layout, LayoutContext, Node, PortHint, ProcessContext, Signal};

/// RGB → separate R, G and B signals.
#[derive(Debug)]
pub struct Split;

impl Split {
    pub const SPEC: NodeSpec = NodeSpec::new("Split", Category::Structure)
        .describe("RGB into separate R, G and B signals");
}

impl Node for Split {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn outputs(&self) -> &'static [&'static str] {
        &["r", "g", "b"]
    }

    fn output_hints(&self) -> &'static [PortHint] {
        &[PortHint::Red, PortHint::Green, PortHint::Blue]
    }

    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        expect_rgb(input)?;
        Ok(vec![Layout::mono(input.width, input.height); 3])
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let [r, g, b] = outputs else { unreachable!() };
        for (i, pixel) in inputs[0].data.as_chunks::<3>().0.iter().enumerate() {
            r.data[i] = pixel[0];
            g.data[i] = pixel[1];
            b.data[i] = pixel[2];
        }
    }
}
