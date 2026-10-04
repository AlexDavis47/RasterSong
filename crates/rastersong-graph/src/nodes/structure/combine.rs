use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{
    InputSpec, Layout, LayoutContext, Node, OutputSpec, Params, PortHint, ProcessContext, Signal,
};

/// Separate R, G and B signals → RGB. The inverse of [`super::Split`].
#[derive(Debug)]
pub struct Combine;

impl NodeKind for Combine {
    const KIND: &'static str = "combine";
    const SPEC: NodeSpec = NodeSpec::new("Combine", Category::Structure)
        .describe("Separate R, G and B signals into RGB")
        .inputs(&[
            InputSpec::required("r", "The red channel"),
            InputSpec::required("g", "The green channel"),
            InputSpec::required("b", "The blue channel"),
        ])
        .outputs(&[OutputSpec::new("out", "RGB video").hint(PortHint::Rgb)]);

    fn new(_: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

impl Node for Combine {
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{Layout, LayoutContext, ProcessContext, Registry, Signal};

    fn ctx<'a>(inputs: &'a [Layout], sources: &'a HashMap<String, Layout>) -> LayoutContext<'a> {
        LayoutContext {
            inputs,
            sources,
            output: Layout::rgb(2, 1),
            output_count: 1,
        }
    }

    #[test]
    fn interleaves_three_channels_into_pixels() {
        let mono = Layout::mono(2, 1);
        let signals = [
            Signal::from_data(mono, vec![0.1, 0.4]),
            Signal::from_data(mono, vec![0.2, 0.5]),
            Signal::from_data(mono, vec![0.3, 0.6]),
        ];
        let mut node = Registry::shared()
            .create("combine", &Default::default())
            .unwrap()
            .unwrap();
        let mut out = vec![Signal::zeros(Layout::rgb(2, 1))];
        let refs: Vec<&Signal> = signals.iter().collect();
        let sources = HashMap::<String, Signal>::new();
        let process = ProcessContext {
            frame: 0,
            frame_rate: 1.0,
            sources: &sources,
            params: &[],
        };
        node.process(&process, &refs, &mut out);
        assert_eq!(out[0].data, [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
    }

    #[test]
    fn rejects_mismatched_sizes() {
        let node = Registry::shared()
            .create("combine", &Default::default())
            .unwrap()
            .unwrap();
        let sources = HashMap::new();
        let layouts = [Layout::mono(2, 1), Layout::mono(2, 1), Layout::mono(3, 1)];
        assert!(node.output_layouts(&ctx(&layouts, &sources)).is_err());
        let layouts = [Layout::mono(2, 1); 3];
        assert_eq!(
            node.output_layouts(&ctx(&layouts, &sources)).unwrap(),
            [Layout::rgb(2, 1)]
        );
    }
}
