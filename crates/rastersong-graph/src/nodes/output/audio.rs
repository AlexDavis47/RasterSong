use crate::dsp::db_to_gain;
use crate::nodes::{AUDIO_OUTPUT, Category, Meter, NodeKind, NodeSpec};
use crate::{
    Diagnostic, InputSpec, LayoutContext, Node, OutputSpec, ParamSpec, Params, ProcessContext,
    Range, Signal, TagRule,
};
use rastersong_lang::tr_args;

/// The graph's sound, written to the export and played in the preview instead of the source
/// audio. Optional: without one, the source audio is used untouched.
///
/// Accepts any signal. One sample per pixel is mono and two are stereo; anything else is written
/// as interleaved samples to a stereo track. Each block is resampled to the project's audio rate
/// from its own rate (its length per frame), and clipped to `-1..1` on the way out.
#[derive(Debug)]
pub struct AudioOutput {
    gain: f32,
    /// The largest sample of the last frame, after the volume and before clipping.
    peak: f32,
}

params! { AudioOutput {
    VOLUME: ParamSpec::number("volume", 0.0,
        -48.0,
        12.0)
    .unit("dB")
    .limits(-120.0, 60.0),
} }

impl NodeKind for AudioOutput {
    const KIND: &'static str = AUDIO_OUTPUT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Output)
        .params(Self::PARAMS)
        .meters(&[Meter::level("peak")])
        .inputs(&[InputSpec::required("in")])
        .outputs(&[OutputSpec::new("out").tag(TagRule::AUDIO)])
        .expects(Range::Bipolar)
        .addable();

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            gain: db_to_gain(params.number_at(Self::VOLUME)?) as f32,
            peak: 0.0,
        })
    }
}

impl Node for AudioOutput {
    fn meters(&self, out: &mut [f32]) {
        out[0] = self.peak;
    }

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
        let mut peak = 0.0f32;
        for (out, &x) in outputs[0].data.iter_mut().zip(&inputs[0].data) {
            *out = x * self.gain;
            peak = peak.max(out.abs());
        }
        self.peak = peak;
    }
}
