//! Source nodes: where host-supplied signals enter the graph.

use crate::nodes::{
    AUDIO_INPUT, Category, DEFAULT_AUDIO, DEFAULT_VIDEO, NodeKind, NodeSpec, SOURCE_PARAM,
    VIDEO_INPUT,
};
use crate::{Layout, LayoutContext, Node, OutputSpec, ParamSpec, Params, PortHint};
use crate::{ProcessContext, Signal};

/// Reads a host-supplied signal by name.
#[derive(Debug)]
struct Source {
    name: String,
}

impl Source {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        ctx.sources
            .get(&self.name)
            .map(|&layout| vec![layout])
            .ok_or_else(|| format!("the host provides no source named `{}`", self.name))
    }

    fn process(&self, ctx: &ProcessContext, outputs: &mut [Signal]) {
        // The graph checks that every source is present with the right layout before processing.
        let source = ctx
            .sources
            .get(&self.name)
            .expect("source checked by the graph");
        outputs[0].data.copy_from_slice(&source.data);
    }
}

/// Reads the video, as RGB in `0..=1`.
#[derive(Debug)]
pub struct VideoInput(Source);

params! { VideoInput {
    SOURCE: ParamSpec::text(
        SOURCE_PARAM,
        "Source",
        DEFAULT_VIDEO,
        "Name of the host-supplied video signal",
    ),
} }

impl NodeKind for VideoInput {
    const KIND: &'static str = VIDEO_INPUT;
    const SPEC: NodeSpec = NodeSpec::new("Video", Category::Input)
        .describe("The video as RGB, 0 to 1")
        .params(Self::PARAMS)
        .inputs(&[])
        .outputs(&[OutputSpec::new("out", "The video, as RGB from 0 to 1").hint(PortHint::Rgb)]);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self(Source {
            name: params.text_at(Self::SOURCE)?,
        }))
    }
}

impl Node for VideoInput {
    fn source(&self) -> Option<&str> {
        Some(&self.0.name)
    }

    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        self.0.output_layouts(ctx)
    }

    fn process(&mut self, ctx: &ProcessContext, _inputs: &[&Signal], outputs: &mut [Signal]) {
        self.0.process(ctx, outputs);
    }
}

/// Reads an audio track: one frame's worth of mono samples per block, in `-1..=1`.
#[derive(Debug)]
pub struct AudioInput(Source);

params! { AudioInput {
    SOURCE: ParamSpec::text(
        SOURCE_PARAM,
        "Source",
        DEFAULT_AUDIO,
        "Name of the host-supplied audio signal",
    ),
} }

impl NodeKind for AudioInput {
    const KIND: &'static str = AUDIO_INPUT;
    const SPEC: NodeSpec = NodeSpec::new("Audio", Category::Input)
        .describe("The audio track, one frame's worth per block, -1 to 1")
        .params(Self::PARAMS)
        .inputs(&[])
        .outputs(&[
            OutputSpec::new("out", "The audio, one frame's worth per block, from -1 to 1")
                .hint(PortHint::Audio),
        ]);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self(Source {
            name: params.text_at(Self::SOURCE)?,
        }))
    }
}

impl Node for AudioInput {
    fn source(&self) -> Option<&str> {
        Some(&self.0.name)
    }

    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        self.0.output_layouts(ctx)
    }

    fn process(&mut self, ctx: &ProcessContext, _inputs: &[&Signal], outputs: &mut [Signal]) {
        self.0.process(ctx, outputs);
    }
}
