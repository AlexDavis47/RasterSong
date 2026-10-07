use crate::nodes::{CHANNEL_PORTS, Category, MAX_CHANNELS, NodeKind, NodeSpec, SPLIT};
use crate::{
    Diagnostic, InputSpec, Layout, LayoutContext, Node, OutputSpec, Params, Part, ProcessContext,
    Signal, TagRule,
};
use rastersong_lang::{tr, tr_args};

/// An interleaved signal → one signal per channel: R, G and B of video, L and R of stereo, or
/// any other channels. Has as many outputs as the input has channels (up to
/// [`MAX_CHANNELS`]); the editor names them from the input's tag.
#[derive(Debug)]
pub struct Split;

const fn channel(name: &'static str) -> OutputSpec {
    // A split-off channel is a part of the whole; which one comes from the input's tag.
    OutputSpec::new(name).tag(TagRule::INHERIT)
}

impl NodeKind for Split {
    const KIND: &'static str = SPLIT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Structure)
        .inputs(&[InputSpec::required("in")])
        .outputs(&[
            channel(CHANNEL_PORTS[0]),
            channel(CHANNEL_PORTS[1]),
            channel(CHANNEL_PORTS[2]),
            channel(CHANNEL_PORTS[3]),
            channel(CHANNEL_PORTS[4]),
            channel(CHANNEL_PORTS[5]),
            channel(CHANNEL_PORTS[6]),
            channel(CHANNEL_PORTS[7]),
        ]);

    fn new(_: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

impl Node for Split {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        let channels = input.samples_per_pixel as usize;
        Ok((0..ctx.output_count)
            .map(|c| {
                let mut layout = input.reshaped(input.width, input.height, 1);
                layout.tag.part = if c < channels {
                    input.tag.channel_part(c)
                } else {
                    Part::Whole
                };
                layout
            })
            .collect())
    }

    fn diagnostics(&self, ctx: &LayoutContext) -> Vec<Diagnostic> {
        let channels = ctx.inputs[0].samples_per_pixel as usize;
        match channels {
            1 => vec![Diagnostic::note(tr("diagnostic.split.one_channel"))],
            n if n > MAX_CHANNELS => vec![Diagnostic::warning(tr_args(
                "diagnostic.split.too_many",
                &[
                    ("count", &n.to_string()),
                    ("max", &MAX_CHANNELS.to_string()),
                ],
            ))],
            _ => Vec::new(),
        }
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = inputs[0];
        let channels = input.layout.samples_per_pixel.max(1) as usize;
        for (c, output) in outputs.iter_mut().enumerate() {
            if c < channels {
                for (out, &x) in output
                    .data
                    .iter_mut()
                    .zip(input.data.iter().skip(c).step_by(channels))
                {
                    *out = x;
                }
            } else {
                output.data.fill(0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::nodes::MAX_CHANNELS;
    use crate::{Layout, LayoutContext, Part, ProcessContext, Registry, Signal};

    fn split(input: &Signal) -> Vec<Signal> {
        let mut node = Registry::shared()
            .create("split", &Default::default())
            .unwrap()
            .unwrap();
        let sources = HashMap::new();
        let layouts = node
            .output_layouts(&LayoutContext {
                inputs: &[input.layout],
                connected: &[true],
                sources: &sources,
                output: input.layout,
                layout: Default::default(),
                output_count: MAX_CHANNELS,
            })
            .unwrap();
        let mut outputs: Vec<Signal> = layouts.into_iter().map(Signal::zeros).collect();
        let ctx = ProcessContext {
            frame: 0,
            frame_rate: 1.0,
            sources: &HashMap::<String, Signal>::new(),
            params: &[],
        };
        node.process(&ctx, &[input], &mut outputs);
        outputs
    }

    #[test]
    fn separates_the_channels_of_each_pixel() {
        let input = Signal::from_data(Layout::video(2, 1), vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
        let outputs = split(&input);
        assert_eq!(outputs[0].data, [0.1, 0.4]);
        assert_eq!(outputs[1].data, [0.2, 0.5]);
        assert_eq!(outputs[2].data, [0.3, 0.6]);
        assert_eq!(outputs[3].data, [0.0, 0.0]);
        assert_eq!(outputs[1].layout.tag.part, Part::Green);
        assert_eq!(outputs[0].layout.samples_per_pixel, 1);
    }

    #[test]
    fn splits_stereo_into_left_and_right() {
        let input = Signal::from_data(
            Layout::audio_channels(3, 2),
            vec![1., -1., 2., -2., 3., -3.],
        );
        let outputs = split(&input);
        assert_eq!(outputs[0].data, [1.0, 2.0, 3.0]);
        assert_eq!(outputs[1].data, [-1.0, -2.0, -3.0]);
        assert_eq!(outputs[0].layout.tag.part, Part::Left);
        assert_eq!(outputs[1].layout.tag.part, Part::Right);
    }
}
