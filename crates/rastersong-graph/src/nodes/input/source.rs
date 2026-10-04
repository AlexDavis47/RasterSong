//! Source nodes: where host-supplied signals enter the graph.

use crate::nodes::{Category, NodeSpec};
use crate::{Layout, LayoutContext, Node, PortHint, ProcessContext, Signal};
use crate::{ParamSpec, Params};

/// Reads a host-supplied signal, e.g. `"video"` (RGB, `0..=1`) or `"audio"` (mono, `-1..=1`).
#[derive(Debug)]
pub struct SourceNode {
    name: String,
    hint: &'static [PortHint],
}

impl SourceNode {
    pub const VIDEO_PARAMS: &[ParamSpec] = &[ParamSpec::text(
        "source",
        "Source",
        "video",
        "Name of the host-supplied video signal",
    )];
    pub const AUDIO_PARAMS: &[ParamSpec] = &[ParamSpec::text(
        "source",
        "Source",
        "audio",
        "Name of the host-supplied audio signal",
    )];

    pub const VIDEO_SPEC: NodeSpec = NodeSpec::new("Video", Category::Input)
        .describe("The video as RGB, 0 to 1")
        .params(Self::VIDEO_PARAMS);
    pub const AUDIO_SPEC: NodeSpec = NodeSpec::new("Audio", Category::Input)
        .describe("The audio track, one frame's worth per block, -1 to 1")
        .params(Self::AUDIO_PARAMS);

    /// A video source (RGB).
    pub fn video(params: &Params) -> Result<Self, String> {
        Ok(Self {
            name: params.text("source")?,
            hint: &[PortHint::Rgb],
        })
    }

    /// An audio source.
    pub fn audio(params: &Params) -> Result<Self, String> {
        Ok(Self {
            name: params.text("source")?,
            hint: &[PortHint::Audio],
        })
    }
}

impl Node for SourceNode {
    fn output_hints(&self) -> &'static [PortHint] {
        self.hint
    }

    fn source(&self) -> Option<&str> {
        Some(&self.name)
    }

    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        ctx.sources
            .get(&self.name)
            .map(|&layout| vec![layout])
            .ok_or_else(|| format!("the host provides no source named `{}`", self.name))
    }

    fn process(&mut self, ctx: &ProcessContext, _inputs: &[&Signal], outputs: &mut [Signal]) {
        // The graph checks that every source is present with the right layout before processing.
        let source = ctx
            .sources
            .get(&self.name)
            .expect("source checked by the graph");
        outputs[0].data.copy_from_slice(&source.data);
    }
}
