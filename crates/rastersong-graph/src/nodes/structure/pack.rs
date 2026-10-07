use rastersong_lang::{tr_args};
use crate::nodes::{Category, MAX_CHANNELS, NodeKind, NodeSpec};
use crate::{
    InputSpec, Layout, LayoutContext, Node, OutputSpec, ParamSpec, Params, ProcessContext, Signal,
};

/// A packed mono carrier → an interleaved signal (RGB by default). The inverse of
/// [`super::Interleave`].
#[derive(Debug)]
pub struct Pack {
    channels: u32,
}

params! { Pack {
    CHANNELS: ParamSpec::number("channels", 3.0,
        1.0,
        MAX_CHANNELS as f64).integer()
    .fixed()
    .limits(1.0, 64.0),
} }

impl NodeKind for Pack {
    const KIND: &'static str = "pack";
    const SPEC: NodeSpec = NodeSpec::new(Category::Structure)
        .params(Self::PARAMS)
        .inputs(&[InputSpec::required("in")])
        .outputs(&[OutputSpec::new("out")]);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            channels: params.number_at(Self::CHANNELS)?.round().max(1.0) as u32,
        })
    }
}

impl Node for Pack {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        let n = self.channels;
        let samples_per_row = input.width * input.samples_per_pixel;
        // A row has to hold whole pixels; that's a fact about the shape, not the signal's type.
        if !samples_per_row.is_multiple_of(n) {
            return Err(tr_args(
                "error.pack.row",
                &[
                    ("input", &input.to_string()),
                    ("samples", &samples_per_row.to_string()),
                    ("channels", &n.to_string()),
                ],
            ));
        }
        Ok(vec![input.reshaped(samples_per_row / n, input.height, n)])
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        outputs[0].data.copy_from_slice(&inputs[0].data);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{ChannelMap, Layout, LayoutContext, ParamValue, Registry};

    #[test]
    fn needs_rows_that_divide_into_pixels() {
        let sources = HashMap::new();
        let layout = |channels: f64, input: Layout| {
            let params = [("channels".to_owned(), ParamValue::Number(channels))].into();
            let node = Registry::shared().create("pack", &params).unwrap().unwrap();
            node.output_layouts(&LayoutContext {
                inputs: &[input],
                connected: &[true],
                sources: &sources,
                output: Layout::rgb(2, 2),
                layout: Default::default(),
                output_count: 1,
            })
        };
        let rgb = layout(3.0, Layout::video(6, 2).reshaped(6, 2, 1)).unwrap()[0];
        assert!(rgb.same_shape(&Layout::rgb(2, 2)));
        assert_eq!(rgb.tag.channels, ChannelMap::Rgb);
        assert!(layout(3.0, Layout::mono(7, 2)).is_err());
        let stereo = layout(2.0, Layout::audio(8)).unwrap()[0];
        assert!(stereo.same_shape(&Layout::audio_channels(4, 2)));
        assert_eq!(stereo.tag.channels, ChannelMap::Stereo);
    }
}
