use crate::nodes::support::expect_rgb;
use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{
    InputSpec, Layout, LayoutContext, Node, OutputSpec, Params, PortHint, ProcessContext, Signal,
};

/// RGB → separate R, G and B signals.
#[derive(Debug)]
pub struct Split;

impl NodeKind for Split {
    const KIND: &'static str = "split";
    const SPEC: NodeSpec = NodeSpec::new("Split", Category::Structure)
        .describe("RGB into separate R, G and B signals")
        .inputs(&[InputSpec::required("in", "RGB video to take apart")])
        .outputs(&[
            OutputSpec::new("r", "The red channel").hint(PortHint::Red),
            OutputSpec::new("g", "The green channel").hint(PortHint::Green),
            OutputSpec::new("b", "The blue channel").hint(PortHint::Blue),
        ]);

    fn new(_: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

impl Node for Split {
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

#[cfg(test)]
mod tests {
    use crate::{Layout, Registry, Signal};

    #[test]
    fn separates_the_channels_of_each_pixel() {
        let registry = Registry::shared();
        let mut node = registry.create("split", &Default::default()).unwrap().unwrap();
        let input = Signal::from_data(Layout::rgb(2, 1), vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
        let mut outputs = vec![Signal::zeros(Layout::mono(2, 1)); 3];
        let ctx = crate::ProcessContext {
            frame: 0,
            frame_rate: 1.0,
            sources: &std::collections::HashMap::<String, Signal>::new(),
            params: &[],
        };
        node.process(&ctx, &[&input], &mut outputs);
        assert_eq!(outputs[0].data, [0.1, 0.4]);
        assert_eq!(outputs[1].data, [0.2, 0.5]);
        assert_eq!(outputs[2].data, [0.3, 0.6]);
    }
}
