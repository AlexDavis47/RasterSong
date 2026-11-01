//! What the renderer composes: the video and audio tracks, the graph tracks above them, and the
//! graphs their items use. See [Composition](../../../docs/engine.md#composition).

use std::collections::BTreeMap;

use rastersong_graph::nodes::{
    AUDIO_INPUT, BELOW, NAME_PARAM, NAMED_INPUT, SOURCE_PARAM, TRACK_PARAM, VIDEO_INPUT, named_key,
    source_key,
};
use rastersong_graph::{GraphDesc, ParamValue};

use crate::project::ResourceId;
use crate::timeline::{GraphItem, Source, TrackKind};

/// Whether an input node reads pictures or sound, or (a Named Input) whatever was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Video,
    Audio,
    Named,
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
    let text = |name: &str| match params.get(name) {
        Some(ParamValue::Text(text)) => text.as_str(),
        _ => "",
    };
    let (kind, key, source) = match kind {
        VIDEO_INPUT | AUDIO_INPUT => {
            let video = kind == VIDEO_INPUT;
            // A missing setting is the default: what is below.
            let source = params
                .get(SOURCE_PARAM)
                .map_or(BELOW, |_| text(SOURCE_PARAM));
            (
                if video {
                    InputKind::Video
                } else {
                    InputKind::Audio
                },
                source_key(video, source),
                Source::from_setting(source),
            )
        }
        NAMED_INPUT => {
            let (track, name) = (text(TRACK_PARAM), text(NAME_PARAM));
            let source = if track.is_empty() || name.is_empty() {
                Source::Nothing
            } else {
                Source::Named {
                    track: track.to_owned(),
                    name: name.to_owned(),
                }
            };
            (InputKind::Named, named_key(track, name), source)
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

/// A graph an item may use: a graph resource.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposedGraph {
    pub id: ResourceId,
    /// What errors call it.
    pub name: String,
    pub desc: GraphDesc,
}

/// What the renderer composes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Composition {
    /// Video tracks top first, then audio tracks.
    pub tracks: Vec<MediaTrack>,
    /// Graph tracks, top first: the topmost active item's outputs are the master.
    pub graph_tracks: Vec<ComposedGraphTrack>,
    /// Every graph an item may use.
    pub graphs: Vec<ComposedGraph>,
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
    fn inputs_read_what_their_source_names() {
        let desc = GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [
                { "id": "v", "type": "video_input" },
                { "id": "a", "type": "audio_input", "params": { "source": "below" } },
                { "id": "k", "type": "audio_input", "params": { "source": "track:Kick" } },
                { "id": "k2", "type": "audio_input", "params": { "source": "track:Kick" } },
                { "id": "kv", "type": "video_input", "params": { "source": "track:Kick" } },
                { "id": "g", "type": "video_input", "params": { "source": "graph:FX" } },
                { "id": "mask", "type": "named_input", "params": { "track": "FX", "name": "Mask" } },
                { "id": "m", "type": "audio_input", "params": { "source": "mix" } },
                { "id": "n", "type": "video_input", "params": { "source": "" } },
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
                (InputKind::Video, Source::Graph("FX".into())),
                (
                    InputKind::Named,
                    Source::Named {
                        track: "FX".into(),
                        name: "Mask".into()
                    }
                ),
                (InputKind::Audio, Source::Mix),
                (InputKind::Video, Source::Nothing),
            ]
        );
    }

    #[test]
    fn source_settings_round_trip() {
        for source in [
            Source::Below,
            Source::Mix,
            Source::Track("a:b".into()),
            Source::Graph("FX".into()),
            Source::Nothing,
        ] {
            assert_eq!(Source::from_setting(&source.setting()), source);
        }
    }
}
