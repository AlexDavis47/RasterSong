//! What the renderer composes: the video and audio tracks, the graph tracks above them, and the
//! graphs their items use. See [Composition](../../../docs/engine.md#composition).

use std::collections::BTreeMap;

use rastersong_graph::nodes::{AUDIO_INPUT, DEFAULT_AUDIO, DEFAULT_VIDEO, PORT_PARAM, VIDEO_INPUT};
use rastersong_graph::{GraphDesc, ParamValue};

use crate::timeline::{GraphItem, TrackKind};

/// Whether an input port reads pictures or sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Video,
    Audio,
}

/// An input port of a graph: what a graph item's sources fill, by name. Several input nodes can
/// read one port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputPort {
    pub name: String,
    /// What the port's first input node reads.
    pub kind: InputKind,
}

impl InputPort {
    /// Whether it is a main port: `Video` read as a picture, or `Audio` read as sound.
    pub fn is_main(&self) -> bool {
        is_main_port(&self.name, self.kind)
    }
}

/// Whether port `name` read as `kind` is a main port, which reads what is below unless set
/// otherwise.
pub fn is_main_port(name: &str, kind: InputKind) -> bool {
    match kind {
        InputKind::Video => name == DEFAULT_VIDEO,
        InputKind::Audio => name == DEFAULT_AUDIO,
    }
}

/// The port a node of type `kind` with `params` reads, and whether it reads pictures or sound;
/// `None` for nodes other than input ports.
pub fn port_of<'a>(
    kind: &str,
    params: &'a BTreeMap<String, ParamValue>,
) -> Option<(&'a str, InputKind)> {
    let (kind, default) = match kind {
        VIDEO_INPUT => (InputKind::Video, DEFAULT_VIDEO),
        AUDIO_INPUT => (InputKind::Audio, DEFAULT_AUDIO),
        _ => return None,
    };
    let name = match params.get(PORT_PARAM) {
        Some(ParamValue::Text(name)) => name.as_str(),
        _ => default,
    };
    Some((name, kind))
}

/// Graph `desc`'s input ports in the order their first nodes come: each name once for each kind
/// it is read as.
pub fn input_ports(desc: &GraphDesc) -> Vec<InputPort> {
    let mut ports: Vec<InputPort> = Vec::new();
    for (name, kind) in desc
        .nodes
        .iter()
        .filter_map(|n| port_of(&n.kind, &n.params))
    {
        if !ports.iter().any(|p| p.name == name && p.kind == kind) {
            ports.push(InputPort {
                name: name.to_owned(),
                kind,
            });
        }
    }
    ports
}

/// The names of `desc`'s input ports other than the main ones, each once. A name read as a
/// picture and as sound gets the picture and the sound of its source.
pub fn extra_ports(desc: &GraphDesc) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for port in input_ports(desc).into_iter().filter(|p| !p.is_main()) {
        if !names.contains(&port.name) {
            names.push(port.name);
        }
    }
    names
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
    fn main_ports_are_video_read_as_a_picture_and_audio_read_as_sound() {
        let desc = GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [
                { "id": "v", "type": "video_input" },
                { "id": "a", "type": "audio_input" },
                { "id": "k", "type": "audio_input", "params": { "port": "Kick" } },
                { "id": "k2", "type": "video_input", "params": { "port": "Kick" } },
                { "id": "odd", "type": "video_input", "params": { "port": "Audio" } },
                { "id": "o", "type": "output" } ] }"#,
        )
        .unwrap();
        assert_eq!(extra_ports(&desc), ["Kick", "Audio"]);
        assert_eq!(input_ports(&desc).len(), 5);
    }
}
