//! The graph's ends: its inputs and outputs.
//!
//! Every graph has one Video Output and one Audio Output. They can't be deleted or copied, and a
//! graph missing one gets it back. Inputs (Video Input, Audio Input, Named Input), Named Outputs
//! and Audio Bus Outputs are ordinary nodes; new graphs come with a Video Input and an Audio
//! Input reading what is below. Input nodes are titled after what they read.

use eframe::egui::pos2;
use rastersong_engine::{AUDIO_OUTPUT, InputKind, NodeType, Source, port_of};
use rastersong_lang::{tr, tr_args};

use super::{EditorNode, GraphEditor, NodeKey};

pub(super) use rastersong_engine::OUTPUT;

/// How far right of the rightmost node a missing output goes.
const OUTPUT_GAP: f32 = 190.0;
/// How far apart missing outputs are.
const ADDED_STEP: f32 = 90.0;

/// The base nodes every graph has one of.
const BASE: [&str; 2] = [OUTPUT, AUDIO_OUTPUT];

impl GraphEditor {
    /// Whether the node is one of the graph's base nodes, which the user can't delete or copy:
    /// the first Video Output and Audio Output.
    pub fn is_protected(&self, key: NodeKey) -> bool {
        let Some(node) = self.node(key) else {
            return false;
        };
        BASE.contains(&node.kind.as_str())
            && self
                .nodes
                .iter()
                .find(|n| n.kind == node.kind)
                .is_some_and(|first| first.key == key)
    }

    /// The title an input node shows unless the user named it: what it reads.
    pub(super) fn port_title(&self, node: &EditorNode) -> Option<String> {
        let port = port_of(&node.kind, &node.params)?;
        Some(port_label(
            &source_label(&port.source, port.kind),
            port.kind,
        ))
    }

    /// Adds whichever of the base nodes the graph lacks, right of everything.
    pub fn ensure_ports(&mut self) {
        let missing: Vec<&str> = BASE
            .into_iter()
            .filter(|kind| !self.nodes.iter().any(|n| n.kind == *kind))
            .collect();
        if missing.is_empty() {
            return;
        }
        let right = self.nodes.iter().map(|n| n.pos.x).reduce(f32::max);
        let top = self
            .nodes
            .iter()
            .map(|n| n.pos.y)
            .reduce(f32::min)
            .unwrap_or(0.0);
        let x = right.map_or(0.0, |r| r + OUTPUT_GAP);
        for (i, kind) in missing.into_iter().enumerate() {
            self.add_node(kind, pos2(x, top + i as f32 * ADDED_STEP));
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
        InputKind::Named => "editor.port.named_title",
    };
    tr_args(key, &[("port", label)])
}

/// How `source`, read as `kind`, is named: the main name for what is below, the mix, a track's
/// name, a graph track, a graph track and its Named Output, or Nothing.
pub fn source_label(source: &Source, kind: InputKind) -> String {
    let video = kind == InputKind::Video;
    match source {
        Source::Below if video => rastersong_engine::DEFAULT_VIDEO.to_owned(),
        Source::Below => rastersong_engine::DEFAULT_AUDIO.to_owned(),
        Source::Mix if video => tr("source.video_mix").to_owned(),
        Source::Mix => tr("source.audio_mix").to_owned(),
        Source::Track(track) | Source::Graph(track) => track.clone(),
        Source::Named { track, name } => {
            tr_args("source.graph_output", &[("track", track), ("output", name)])
        }
        Source::Nothing => tr("source.nothing").to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use rastersong_engine::{GraphDesc, ParamValue};

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
    fn inputs_reading_elsewhere_are_titled_after_what_they_read() {
        let mut desc = GraphDesc::from_json(GRAPH).unwrap();
        let mut kick =
            rastersong_engine::NodeDesc::new("kick".into(), rastersong_engine::AUDIO_INPUT);
        kick.params.insert(
            rastersong_engine::SOURCE_PARAM.into(),
            ParamValue::Text("track:Kick".into()),
        );
        desc.nodes.push(kick);
        let mut glow =
            rastersong_engine::NodeDesc::new("glow".into(), rastersong_engine::NAMED_INPUT);
        for (param, value) in [
            (rastersong_engine::TRACK_PARAM, "FX"),
            (rastersong_engine::NAME_PARAM, "Glow"),
        ] {
            glow.params
                .insert(param.into(), ParamValue::Text(value.into()));
        }
        desc.nodes.push(glow);
        let editor = GraphEditor::new(&desc);
        let title = |id: &str| editor.port_title(editor.node(editor.key_of(id).unwrap()).unwrap());
        assert_eq!(title("kick").as_deref(), Some("♪ In: Kick"));
        assert_eq!(title("glow").as_deref(), Some("◆ In: FX › Glow"));
    }

    #[test]
    fn only_the_outputs_are_kept_from_deleting_and_copying() {
        let mut editor = GraphEditor::new(&GraphDesc::from_json(GRAPH).unwrap());
        let keys: BTreeSet<NodeKey> = ["video", "audio", "out", "sound", "split"]
            .iter()
            .map(|id| editor.key_of(id).unwrap())
            .collect();
        let copied = editor.fragment(&keys);
        let ids: Vec<&str> = copied.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["video", "audio", "split"]);
        editor.remove_nodes(&keys);
        let left: Vec<&str> = editor.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(
            ["out", "sound"].iter().all(|id| left.contains(id))
                && !["video", "audio", "split"]
                    .iter()
                    .any(|id| left.contains(id)),
            "{left:?}"
        );
    }

    #[test]
    fn missing_outputs_are_added() {
        let mut editor =
            GraphEditor::new(&GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        editor.ensure_ports();
        editor.ensure_ports();
        let kinds: Vec<&str> = editor.nodes.iter().map(|n| n.kind.as_str()).collect();
        assert_eq!(kinds, [OUTPUT, AUDIO_OUTPUT]);
        assert!(editor.nodes.iter().all(|n| editor.is_protected(n.key)));
    }
}
