use crate::nodes::support::expect_rgb;
use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{
    InputSpec, Layout, LayoutContext, Node, OutputSpec, Params, PortHint, ProcessContext, Signal,
};

/// RGB → one mono carrier with the channels packed in sequence (R, G, B, R, G, B, …), three times
/// as wide. The samples don't change, but downstream nodes now treat each channel value as its own
/// sample: a mono modulator varies across a pixel's R, G and B instead of moving them together.
#[derive(Debug)]
pub struct Interleave;

impl NodeKind for Interleave {
    const KIND: &'static str = "interleave";
    const SPEC: NodeSpec = NodeSpec::new("Interleave", Category::Structure)
        .describe("RGB as one mono carrier, three times as wide (R, G, B, R, G, B, …)")
        .inputs(&[InputSpec::required("in", "RGB video to pack")])
        .outputs(&[OutputSpec::new(
            "out",
            "The channels in sequence, as one mono signal three times as wide",
        )
        .hint(PortHint::Rgb)]);

    fn new(_: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

impl Node for Interleave {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        expect_rgb(input)?;
        Ok(vec![Layout::mono(input.width * 3, input.height)])
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
    fn makes_a_mono_signal_three_times_as_wide() {
        let node = Registry::shared()
            .create("interleave", &Default::default())
            .unwrap()
            .unwrap();
        let sources = HashMap::new();
        let inputs = [Layout::rgb(4, 2)];
        let context = LayoutContext {
            inputs: &inputs,
            sources: &sources,
            output: Layout::rgb(4, 2),
            output_count: 1,
        };
        assert_eq!(node.output_layouts(&context).unwrap(), [Layout::mono(12, 2)]);
        let inputs = [Layout::mono(4, 2)];
        let context = LayoutContext {
            inputs: &inputs,
            ..context
        };
        assert!(node.output_layouts(&context).is_err());
    }
}
