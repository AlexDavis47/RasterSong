//! Where signals enter the graph. Every graph has one Video In and one Audio In, which read what
//! is below the graph's item on the timeline. Video Source and Audio Source nodes read anything
//! else (the mix, a track's stem, another graph track's output), picked on the node itself.

use crate::nodes::{
    AUDIO_INPUT, AUDIO_SOURCE, Category, DEFAULT_AUDIO, DEFAULT_VIDEO, NodeKind, NodeSpec,
    OUTPUT_PARAM, SOURCE_PARAM, VIDEO_INPUT, VIDEO_SOURCE, source_key,
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

/// The graph's main picture: what is below its item, as RGB in `0..=1`. Every graph has one.
#[derive(Debug)]
pub struct VideoInput(Source);

impl NodeKind for VideoInput {
    const KIND: &'static str = VIDEO_INPUT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Input)
        .inputs(&[])
        .outputs(&[OutputSpec::new("out").tag(TagRule::VIDEO)])
        .fixed();

    fn new(_params: &Params) -> Result<Self, String> {
        Ok(Self(Source {
            name: DEFAULT_VIDEO.to_owned(),
        }))
    }
}

source_node!(VideoInput);

/// The graph's main sound: what is below its item, one frame's worth of samples per block, in
/// `-1..=1`, with the channels interleaved (L, R, L, R, … for stereo). Every graph has one.
#[derive(Debug)]
pub struct AudioInput(Source);

impl NodeKind for AudioInput {
    const KIND: &'static str = AUDIO_INPUT;
    const SPEC: NodeSpec = NodeSpec::new(Category::Input)
        .inputs(&[])
        .outputs(&[OutputSpec::new("out").tag(TagRule::AUDIO)])
        .fixed();

    fn new(_params: &Params) -> Result<Self, String> {
        Ok(Self(Source {
            name: DEFAULT_AUDIO.to_owned(),
        }))
    }
}

source_node!(AudioInput);

/// A picture from elsewhere in the project, as RGB in `0..=1`: the video mix, a video track, or
/// a graph track's output, named by `source` (and `output`, for a graph track).
#[derive(Debug)]
pub struct VideoSource(Source);

params! { VideoSource {
    SOURCE: ParamSpec::text(SOURCE_PARAM, ""),
    OUTPUT: ParamSpec::text(OUTPUT_PARAM, ""),
} }

impl NodeKind for VideoSource {
    const KIND: &'static str = VIDEO_SOURCE;
    const SPEC: NodeSpec = NodeSpec::new(Category::Input)
        .params(Self::PARAMS)
        .inputs(&[])
        .outputs(&[OutputSpec::new("out").tag(TagRule::VIDEO)]);

    fn new(params: &Params) -> Result<Self, String> {
        let (source, output) = (params.text_at(Self::SOURCE)?, params.text_at(Self::OUTPUT)?);
        Ok(Self(Source {
            name: source_key(true, &source, &output),
        }))
    }
}

source_node!(VideoSource);

/// Sound from elsewhere in the project: the audio mix, an audio track, or a graph track's
/// output, named by `source` (and `output`, for a graph track).
#[derive(Debug)]
pub struct AudioSource(Source);

params! { AudioSource {
    SOURCE: ParamSpec::text(SOURCE_PARAM, ""),
    OUTPUT: ParamSpec::text(OUTPUT_PARAM, ""),
} }

impl NodeKind for AudioSource {
    const KIND: &'static str = AUDIO_SOURCE;
    const SPEC: NodeSpec = NodeSpec::new(Category::Input)
        .params(Self::PARAMS)
        .inputs(&[])
        .outputs(&[OutputSpec::new("out").tag(TagRule::AUDIO)]);

    fn new(params: &Params) -> Result<Self, String> {
        let (source, output) = (params.text_at(Self::SOURCE)?, params.text_at(Self::OUTPUT)?);
        Ok(Self(Source {
            name: source_key(false, &source, &output),
        }))
    }
}

source_node!(AudioSource);
