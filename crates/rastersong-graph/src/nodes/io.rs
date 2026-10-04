//! Source and output nodes: where signals enter and leave the graph.

use crate::desc::Params;
use crate::{InputSpec, Layout, LayoutContext, Node, ProcessContext, Signal};

/// Reads a host-supplied signal, e.g. `"video"` (RGB, `0..=1`) or `"audio"` (mono, `-1..=1`).
#[derive(Debug)]
pub struct SourceNode {
    name: String,
}

impl SourceNode {
    pub fn video(params: &mut Params) -> Result<Self, String> {
        Ok(Self {
            name: params.text("source", "video")?,
        })
    }

    pub fn audio(params: &mut Params) -> Result<Self, String> {
        Ok(Self {
            name: params.text("source", "audio")?,
        })
    }
}

impl Node for SourceNode {
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

/// The graph's result. Accepts an RGB signal of the output size, or a mono one, which is shown
/// as grayscale.
#[derive(Debug)]
pub struct Output;

impl Node for Output {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        let size_matches = (input.width, input.height) == (ctx.output.width, ctx.output.height);
        if size_matches && matches!(input.samples_per_pixel, 1 | 3) {
            Ok(vec![ctx.output])
        } else {
            Err(format!(
                "expects a {} or mono signal of that size, got {input}",
                ctx.output
            ))
        }
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = inputs[0];
        if input.layout.samples_per_pixel == 3 {
            outputs[0].data.copy_from_slice(&input.data);
        } else {
            for (pixel, &value) in outputs[0].data.chunks_mut(3).zip(&input.data) {
                pixel.fill(value);
            }
        }
    }
}
