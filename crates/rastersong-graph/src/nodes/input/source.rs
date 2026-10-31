//! Where signals enter the graph. A Video Input or Audio Input reads what its `source` names:
//! what is below the graph's item (the default), the mix, a track's stem or a graph track's main
//! output. A Named Input reads a Named Output of a graph track by its name.

use crate::nodes::{
    AUDIO_INPUT, BELOW, Category, NAME_PARAM, NAMED_INPUT, NodeKind, NodeSpec, SOURCE_PARAM,
    TRACK_PARAM, VIDEO_INPUT, named_key, source_key,
};
use crate::{Layout, LayoutContext, Node, OutputSpec, ParamSpec, Params, TagRule};
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
            .ok_or_else(|| format!("nothing fills the source `{}`", self.name))
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

/// Implements [`Node`] for a node wrapping a [`Source`].
macro_rules! source_node {
    ($ty:ident) => {
        impl Node for $ty {
            fn source(&self) -> Option<&str> {
                Some(&self.0.name)
            }

            fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
                self.0.output_layouts(ctx)
            }

            fn process(
                &mut self,
                ctx: &ProcessContext,
                _inputs: &[&Signal],
                outputs: &mut [Signal],
            ) {
                self.0.process(ctx, outputs);
            }
        }
    };
}

/// A picture, as RGB in `0..=1`: by default what is below the graph's item, else the video mix,
/// a video track or a graph track's main output, named by `source`.
#[derive(Debug)]
pub struct VideoInput(Source);

params! { VideoInput {
    SOURCE: ParamSpec::text(SOURCE_PARAM, BELOW),
} }

impl NodeKind for VideoInput {
    const KIND: &'static str = VIDEO_INPUT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Input)
        .params(Self::PARAMS)
        .inputs(&[])
        .outputs(&[OutputSpec::new("out").tag(TagRule::VIDEO)]);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self(Source {
            name: source_key(true, &params.text_at(Self::SOURCE)?),
        }))
    }
}

source_node!(VideoInput);

/// Sound, one frame's worth of samples per block, in `-1..=1`, with the channels interleaved
/// (L, R, L, R, … for stereo): by default what is below the graph's item, else the audio mix, an
/// audio track or a graph track's main output, named by `source`.
#[derive(Debug)]
pub struct AudioInput(Source);

params! { AudioInput {
    SOURCE: ParamSpec::text(SOURCE_PARAM, BELOW),
} }

impl NodeKind for AudioInput {
    const KIND: &'static str = AUDIO_INPUT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Input)
        .params(Self::PARAMS)
        .inputs(&[])
        .outputs(&[OutputSpec::new("out").tag(TagRule::AUDIO)]);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self(Source {
            name: source_key(false, &params.text_at(Self::SOURCE)?),
        }))
    }
}

source_node!(AudioInput);

/// What a Named Output called `name` on graph track `track` sends, as it is sent.
#[derive(Debug)]
pub struct NamedInput(Source);

params! { NamedInput {
    TRACK: ParamSpec::text(TRACK_PARAM, ""),
    NAME: ParamSpec::text(NAME_PARAM, ""),
} }

impl NodeKind for NamedInput {
    const KIND: &'static str = NAMED_INPUT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Input)
        .params(Self::PARAMS)
        .inputs(&[])
        .outputs(&[OutputSpec::new("out")]);

    fn new(params: &Params) -> Result<Self, String> {
        let (track, name) = (params.text_at(Self::TRACK)?, params.text_at(Self::NAME)?);
        Ok(Self(Source {
            name: named_key(&track, &name),
        }))
    }
}

source_node!(NamedInput);
