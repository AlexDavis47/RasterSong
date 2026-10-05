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
    const SPEC: NodeSpec = NodeSpec::new("Audio Output", Category::Output)
        .describe("The rendered sound: replaces the source audio in the preview and the export")
        .doc("Optional. Without it, or with nothing connected, the source audio is used untouched; so is a track wired straight in. Mono and stereo signals are written as they are; any other signal is written as interleaved samples to a stereo track. Each frame's block is resampled from its own rate to the project's audio rate, and clipped to -1 to 1.")
        .inputs(&[InputSpec::required(
            "in",
            "The sound to output, from -1 to 1: mono, stereo, or any signal read as samples",
        )])
        .outputs(&[OutputSpec::new("out", "The sound as written").tag(TagRule::AUDIO)])
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
            n => vec![Diagnostic::note(format!(
                "{n} samples per pixel are read as one long stereo stream, so expect a raw, buzzy sound."
            ))],
        }
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        outputs[0].data.copy_from_slice(&inputs[0].data);
    }
}
