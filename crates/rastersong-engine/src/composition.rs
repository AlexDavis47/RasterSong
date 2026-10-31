//! What the renderer composes: the video and audio tracks, the graph tracks above them, and the
//! graphs their items use. See [Composition](../../../docs/engine.md#composition).

use std::collections::BTreeMap;

use rastersong_graph::nodes::{
    AUDIO_INPUT, AUDIO_SOURCE, DEFAULT_AUDIO, DEFAULT_VIDEO, OUTPUT_PARAM, SOURCE_PARAM,
    VIDEO_INPUT, VIDEO_SOURCE, source_key,
};
use rastersong_graph::{GraphDesc, ParamValue};

use crate::timeline::{GraphItem, Source, TrackKind};

/// Whether an input node reads pictures or sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Video,
    Audio,
}

/// What one input node of a graph reads, and the name the graph is filled by for it. Several
/// nodes reading the same source share it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputPort {
    /// The name the host fills: `Video` or `Audio` for the main inputs.
    pub key: String,
    pub kind: InputKind,
    pub source: Source,
}

/// What a node of type `kind` with `params` reads; `None` for nodes other than inputs.
pub fn port_of(kind: &str, params: &BTreeMap<String, ParamValue>) -> Option<InputPort> {
    let (kind, key, source) = match kind {
        VIDEO_INPUT => (InputKind::Video, DEFAULT_VIDEO.to_owned(), Source::Below),
        AUDIO_INPUT => (InputKind::Audio, DEFAULT_AUDIO.to_owned(), Source::Below),
        VIDEO_SOURCE | AUDIO_SOURCE => {
            let video = kind == VIDEO_SOURCE;
            let text = |name: &str| match params.get(name) {
                Some(ParamValue::Text(text)) => text.as_str(),
                _ => "",
            };
            let (source, output) = (text(SOURCE_PARAM), text(OUTPUT_PARAM));
            let kind = if video {
                InputKind::Video
            } else {
                InputKind::Audio
            };
            (
                kind,
                source_key(video, source, output),
                Source::from_settings(source, output, video),
            )
        }
        _ => return None,
    };
    Some(InputPort { key, kind, source })
}

/// What graph `desc`'s input nodes read, each once, in the order their first nodes come.
pub fn input_ports(desc: &GraphDesc) -> Vec<InputPort> {
    let mut ports: Vec<InputPort> = Vec::new();
    for port in desc
        .nodes
        .iter()
        .filter_map(|n| port_of(&n.kind, &n.params))
    {
        if !ports.iter().any(|p| p.key == port.key) {
            ports.push(port);
        }
    }
    ports
}

/// A video or audio track as the renderer composes it.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaTrack {
    pub name: String,
    pub kind: TrackKind,
    /// Its level in the audio mix (a video track's is ignored).
    pub volume: f32,
    /// Whether it is in the mix: not muted, and not left out by a solo. Graphs read its stem
    /// either way.
    pub in_mix: bool,
    /// The bus an audio track is summed into.
    pub bus: String,
}

impl MediaTrack {
    /// A track of `kind` in the mix at full level, on the master bus.
    pub fn new(name: impl Into<String>, kind: TrackKind) -> Self {
        Self {
            name: name.into(),
            kind,
            volume: 1.0,
            in_mix: true,
            bus: rastersong_graph::nodes::DEFAULT_BUS.to_owned(),
        }
    }
}

/// A graph track as the renderer composes it: the items that play (muted ones and muted tracks
/// are left out).
#[derive(Debug, Clone, PartialEq)]
pub struct ComposedGraphTrack {
    pub name: String,
    pub items: Vec<GraphItem>,
}

/// What the renderer composes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Composition {
    /// Video tracks top first, then audio tracks.
    pub tracks: Vec<MediaTrack>,
    /// Graph tracks, top first: the topmost active item's outputs are the master.
    pub graph_tracks: Vec<ComposedGraphTrack>,
    /// Every graph an item may use, by id, but the open one, which comes separately because it
    /// changes as it is edited.
    pub graphs: Vec<(u32, GraphDesc)>,
    /// The id of the open graph.
    pub open: u32,
}

impl Composition {
    /// The composition with every graph track left out: the plain mix.
    pub fn without_graphs(mut self) -> Self {
        self.graph_tracks.clear();
        self
    }

    /// Whether any graph item plays.
    pub fn has_graphs(&self) -> bool {
        self.graph_tracks.iter().any(|t| !t.items.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_read_below_and_sources_what_they_name() {
        let desc = GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [
                { "id": "v", "type": "video_input" },
                { "id": "a", "type": "audio_input" },
                { "id": "k", "type": "audio_source", "params": { "source": "track:Kick" } },
                { "id": "k2", "type": "audio_source", "params": { "source": "track:Kick" } },
                { "id": "kv", "type": "video_source", "params": { "source": "track:Kick" } },
                { "id": "g", "type": "video_source",
                  "params": { "source": "graph:FX", "output": "Mask" } },
                { "id": "m", "type": "audio_source", "params": { "source": "mix" } },
                { "id": "n", "type": "video_source" },
                { "id": "o", "type": "output" } ] }"#,
        )
        .unwrap();
        let sources: Vec<(InputKind, Source)> = input_ports(&desc)
            .into_iter()
            .map(|p| (p.kind, p.source))
            .collect();
        assert_eq!(
            sources,
            [
                (InputKind::Video, Source::Below),
                (InputKind::Audio, Source::Below),
                (InputKind::Audio, Source::Track("Kick".into())),
                (InputKind::Video, Source::Track("Kick".into())),
                (
                    InputKind::Video,
                    Source::Graph {
                        track: "FX".into(),
                        port: "Mask".into()
                    }
                ),
                (InputKind::Audio, Source::Mix),
                (InputKind::Video, Source::Nothing),
            ]
        );
    }

    #[test]
    fn source_settings_round_trip() {
        for (source, video) in [
            (Source::Mix, true),
            (Source::Track("a:b".into()), false),
            (
                Source::Graph {
                    track: "FX".into(),
                    port: "Video".into(),
                },
                true,
            ),
            (
                Source::Graph {
                    track: "FX".into(),
                    port: "Mask".into(),
                },
                false,
            ),
            (Source::Nothing, true),
        ] {
            let (s, o) = source.settings(video);
            assert_eq!(Source::from_settings(&s, &o, video), source);
        }
        let (_, output) = Source::Graph {
            track: "FX".into(),
            port: "Video".into(),
        }
        .settings(true);
        assert_eq!(output, "", "the main output is left out");
    }
}
