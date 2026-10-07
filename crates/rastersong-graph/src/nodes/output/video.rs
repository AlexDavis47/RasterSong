use crate::Interpolation;
use crate::dsp::Stretcher;
use crate::nodes::{Category, NodeKind, NodeSpec, OUTPUT};
use crate::{
    InputSpec, Layout, LayoutContext, Node, ParamSpec, Params, ProcessContext, Range, Signal,
    TagRule,
};
use rastersong_lang::tr_args;

/// The graph's result. Accepts an RGB signal of the output size, or a mono one, which is shown
/// as grayscale. With `stretch` on (the default) any other signal is stretched to the output
/// size, as if it were wired to the video input, so the output shows something whatever the
/// signal's layout; with it off, a signal of the wrong size is an error.
#[derive(Debug)]
pub struct Output {
    stretch: bool,
    stretcher: Stretcher,
}

params! { Output {
    STRETCH: ParamSpec::choice("stretch", &["on", "off"], "on"),
} }

impl NodeKind for Output {
    const KIND: &'static str = OUTPUT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Output)
        .params(Self::PARAMS)
        .inputs(&[InputSpec::required("in")])
        .outputs(&[crate::OutputSpec::new("out").tag(TagRule::VIDEO)])
        .expects(Range::Unipolar);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            stretch: params.choice_at(Self::STRETCH)? == "on",
            stretcher: Stretcher::default(),
        })
    }
}

impl Node for Output {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        let size_matches = (input.width, input.height) == (ctx.output.width, ctx.output.height);
        if self.stretch || (size_matches && matches!(input.samples_per_pixel, 1 | 3)) {
            Ok(vec![ctx.output])
        } else {
            Err(tr_args(
                "error.video_output.shape",
                &[
                    ("expected", &ctx.output.to_string()),
                    ("got", &input.to_string()),
                ],
            ))
        }
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = inputs[0];
        let output = &mut outputs[0];
        let fits = input.layout.width == output.layout.width
            && input.layout.height == output.layout.height;
        match input.layout.samples_per_pixel {
            3 if fits => output.data.copy_from_slice(&input.data),
            1 if fits => {
                for (pixel, &value) in output.data.chunks_mut(3).zip(&input.data) {
                    pixel.fill(value);
                }
            }
            spp => self.stretcher.stretch(
                &input.data,
                spp as usize,
                &mut output.data,
                output.layout.samples_per_pixel as usize,
                Interpolation::Hold,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{Layout, LayoutContext, Node, ProcessContext, Registry, Signal};

    fn output_with(params: &str) -> Box<dyn Node> {
        Registry::shared()
            .create("output", &serde_json::from_str(params).unwrap())
            .unwrap()
            .unwrap()
    }

    fn output() -> Box<dyn Node> {
        output_with("{}")
    }

    fn process(node: &mut dyn Node, input: Signal, out: Layout) -> Vec<f32> {
        let mut outputs = vec![Signal::zeros(out)];
        let sources = HashMap::<String, Signal>::new();
        let ctx = ProcessContext {
            frame: 0,
            frame_rate: 1.0,
            sources: &sources,
            params: &[],
        };
        node.process(&ctx, &[&input], &mut outputs);
        outputs.remove(0).data
    }

    #[test]
    fn mono_is_shown_as_gray() {
        let mut node = output();
        let input = Signal::from_data(Layout::mono(2, 1), vec![0.25, 0.75]);
        let out = process(node.as_mut(), input, Layout::rgb(2, 1));
        assert_eq!(out, [0.25, 0.25, 0.25, 0.75, 0.75, 0.75]);
    }

    #[test]
    fn other_signals_are_stretched_over_the_picture() {
        let mut node = output();
        // A 2x1 picture from four audio samples: held, each pixel takes the nearest sample.
        let input = Signal::from_data(Layout::audio(4), vec![0.0, 0.25, 0.5, 1.0]);
        let out = process(node.as_mut(), input, Layout::rgb(2, 1));
        assert_eq!(out.len(), 6);
        assert!(out[..3].iter().all(|&v| v == out[0]));
        assert!(out[3] > out[0], "{out:?}");
    }

    #[test]
    fn an_rgb_picture_of_another_size_keeps_its_channels() {
        let mut node = output();
        // Two pixels, red then blue, stretched to four.
        let input = Signal::from_data(Layout::rgb(2, 1), vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let out = process(node.as_mut(), input, Layout::rgb(4, 1));
        assert_eq!(out[..3], [1.0, 0.0, 0.0]);
        assert_eq!(out[9..], [0.0, 0.0, 1.0]);
    }

    #[test]
    fn without_stretch_the_wrong_size_is_rejected() {
        let node = output_with(r#"{ "stretch": "off" }"#);
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

    #[test]
    fn with_stretch_any_layout_is_accepted() {
        let node = output();
        let sources = HashMap::new();
        let inputs = [Layout::audio(100)];
        let layout = node
            .output_layouts(&LayoutContext {
                inputs: &inputs,
                connected: &[true],
                sources: &sources,
                output: Layout::rgb(4, 2),
                layout: Default::default(),
                output_count: 1,
            })
            .unwrap();
        assert!(layout[0].same_shape(&Layout::rgb(4, 2)));
    }
}
