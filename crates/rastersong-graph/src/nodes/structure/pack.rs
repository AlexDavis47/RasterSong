use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{
    InputSpec, Layout, LayoutContext, Node, OutputSpec, Params, PortHint, ProcessContext, Signal,
};

/// A packed mono carrier → RGB. The inverse of [`super::Interleave`].
#[derive(Debug)]
pub struct Pack;

impl NodeKind for Pack {
    const KIND: &'static str = "pack";
    const SPEC: NodeSpec = NodeSpec::new("Pack", Category::Structure)
        .describe("A packed mono carrier back into RGB")
        .inputs(&[InputSpec::required(
            "in",
            "A mono signal three times as wide as the picture, as Interleave makes",
        )])
        .outputs(&[OutputSpec::new("out", "RGB video").hint(PortHint::Rgb)]);

    fn new(_: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

impl Node for Pack {
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{Layout, LayoutContext, Registry};

    #[test]
    fn needs_a_width_divisible_by_three() {
        let node = Registry::shared()
            .create("pack", &Default::default())
            .unwrap()
            .unwrap();
        let sources = HashMap::new();
        let layout = |width| {
            let inputs = [Layout::mono(width, 2)];
            node.output_layouts(&LayoutContext {
                inputs: &inputs,
                sources: &sources,
                output: Layout::rgb(2, 2),
                output_count: 1,
            })
        };
        assert_eq!(layout(6).unwrap(), [Layout::rgb(2, 2)]);
        assert!(layout(7).is_err());
    }
}
