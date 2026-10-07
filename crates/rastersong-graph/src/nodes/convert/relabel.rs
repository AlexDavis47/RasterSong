use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{
    ChannelMap, Kind, Layout, LayoutContext, Node, OutputSpec, ParamSpec, Params, Part,
    ProcessContext, Range, Signal,
};

choice! {
    /// What the signal is meant to be.
    pub enum KindChoice {
        Keep = "keep",
        Video = "video",
        Audio = "audio",
        Unknown = "unknown",
    }
}

choice! {
    /// What the interleaved channels are.
    pub enum ChannelsChoice {
        Keep = "keep",
        /// The usual names for the count and kind: RGB, stereo, mono.
        Named = "named",
        /// Just numbered: three channels that aren't R, G, B.
        Numbered = "numbered",
    }
}

choice! {
    /// The nominal range.
    pub enum RangeChoice {
        Keep = "keep",
        /// `0..1`.
        Unipolar = "0 to 1",
        /// `-1..1`.
        Bipolar = "-1 to 1",
        Unknown = "unknown",
    }
}

choice! {
    /// Which part of a whole the signal is.
    pub enum PartChoice {
        Keep = "keep",
        /// No longer a channel or band of something else.
        Whole = "whole",
    }
}

/// Changes only what a signal is said to be, not its samples: for deliberately reading video as
/// audio, three channels as something other than RGB, and so on. Affects wire colours and
/// warnings, nothing else.
#[derive(Debug)]
pub struct Relabel {
    kind: KindChoice,
    channels: ChannelsChoice,
    range: RangeChoice,
    part: PartChoice,
}

params! { Relabel {
    KIND: ParamSpec::choice("kind", "Kind", KindChoice::OPTIONS, "keep", "What the signal is meant to be"),
    CHANNELS: ParamSpec::choice(
        "channels",
        "Channels",
        ChannelsChoice::OPTIONS,
        "keep",
        "named gives the channels their usual names (RGB, stereo), numbered just counts them",
    ),
    RANGE: ParamSpec::choice("range", "Range", RangeChoice::OPTIONS, "keep", "The range the values are meant to span"),
    PART: ParamSpec::choice(
        "part",
        "Part",
        PartChoice::OPTIONS,
        "keep",
        "whole stops treating the signal as one channel or band of another",
    ),
} }

impl NodeKind for Relabel {
    const KIND: &'static str = "relabel";
    const SPEC: NodeSpec = NodeSpec::new("Relabel", Category::Convert)
        .describe("Changes what a signal is said to be (kind, channels, range), not its samples")
        .doc("Signals carry a tag (video or audio, which channels, the range of values) that colours wires and drives warnings. Nothing converts a signal because of its tag; this node only rewrites it, for when a signal is reused on purpose as something else.")
        .params(Self::PARAMS)
        .outputs(&[OutputSpec::new("out", "The same samples, relabelled")]);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            kind: params.choice_as(Self::KIND)?,
            channels: params.choice_as(Self::CHANNELS)?,
            range: params.choice_as(Self::RANGE)?,
            part: params.choice_as(Self::PART)?,
        })
    }
}

impl Node for Relabel {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let mut layout = ctx.inputs[0];
        let tag = &mut layout.tag;
        match self.kind {
            KindChoice::Keep => {}
            KindChoice::Video => tag.kind = Kind::Video,
            KindChoice::Audio => tag.kind = Kind::Audio,
            KindChoice::Unknown => tag.kind = Kind::Unknown,
        }
        match self.range {
            RangeChoice::Keep => {}
            RangeChoice::Unipolar => tag.range = Range::Unipolar,
            RangeChoice::Bipolar => tag.range = Range::Bipolar,
            RangeChoice::Unknown => tag.range = Range::Unknown,
        }
        if self.part == PartChoice::Whole {
            tag.part = Part::Whole;
        }
        let spp = layout.samples_per_pixel;
        layout.tag = match self.channels {
            ChannelsChoice::Keep => layout.tag.fit(spp),
            ChannelsChoice::Named => crate::Tag {
                channels: ChannelMap::usual(spp, layout.tag.kind),
                ..layout.tag
            },
            ChannelsChoice::Numbered if spp > 1 => crate::Tag {
                channels: ChannelMap::Numbered,
                ..layout.tag
            },
            ChannelsChoice::Numbered => layout.tag.fit(spp),
        };
        Ok(vec![layout])
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        outputs[0].data.copy_from_slice(&inputs[0].data);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{ChannelMap, Kind, Layout, LayoutContext, ParamValue, Range, Registry};

    fn relabel(params: &[(&str, &str)], input: Layout) -> Layout {
        let params = params
            .iter()
            .map(|&(k, v)| (k.to_owned(), ParamValue::Text(v.to_owned())))
            .collect();
        let node = Registry::shared()
            .create("relabel", &params)
            .unwrap()
            .unwrap();
        let sources = HashMap::new();
        node.output_layouts(&LayoutContext {
            inputs: &[input],
            connected: &[true],
            sources: &sources,
            output: input,
            layout: Default::default(),
            output_count: 1,
        })
        .unwrap()[0]
    }

    #[test]
    fn rewrites_only_what_it_is_told_to() {
        let video = Layout::video(4, 2);
        assert_eq!(relabel(&[], video), video);
        let audio = relabel(&[("kind", "audio"), ("range", "-1 to 1")], video);
        assert_eq!(audio.tag.kind, Kind::Audio);
        assert_eq!(audio.tag.range, Range::Bipolar);
        // Three channels of audio aren't RGB.
        assert_eq!(audio.tag.channels, ChannelMap::Numbered);
        assert!(audio.same_shape(&video));
        let numbered = relabel(&[("channels", "numbered")], video);
        assert_eq!(numbered.tag.channels, ChannelMap::Numbered);
        assert_eq!(
            relabel(&[("channels", "named")], numbered).tag.channels,
            ChannelMap::Rgb
        );
    }
}
