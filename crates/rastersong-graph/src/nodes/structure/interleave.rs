use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{
    Diagnostic, InputSpec, Layout, LayoutContext, Node, OutputSpec, Params, ProcessContext, Signal,
};

/// An interleaved signal (RGB, stereo, …) → one mono carrier with the channels in sequence
/// (R, G, B, R, G, B, …), as many times as wide as there are channels. The samples don't change,
/// but downstream nodes now treat each channel value as its own sample: a mono modulator varies
/// across a pixel's R, G and B instead of moving them together.
#[derive(Debug)]
pub struct Interleave;

impl NodeKind for Interleave {
    const KIND: &'static str = "interleave";
    const SPEC: NodeSpec = NodeSpec::new("Interleave", Category::Structure)
        .describe("Channels as one mono carrier, as many times as wide (R, G, B, R, G, B, …)")
        .inputs(&[InputSpec::required("in", "An interleaved signal, such as RGB video, to flatten")])
        .outputs(&[OutputSpec::new(
            "out",
            "The channels in sequence, as one mono signal as many times as wide as there are channels",
        )]);

    fn new(_: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

impl Node for Interleave {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        Ok(vec![input.reshaped(
            input.width * input.samples_per_pixel,
            input.height,
            1,
        )])
    }

    fn diagnostics(&self, ctx: &LayoutContext) -> Vec<Diagnostic> {
        if ctx.inputs[0].samples_per_pixel == 1 {
            vec![Diagnostic::note(
                "The input already has one channel, so it passes through unchanged.",
            )]
        } else {
            Vec::new()
        }
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        outputs[0].data.copy_from_slice(&inputs[0].data);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{ChannelMap, Kind, Layout, LayoutContext, Registry};

    #[test]
    fn makes_a_mono_signal_as_wide_as_its_samples() {
        let node = Registry::shared()
            .create("interleave", &Default::default())
            .unwrap()
            .unwrap();
        let sources = HashMap::new();
        let layouts = |input: Layout| {
            node.output_layouts(&LayoutContext {
                inputs: &[input],
                connected: &[true],
                sources: &sources,
                output: Layout::rgb(4, 2),
                output_count: 1,
            })
            .unwrap()[0]
        };
        let rgb = layouts(Layout::video(4, 2));
        assert!(rgb.same_shape(&Layout::mono(12, 2)));
        assert_eq!(rgb.tag.kind, Kind::Video);
        assert_eq!(rgb.tag.channels, ChannelMap::Mono);
        assert!(layouts(Layout::audio_channels(5, 2)).same_shape(&Layout::mono(10, 1)));
        // Mono passes through rather than failing.
        assert!(layouts(Layout::mono(4, 2)).same_shape(&Layout::mono(4, 2)));
    }
}
