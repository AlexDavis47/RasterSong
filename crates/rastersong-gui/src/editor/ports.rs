//! The graph's ends: its input ports and its output.
//!
//! Input ports are ordinary nodes, titled after the port they read. Every graph has exactly one
//! output, so it can't be deleted or copied, and a graph missing one gets one.

use eframe::egui::pos2;
use rastersong_engine::{InputKind, NodeType, port_of};
use rastersong_lang::tr_args;

use super::{EditorNode, GraphEditor, NodeKey};

pub(super) use rastersong_engine::OUTPUT;

/// How far right of the rightmost node a missing output goes.
const OUTPUT_GAP: f32 = 190.0;

impl GraphEditor {
    /// Whether the node is the graph's output, which the user can't delete or copy.
    pub fn is_protected(&self, key: NodeKey) -> bool {
        self.node(key).is_some_and(|n| n.kind == OUTPUT)
    }

    /// The title an input port shows unless the user named it: the port it reads.
    pub(super) fn port_title(&self, node: &EditorNode) -> Option<String> {
        let (port, kind) = port_of(&node.kind, &node.params)?;
        let key = match kind {
            InputKind::Video => "editor.port.video_title",
            InputKind::Audio => "editor.port.audio_title",
        };
        Some(tr_args(key, &[("port", port)]))
    }

    /// Adds an output when the graph has none.
    pub fn ensure_output(&mut self) {
        if self.nodes.iter().any(|n| n.kind == OUTPUT) {
            return;
        }
        let right = self.nodes.iter().map(|n| n.pos.x).fold(0.0, f32::max);
        let top = self
            .nodes
            .iter()
            .map(|n| n.pos.y)
            .reduce(f32::min)
            .unwrap_or(0.0);
        self.add_node(OUTPUT, pos2(right + OUTPUT_GAP, top));
    }

    /// Whether the user can add a node type themselves: anything but the output.
    pub(super) fn user_addable(kind: &NodeType) -> bool {
        kind.spec.addable
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
    fn only_the_output_is_kept_from_deleting_and_copying() {
        let mut editor = GraphEditor::new(&GraphDesc::from_json(GRAPH).unwrap());
        let keys: BTreeSet<NodeKey> = ["video", "audio", "out", "split"]
            .iter()
            .map(|id| editor.key_of(id).unwrap())
            .collect();
        let copied = editor.fragment(&keys);
        assert!(copied.nodes.iter().all(|n| n.kind != OUTPUT));
        assert_eq!(copied.nodes.len(), 3);
        editor.remove_nodes(&keys);
        let left: Vec<&str> = editor.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(
            left.contains(&"out") && !left.contains(&"video"),
            "{left:?}"
        );
    }

    #[test]
    fn a_missing_output_is_added() {
        let mut editor =
            GraphEditor::new(&GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        editor.ensure_output();
        editor.ensure_output();
        let kinds: Vec<&str> = editor.nodes.iter().map(|n| n.kind.as_str()).collect();
        assert_eq!(kinds, [OUTPUT]);
    }
}
