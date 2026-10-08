//! What the renderer routes: the track tree with each track's FX, the master's FX, and the graphs
//! they use. See [Routing](../../../docs/engine.md#routing).

use std::collections::BTreeMap;

use rastersong_graph::nodes::{AUDIO_INPUT, DEFAULT_AUDIO, DEFAULT_VIDEO, PORT_PARAM, VIDEO_INPUT};
use rastersong_graph::{GraphDesc, ParamValue};

use crate::timeline::{Fx, TrackKind};

/// Whether an input port reads pictures or sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Video,
    Audio,
}

/// An input port of a graph: what the host or a receive fills, by name. Several input nodes can
/// read one port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputPort {
    pub name: String,
    /// What the port's first input node reads.
    pub kind: InputKind,
}

impl InputPort {
    /// Whether the host fills the port: `Video` read as a picture, or `Audio` read as sound.
    pub fn is_main(&self) -> bool {
        is_main_port(&self.name, self.kind)
    }
}

/// Whether port `name` read as `kind` is a main port, filled by what the FX is on.
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

/// The ports of `desc` a receive can fill, by name: every input port but the main ones. A
/// receive fills a name read as a picture and as sound with the track's picture and sound.
pub fn receive_ports(desc: &GraphDesc) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for port in input_ports(desc).into_iter().filter(|p| !p.is_main()) {
        if !names.contains(&port.name) {
            names.push(port.name);
        }
    }
    names
}

/// One track of the tree as the renderer routes it.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteTrack {
    pub name: String,
    /// How many folders it is in; a folder holds the deeper tracks right after it.
    pub depth: u32,
    pub folder: bool,
    /// What it plays; `None` for a folder or an empty track.
    pub kind: Option<TrackKind>,
    /// Its FX chain, in order.
    pub fx: Vec<Fx>,
    /// Its level in its folder's (or the master's) sound.
    pub volume: f32,
    pub muted: bool,
    /// Whether it is mixed into its folder (or the master) at all.
    pub master_send: bool,
    /// Left out of the mix by a solo on another track of its kind.
    pub solo_out: bool,
    /// The bus a top-level track's sound is summed into.
    pub bus: String,
}

impl RouteTrack {
    /// A top-level track of `kind`, at full level on the master, with no FX.
    pub fn new(name: impl Into<String>, kind: Option<TrackKind>) -> Self {
        Self {
            name: name.into(),
            depth: 0,
            folder: false,
            kind,
            fx: Vec::new(),
            volume: 1.0,
            muted: false,
            master_send: true,
            solo_out: false,
            bus: rastersong_graph::nodes::DEFAULT_BUS.to_owned(),
        }
    }

    /// Whether it reaches its folder (or the master): not muted, sending, and not left out by a
    /// solo.
    pub fn sends(&self) -> bool {
        !self.muted && self.master_send && !self.solo_out
    }
}

/// What the renderer routes: the track tree, top first, the master's FX chain, and the graphs
/// the FX use.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Routing {
    pub tracks: Vec<RouteTrack>,
    pub master_fx: Vec<Fx>,
    /// Every graph an FX may use, by id, but the open one, which comes separately because it
    /// changes as it is edited.
    pub graphs: Vec<(u32, GraphDesc)>,
    /// The id of the open graph.
    pub open: u32,
}

impl Routing {
    /// The routing with every FX left out: what plays with all of them bypassed.
    pub fn without_fx(mut self) -> Self {
        for track in &mut self.tracks {
            track.fx.clear();
        }
        self.master_fx.clear();
        self
    }

    /// Whether any FX is on (items' FX are with the tracks' items).
    pub fn has_fx(&self) -> bool {
        self.master_fx.iter().any(|f| !f.bypass)
            || self.tracks.iter().flat_map(|t| &t.fx).any(|f| !f.bypass)
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
        assert_eq!(receive_ports(&desc), ["Kick", "Audio"]);
        assert_eq!(input_ports(&desc).len(), 5);
    }
}
