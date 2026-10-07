use rastersong_lang::{tr_args};
use crate::nodes::{AUDIO_OUTPUT, Category, NodeKind, NodeSpec};
use crate::{
    Diagnostic, InputSpec, LayoutContext, Node, OutputSpec, Params, ProcessContext, Range, Signal,
    TagRule,
};

/// The graph's sound, written to the export and played in the preview instead of the source
/// audio. Optional: without one, the source audio is used untouched.
///
/// Accepts any signal. One sample per pixel is mono and two are stereo; anything else is written
/// as interleaved samples to a stereo track. Each block is resampled to the project's audio rate
/// from its own rate (its length per frame), and clipped to `-1..1` on the way out.
#[derive(Debug)]
pub struct AudioOutput;

impl NodeKind for AudioOutput {
    const KIND: &'static str = AUDIO_OUTPUT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Output)
        .inputs(&[InputSpec::required("in")])
        .outputs(&[OutputSpec::new("out").tag(TagRule::AUDIO)])
        .expects(Range::Bipolar)
        .addable();

    fn new(_: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

impl Node for AudioOutput {
    fn diagnostics(&self, ctx: &LayoutContext) -> Vec<Diagnostic> {
        match ctx.inputs[0].samples_per_pixel {
            1 | 2 => Vec::new(),
            n => vec![Diagnostic::note(tr_args(
                "diagnostic.audio_output.many_channels",
                &[("count", &n.to_string())],
            ))],
        }
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        outputs[0].data.copy_from_slice(&inputs[0].data);
    }
}
