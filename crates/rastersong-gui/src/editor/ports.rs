//! The graph's ends: its inputs and outputs.
//!
//! Every graph has one each of Video In, Audio In, Output and an Audio Output on the project's
//! master bus. They can't be deleted or copied, and a graph missing one gets it back. Video
//! Source, Audio Source, Named Output and Audio Outputs for other buses are ordinary nodes.
//! Input nodes are titled after what they read.

use eframe::egui::pos2;
use rastersong_engine::{
    AUDIO_INPUT, AUDIO_OUTPUT, BUS_PARAM, InputKind, NodeType, ParamValue, Source, VIDEO_INPUT,
    audio_output_bus, port_of,
};
use rastersong_lang::{tr, tr_args};

use super::{EditorNode, GraphEditor, NodeKey};

pub(super) use rastersong_engine::OUTPUT;

/// How far right of the rightmost node a missing output goes, and left of the leftmost a
/// missing input.
const OUTPUT_GAP: f32 = 190.0;
/// How far apart missing nodes added on one side are.
const ADDED_STEP: f32 = 90.0;

impl GraphEditor {
    /// Whether the node is one of the graph's base nodes, which the user can't delete or copy:
    /// the first Video In, Audio In and Output, and the first Audio Output on the master bus.
    pub fn is_protected(&self, key: NodeKey) -> bool {
        let Some(node) = self.node(key) else {
            return false;
        };
        let base = |n: &EditorNode| match n.kind.as_str() {
            VIDEO_INPUT | AUDIO_INPUT | OUTPUT => true,
            AUDIO_OUTPUT => self.writes_master(n),
            _ => false,
        };
        base(node)
            && self
                .nodes
                .iter()
                .find(|n| n.kind == node.kind && base(n))
                .is_some_and(|first| first.key == key)
    }

    /// Whether audio output `node` writes to the project's master bus.
    fn writes_master(&self, node: &EditorNode) -> bool {
        let params = node
            .params
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let desc = rastersong_engine::NodeDesc {
            params,
            ..rastersong_engine::NodeDesc::new(node.id.clone(), AUDIO_OUTPUT)
        };
        audio_output_bus(&desc) == self.master_bus
    }

    /// The title an input node shows unless the user named it: what it reads.
    pub(super) fn port_title(&self, node: &EditorNode) -> Option<String> {
        let port = port_of(&node.kind, &node.params)?;
        Some(port_label(
            &source_label(&port.source, port.kind),
            port.kind,
        ))
    }

    /// Adds whichever of the base nodes the graph lacks: the inputs left of everything, the
    /// outputs right of it.
    pub fn ensure_ports(&mut self) {
        let has = |editor: &Self, kind: &str| match kind {
            AUDIO_OUTPUT => editor
                .nodes
                .iter()
                .any(|n| n.kind == AUDIO_OUTPUT && editor.writes_master(n)),
            _ => editor.nodes.iter().any(|n| n.kind == kind),
        };
        let missing: Vec<&str> = [VIDEO_INPUT, AUDIO_INPUT, OUTPUT, AUDIO_OUTPUT]
            .into_iter()
            .filter(|kind| !has(self, kind))
            .collect();
        if missing.is_empty() {
            return;
        }
        let xs = self.nodes.iter().map(|n| n.pos.x);
        let (left, right) = (
            xs.clone().reduce(f32::min).unwrap_or(0.0),
            xs.reduce(f32::max).unwrap_or(0.0),
        );
        let top = self
            .nodes
            .iter()
            .map(|n| n.pos.y)
            .reduce(f32::min)
            .unwrap_or(0.0);
        let (mut inputs, mut outputs) = (0.0, 0.0);
        for kind in missing {
            let at = if matches!(kind, VIDEO_INPUT | AUDIO_INPUT) {
                inputs += ADDED_STEP;
                pos2(left - OUTPUT_GAP, top + inputs - ADDED_STEP)
            } else {
                outputs += ADDED_STEP;
                pos2(right + OUTPUT_GAP, top + outputs - ADDED_STEP)
            };
            if let Some(key) = self.add_node(kind, at)
                && kind == AUDIO_OUTPUT
            {
                let master = self.master_bus.clone();
                self.node_mut(key)
                    .unwrap()
                    .params
                    .insert(BUS_PARAM.to_owned(), ParamValue::Text(master));
            }
        }
    }

    /// Whether the user can add a node type themselves: anything but the base nodes.
    pub(super) fn user_addable(kind: &NodeType) -> bool {
        kind.spec.addable
    }
}

/// How an input reading `label` as `kind` is named wherever it is shown: "In: Video" with the
/// kind's glyph.
pub fn port_label(label: &str, kind: InputKind) -> String {
    let key = match kind {
        InputKind::Video => "editor.port.video_title",
        InputKind::Audio => "editor.port.audio_title",
    };
    tr_args(key, &[("port", label)])
}

/// How `source`, read as `kind`, is named: the main name for what is below, the mix, a track's
/// name, a graph track and its output, or Nothing.
pub fn source_label(source: &Source, kind: InputKind) -> String {
    let video = kind == InputKind::Video;
    match source {
        Source::Below if video => rastersong_engine::DEFAULT_VIDEO.to_owned(),
        Source::Below => rastersong_engine::DEFAULT_AUDIO.to_owned(),
        Source::Mix if video => tr("source.video_mix").to_owned(),
        Source::Mix => tr("source.audio_mix").to_owned(),
        Source::Track(track) => track.clone(),
        Source::Graph { track, port } => {
            let main = if video {
                rastersong_engine::DEFAULT_VIDEO
            } else {
                rastersong_engine::DEFAULT_AUDIO
            };
            if port == main {
                track.clone()
            } else {
                tr_args("source.graph_output", &[("track", track), ("output", port)])
            }
        }
        Source::Nothing => tr("source.nothing").to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use rastersong_engine::GraphDesc;

    use super::*;

    const GRAPH: &str = include_str!("../../../../examples/graphs/am_bands.json");

    #[test]
    fn inputs_are_titled_after_their_port() {
        let editor = GraphEditor::new(&GraphDesc::from_json(GRAPH).unwrap());
        let title = |id: &str| editor.port_title(editor.node(editor.key_of(id).unwrap()).unwrap());
        assert_eq!(title("video").as_deref(), Some("▣ In: Video"));
        assert_eq!(title("audio").as_deref(), Some("♪ In: Audio"));
        assert_eq!(title("out"), None);
    }

    #[test]
    fn sources_are_titled_after_what_they_read() {
        let mut desc = GraphDesc::from_json(GRAPH).unwrap();
        let mut kick =
            rastersong_engine::NodeDesc::new("kick".into(), rastersong_engine::AUDIO_SOURCE_NODE);
        kick.params.insert(
            rastersong_engine::SOURCE_PARAM.into(),
            ParamValue::Text("track:Kick".into()),
        );
        desc.nodes.push(kick);
        let editor = GraphEditor::new(&desc);
        let title = |id: &str| editor.port_title(editor.node(editor.key_of(id).unwrap()).unwrap());
        assert_eq!(title("kick").as_deref(), Some("♪ In: Kick"));
    }

    #[test]
    fn only_the_base_nodes_are_kept_from_deleting_and_copying() {
        let mut editor = GraphEditor::new(&GraphDesc::from_json(GRAPH).unwrap());
        let keys: BTreeSet<NodeKey> = ["video", "audio", "out", "split"]
            .iter()
            .map(|id| editor.key_of(id).unwrap())
            .collect();
        let copied = editor.fragment(&keys);
        let ids: Vec<&str> = copied.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["split"]);
        editor.remove_nodes(&keys);
        let left: Vec<&str> = editor.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(
            ["video", "audio", "out"].iter().all(|id| left.contains(id))
                && !left.contains(&"split"),
            "{left:?}"
        );
        // A second Video In is an ordinary node.
        let extra = editor.add_node(VIDEO_INPUT, pos2(0.0, 0.0)).unwrap();
        assert!(!editor.is_protected(extra));
        editor.remove_nodes(&BTreeSet::from([extra]));
        assert!(editor.node(extra).is_none());
    }

    #[test]
    fn missing_base_nodes_are_added() {
        let mut editor =
            GraphEditor::new(&GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        editor.master_bus = "Master".into();
        editor.ensure_ports();
        editor.ensure_ports();
        let kinds: Vec<&str> = editor.nodes.iter().map(|n| n.kind.as_str()).collect();
        assert_eq!(kinds, [VIDEO_INPUT, AUDIO_INPUT, OUTPUT, AUDIO_OUTPUT]);
        let audio = editor.nodes.last().unwrap();
        assert_eq!(
            audio.params.get(BUS_PARAM),
            Some(&ParamValue::Text("Master".into()))
        );
        assert!(editor.is_protected(audio.key));
    }
}
