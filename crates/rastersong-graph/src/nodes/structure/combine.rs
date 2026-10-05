use crate::nodes::{CHANNEL_PORTS, COMBINE, Category, NodeKind, NodeSpec};
use crate::{
    InputSpec, Layout, LayoutContext, Node, OutputSpec, Params, Part, ProcessContext, Signal,
    TagRule,
};

/// Separate signals → one interleaved signal, a channel each: R, G, B into RGB, L, R into
/// stereo. The inverse of [`super::Split`]. The output has as many channels as the last
/// connected input's number; unconnected inputs before it are silence.
#[derive(Debug)]
pub struct Combine;

impl NodeKind for Combine {
    const KIND: &'static str = COMBINE;
    const SPEC: NodeSpec = NodeSpec::new("Combine Channels", Category::Structure)
        .describe("Separate signals into one interleaved signal: R, G, B into RGB, or L, R into stereo")
        .doc("Each connected input becomes one channel of the output, in order. The first input sets the size; the others are stretched to it. Three channels of video make RGB; two of audio make stereo.")
        .inputs(&[
            InputSpec::required(CHANNEL_PORTS[0], "Channel 1: red, or left. Sets the size"),
            InputSpec::optional(CHANNEL_PORTS[1], "Channel 2: green, or right"),
            InputSpec::optional(CHANNEL_PORTS[2], "Channel 3: blue"),
            InputSpec::optional(CHANNEL_PORTS[3], "Channel 4"),
            InputSpec::optional(CHANNEL_PORTS[4], "Channel 5"),
            InputSpec::optional(CHANNEL_PORTS[5], "Channel 6"),
            InputSpec::optional(CHANNEL_PORTS[6], "Channel 7"),
            InputSpec::optional(CHANNEL_PORTS[7], "Channel 8"),
        ])
        .outputs(&[OutputSpec::new("out", "The channels interleaved, pixel by pixel")
            .tag(TagRule::INHERIT.part(Part::Whole))]);

    fn new(_: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

/// How many channels the output has: up to the last connected input.
fn channel_count(connected: &[bool]) -> usize {
    connected.iter().rposition(|&c| c).map_or(1, |last| last + 1)
}

impl Node for Combine {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let first = ctx.inputs[0];
        let count = channel_count(ctx.connected) as u32;
        let mut layout =
            first.reshaped(first.width, first.height, first.samples_per_pixel * count);
        layout.tag.part = Part::Whole;
        Ok(vec![layout])
    }

    fn diagnostics(&self, ctx: &LayoutContext) -> Vec<String> {
        let first = ctx.inputs[0];
        ctx.inputs
            .iter()
            .zip(ctx.connected)
            .enumerate()
            .skip(1)
            .filter(|&(_, (layout, &connected))| connected && !layout.same_shape(&first))
            .map(|(i, (layout, _))| {
                format!(
                    "{} is {layout}, stretched to {}'s {first}",
                    CHANNEL_PORTS[i], CHANNEL_PORTS[0]
                )
            })
            .collect()
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let out = &mut outputs[0];
        let spp = inputs[0].layout.samples_per_pixel.max(1) as usize;
        let stride = out.layout.samples_per_pixel as usize;
        let count = stride / spp;
        for (c, input) in inputs.iter().take(count).enumerate() {
            for (pixel, samples) in out.data.chunks_exact_mut(stride).enumerate() {
                samples[c * spp..(c + 1) * spp]
                    .copy_from_slice(&input.data[pixel * spp..(pixel + 1) * spp]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{ChannelMap, Layout, LayoutContext, ProcessContext, Registry, Signal};

    fn combine(signals: &[Signal], connected: &[bool]) -> Signal {
        let mut node = Registry::shared()
            .create("combine", &Default::default())
            .unwrap()
            .unwrap();
        let inputs: Vec<Layout> = (0..connected.len())
            .map(|i| signals.get(i).map_or(signals[0].layout, |s| s.layout))
            .collect();
        let sources = HashMap::new();
        let layouts = node
            .output_layouts(&LayoutContext {
                inputs: &inputs,
                connected,
                sources: &sources,
                output: Layout::rgb(2, 1),
                output_count: 1,
            })
            .unwrap();
        let mut out = vec![Signal::zeros(layouts[0])];
        let silence = Signal::zeros(signals[0].layout);
        let refs: Vec<&Signal> = (0..connected.len())
            .map(|i| signals.get(i).unwrap_or(&silence))
            .collect();
        let sources = HashMap::<String, Signal>::new();
        let process = ProcessContext {
            frame: 0,
            frame_rate: 1.0,
            sources: &sources,
            params: &[],
        };
        node.process(&process, &refs, &mut out);
        out.remove(0)
    }

    #[test]
    fn interleaves_three_channels_into_pixels() {
        let mono = Layout::video(2, 1).reshaped(2, 1, 1);
        let signals = [
            Signal::from_data(mono, vec![0.1, 0.4]),
            Signal::from_data(mono, vec![0.2, 0.5]),
            Signal::from_data(mono, vec![0.3, 0.6]),
        ];
        let mut connected = [false; 8];
        connected[..3].fill(true);
        let out = combine(&signals, &connected);
        assert_eq!(out.data, [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
        assert_eq!(out.layout.tag.channels, ChannelMap::Rgb);
    }

    #[test]
    fn makes_as_many_channels_as_the_last_connected_input() {
        let mono = Layout::audio(2);
        let signals = [
            Signal::from_data(mono, vec![1.0, 2.0]),
            Signal::from_data(mono, vec![-1.0, -2.0]),
        ];
        let mut connected = [false; 8];
        connected[..2].fill(true);
        let out = combine(&signals, &connected);
        assert_eq!(out.data, [1.0, -1.0, 2.0, -2.0]);
        assert_eq!(out.layout.tag.channels, ChannelMap::Stereo);

        // A gap is silence.
        let mut connected = [false; 8];
        connected[0] = true;
        connected[2] = true;
        let out = combine(&signals[..1], &connected);
        assert_eq!(out.layout.samples_per_pixel, 3);
        assert_eq!(out.data, [1.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
    }
}
