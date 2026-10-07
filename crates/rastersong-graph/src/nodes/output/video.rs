use rastersong_lang::{tr_args};
use crate::nodes::{Category, NodeKind, NodeSpec, OUTPUT};
use crate::{
    InputSpec, Layout, LayoutContext, Node, Params, ProcessContext, Range, Signal, TagRule,
};

/// The graph's result. Accepts an RGB signal of the output size, or a mono one, which is shown
/// as grayscale.
#[derive(Debug)]
pub struct Output;

impl NodeKind for Output {
    const KIND: &'static str = OUTPUT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Output)
        .inputs(&[InputSpec::required("in")])
        .outputs(&[crate::OutputSpec::new("out").tag(TagRule::VIDEO)])
        .expects(Range::Unipolar);

    fn new(_: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

impl Node for Output {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        let size_matches = (input.width, input.height) == (ctx.output.width, ctx.output.height);
        if size_matches && matches!(input.samples_per_pixel, 1 | 3) {
            Ok(vec![ctx.output])
        } else {
            Err(tr_args(
                "error.video_output.shape",
                &[("expected", &ctx.output.to_string()), ("got", &input.to_string())],
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{Layout, LayoutContext, Node, ProcessContext, Registry, Signal};

    fn output() -> Box<dyn Node> {
        Registry::shared()
            .create("output", &Default::default())
            .unwrap()
            .unwrap()
    }

    #[test]
    fn mono_is_shown_as_gray() {
        let mut node = output();
        let input = Signal::from_data(Layout::mono(2, 1), vec![0.25, 0.75]);
        let mut out = vec![Signal::zeros(Layout::rgb(2, 1))];
        let sources = HashMap::<String, Signal>::new();
        let ctx = ProcessContext {
            frame: 0,
            frame_rate: 1.0,
            sources: &sources,
            params: &[],
        };
        node.process(&ctx, &[&input], &mut out);
        assert_eq!(out[0].data, [0.25, 0.25, 0.25, 0.75, 0.75, 0.75]);
    }

    #[test]
    fn rejects_the_wrong_size() {
        let node = output();
        let sources = HashMap::new();
        let layouts = |input: Layout| {
            let inputs = [input];
            node.output_layouts(&LayoutContext {
                inputs: &inputs,
                connected: &[true],
                sources: &sources,
                output: Layout::rgb(4, 2),
                layout: Default::default(),
                output_count: 1,
            })
        };
        assert!(layouts(Layout::rgb(4, 2)).is_ok());
        assert!(layouts(Layout::mono(4, 2)).is_ok());
        assert!(layouts(Layout::rgb(3, 2)).is_err());
        assert!(layouts(Layout::audio(100)).is_err());
    }
}
