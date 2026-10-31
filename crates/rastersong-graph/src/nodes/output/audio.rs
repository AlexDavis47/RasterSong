use crate::dsp::db_to_gain;
use crate::nodes::{
    AUDIO_BUS_OUTPUT, AUDIO_OUTPUT, BUS_PARAM, Category, Meter, NodeKind, NodeSpec,
};
use crate::{
    Diagnostic, InputSpec, LayoutContext, Node, OutputSpec, ParamSpec, Params, ProcessContext,
    Range, Signal, TagRule,
};
use rastersong_lang::tr_args;

/// Writes sound to a bus at a volume, metering its peak. Shared by both audio outputs.
///
/// Accepts any signal. One sample per pixel is mono and is written to every channel of the bus;
/// anything else is written as interleaved samples across the bus's channels. Each block is
/// resampled to the project's audio rate from its own rate (its length per frame), and clipped to
/// `-1..1` on the way out.
#[derive(Debug)]
struct Writer {
    gain: f32,
    /// The largest sample of the last frame, after the volume and before clipping.
    peak: f32,
}

impl Writer {
    fn new(volume: f64) -> Self {
        Self {
            gain: db_to_gain(volume) as f32,
            peak: 0.0,
        }
    }

    fn diagnostics(ctx: &LayoutContext) -> Vec<Diagnostic> {
        match ctx.inputs[0].samples_per_pixel {
            1 | 2 => Vec::new(),
            n => vec![Diagnostic::note(tr_args(
                "diagnostic.audio_output.many_channels",
                &[("count", &n.to_string())],
            ))],
        }
    }

    fn process(&mut self, inputs: &[&Signal], outputs: &mut [Signal]) {
        let mut peak = 0.0f32;
        for (out, &x) in outputs[0].data.iter_mut().zip(&inputs[0].data) {
            *out = x * self.gain;
            peak = peak.max(out.abs());
        }
        self.peak = peak;
    }
}

/// Implements [`Node`] for a node wrapping a [`Writer`].
macro_rules! writer_node {
    ($ty:ident) => {
        impl Node for $ty {
            fn meters(&self, out: &mut [f32]) {
                out[0] = self.0.peak;
            }

            fn diagnostics(&self, ctx: &LayoutContext) -> Vec<Diagnostic> {
                Writer::diagnostics(ctx)
            }

            fn process(
                &mut self,
                _ctx: &ProcessContext,
                inputs: &[&Signal],
                outputs: &mut [Signal],
            ) {
                self.0.process(inputs, outputs);
            }
        }
    };
}

/// The graph's main sound, on the project's master bus: what graphs above read as its sound,
/// played in the preview and written to the export instead of the master's track mix. Every
/// graph has one; unconnected, the master plays its track mix.
#[derive(Debug)]
pub struct AudioOutput(Writer);

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
        .expects(Range::Bipolar);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self(Writer::new(params.number_at(Self::VOLUME)?)))
    }
}

writer_node!(AudioOutput);

/// The graph's sound for one of the project's other output buses, written to the export of that
/// bus instead of its track mix. Optional: without one, the bus plays its track mix. A graph has
/// at most one per bus; one naming the master bus is ignored, as the Audio Output writes there.
#[derive(Debug)]
pub struct AudioBusOutput(Writer);

params! { AudioBusOutput {
    VOLUME: ParamSpec::number("volume", 0.0,
        -48.0,
        12.0)
    .unit("dB")
    .limits(-120.0, 60.0),
    BUS: ParamSpec::text(BUS_PARAM, ""),
} }

impl NodeKind for AudioBusOutput {
    const KIND: &'static str = AUDIO_BUS_OUTPUT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Output)
        .params(Self::PARAMS)
        .meters(&[Meter::level("peak")])
        .inputs(&[InputSpec::required("in")])
        .outputs(&[OutputSpec::new("out").tag(TagRule::AUDIO)])
        .expects(Range::Bipolar)
        .addable();

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self(Writer::new(params.number_at(Self::VOLUME)?)))
    }
}

writer_node!(AudioBusOutput);
