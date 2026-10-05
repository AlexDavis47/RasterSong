use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{InputSpec, Layout, LayoutContext, Node, OutputSpec, Params, ProcessContext, Signal};

/// A signal stretched (or squeezed) to the length and layout of another. Every node does this to
/// its secondary inputs; this node does it on purpose, so the stretched signal itself goes on.
/// The node's grouping and interpolation settings choose how.
#[derive(Debug)]
pub struct Stretch;

impl NodeKind for Stretch {
    const KIND: &'static str = "stretch";
    const SPEC: NodeSpec = NodeSpec::new("Stretch to Match", Category::Structure)
        .describe("A signal stretched to the length and layout of another")
        .doc("The output has `like`'s size and layout and `in`'s values: audio stretched over a picture, or a picture squeezed into an audio block. With pixel grouping (the default), a mono signal stretched over RGB moves each pixel's channels together; per sample, it is spread over every channel value.")
        .inputs(&[
            InputSpec::required("like", "The signal whose size and layout the result takes"),
            InputSpec::required("in", "The signal to stretch"),
        ])
        .outputs(&[OutputSpec::new("out", "`in` at the size of `like`")]);

    fn new(_: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

impl Node for Stretch {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        // `like`'s shape, carrying what `in` is.
        let (like, input) = (ctx.inputs[0], ctx.inputs[1]);
        let mut layout = input.reshaped(like.width, like.height, like.samples_per_pixel);
        layout.tag = layout.tag.fit(like.samples_per_pixel);
        Ok(vec![layout])
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        // The graph already stretched `in` to `like`'s length.
        outputs[0].data.copy_from_slice(&inputs[1].data);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{Kind, Layout, LayoutContext, Range, Registry};

    #[test]
    fn takes_the_shape_of_like_and_the_tag_of_in() {
        let node = Registry::shared()
            .create("stretch", &Default::default())
            .unwrap()
            .unwrap();
        let sources = HashMap::new();
        let layout = node
            .output_layouts(&LayoutContext {
                inputs: &[Layout::video(4, 2), Layout::audio(100)],
                connected: &[true, true],
                sources: &sources,
                output: Layout::rgb(4, 2),
                output_count: 1,
            })
            .unwrap()[0];
        assert!(layout.same_shape(&Layout::rgb(4, 2)));
        assert_eq!(layout.tag.kind, Kind::Audio);
        assert_eq!(layout.tag.range, Range::Bipolar);
    }
}
