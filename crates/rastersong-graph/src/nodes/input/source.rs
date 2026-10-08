//! Input ports: where signals enter the graph. Each reads the port it names, and whoever uses the
//! graph fills the ports: as an FX, the host track fills the main ports (`Video`, `Audio`) and
//! receives fill the others.

use crate::nodes::{
    AUDIO_INPUT, Category, DEFAULT_AUDIO, DEFAULT_VIDEO, NodeKind, NodeSpec, PORT_PARAM,
    VIDEO_INPUT,
};
use crate::{Layout, LayoutContext, Node, OutputSpec, ParamSpec, Params, TagRule};
use crate::{ProcessContext, Signal};

/// Reads a port by name.
#[derive(Debug)]
struct Source {
    name: String,
}

impl Source {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        ctx.sources
            .get(&self.name)
            .map(|&layout| vec![layout])
            .ok_or_else(|| format!("nothing fills the port `{}`", self.name))
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

/// An input port that reads a picture, as RGB in `0..=1`.
#[derive(Debug)]
pub struct VideoInput(Source);

params! { VideoInput {
    PORT: ParamSpec::text(PORT_PARAM, DEFAULT_VIDEO),
} }

impl NodeKind for VideoInput {
    const KIND: &'static str = VIDEO_INPUT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Input)
        .params(Self::PARAMS)
        .inputs(&[])
        .outputs(&[OutputSpec::new("out").tag(TagRule::VIDEO)]);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self(Source {
            name: params.text_at(Self::PORT)?,
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

/// An input port that reads sound: one frame's worth of samples per block, in `-1..=1`, with the
/// channels interleaved (L, R, L, R, … for stereo).
#[derive(Debug)]
pub struct AudioInput(Source);

params! { AudioInput {
    PORT: ParamSpec::text(PORT_PARAM, DEFAULT_AUDIO),
} }

impl NodeKind for AudioInput {
    const KIND: &'static str = AUDIO_INPUT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Input)
        .params(Self::PARAMS)
        .inputs(&[])
        .outputs(&[OutputSpec::new("out").tag(TagRule::AUDIO)]);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self(Source {
            name: params.text_at(Self::PORT)?,
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
