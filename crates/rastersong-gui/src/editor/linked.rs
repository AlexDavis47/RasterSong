//! Nodes linked to the project: the video input, one audio input per audio track, and the output.
//!
//! The project manages them, not the user. Adding the video or a track adds its node, named after
//! it, and removing a track removes its node. They can't be deleted, copied or added by hand.
//! An audio input whose track doesn't exist (e.g. in the starter graph, before any audio is
//! added) is an ordinary node, and the first track added takes it over.

use std::collections::BTreeSet;

use eframe::egui::{Pos2, pos2, vec2};
use rastersong_engine::{DEFAULT_AUDIO_TRACK, NodeType, ParamValue};

use super::{EditorNode, GraphEditor, NodeKey};

pub(super) use rastersong_engine::{AUDIO_INPUT, OUTPUT, VIDEO_INPUT};

/// Vertical distance between input nodes added for the project.
const INPUT_SPACING: f32 = 80.0;
/// How far right of the rightmost node a new output goes.
const OUTPUT_GAP: f32 = 190.0;

/// A name the user changed on a node linked to the project. The name belongs to the project (the
/// timeline shows it too), so the editor passes the rename on instead of keeping it on the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkedRename {
    Video(String),
    Track { from: String, to: String },
}

/// The track an audio input reads.
pub(super) fn track_of(node: &EditorNode) -> &str {
    match node.params.get("source") {
        Some(ParamValue::Text(name)) => name,
        _ => DEFAULT_AUDIO_TRACK,
    }
}

impl GraphEditor {
    /// Tells the editor what the project holds, for naming and protecting the linked nodes.
    /// Call it whenever the video or the tracks change (it's cheap to call every frame).
    pub fn set_project_inputs(&mut self, video: Option<String>, tracks: Vec<String>) {
        self.project_video = video;
        self.project_tracks = tracks;
    }

    /// Whether the project manages this node, so the user can't delete or copy it.
    pub fn is_linked(&self, key: NodeKey) -> bool {
        self.node(key).is_some_and(|n| self.node_is_linked(n))
    }

    pub(super) fn node_is_linked(&self, node: &EditorNode) -> bool {
        match node.kind.as_str() {
            VIDEO_INPUT | OUTPUT => true,
            AUDIO_INPUT => self.project_tracks.iter().any(|t| t == track_of(node)),
            _ => false,
        }
    }

    /// Renames the user made on linked nodes since the last call.
    pub fn take_renames(&mut self) -> Vec<LinkedRename> {
        std::mem::take(&mut self.renames)
    }

    /// The project name a linked node shows and renames, if it has one: the video's, or its
    /// track's. `None` for other nodes, which keep their names to themselves.
    pub(super) fn linked_name(&self, node: &EditorNode) -> Option<String> {
        match node.kind.as_str() {
            VIDEO_INPUT => Some(self.project_video.clone().unwrap_or_else(|| "Video".into())),
            AUDIO_INPUT if self.node_is_linked(node) => Some(track_of(node).to_owned()),
            _ => None,
        }
    }

    /// What renaming a linked node to `name` asks of the project.
    pub(super) fn rename_request(&self, node: &EditorNode, name: String) -> Option<LinkedRename> {
        match node.kind.as_str() {
            VIDEO_INPUT => Some(LinkedRename::Video(name)),
            AUDIO_INPUT if self.node_is_linked(node) => Some(LinkedRename::Track {
                from: track_of(node).to_owned(),
                to: name,
            }),
            _ => None,
        }
    }

    /// The title a linked node shows unless the user named it: what it's linked to.
    pub(super) fn linked_title(&self, node: &EditorNode) -> Option<String> {
        match node.kind.as_str() {
            VIDEO_INPUT => Some(format!(
                "▣ {}",
                self.project_video.as_deref().unwrap_or("Video")
            )),
            AUDIO_INPUT => {
                let track = track_of(node);
                Some(if self.project_tracks.iter().any(|t| t == track) {
                    format!("♪ {track}")
                } else {
                    format!("♪ {track} (no track)")
                })
            }
            _ => None,
        }
    }

    /// Adds the node for a new audio track, or hands it an audio input that has no track yet.
    pub fn link_track(&mut self, name: &str) {
        let tracks = self.project_tracks.clone();
        if self
            .nodes
            .iter()
            .any(|n| n.kind == AUDIO_INPUT && track_of(n) == name)
        {
            return;
        }
        let orphan = self
            .nodes
            .iter_mut()
            .find(|n| n.kind == AUDIO_INPUT && !tracks.iter().any(|t| t == track_of(n)));
        match orphan {
            Some(node) => {
                node.params
                    .insert("source".into(), ParamValue::Text(name.to_owned()));
            }
            None => {
                let pos = self.next_input_position();
                if let Some(key) = self.add_node(AUDIO_INPUT, pos) {
                    self.node_mut(key)
                        .unwrap()
                        .params
                        .insert("source".into(), ParamValue::Text(name.to_owned()));
                }
            }
        }
    }

    /// Removes the nodes of a track that was removed from the project.
    pub fn unlink_track(&mut self, name: &str) {
        let keys: BTreeSet<NodeKey> = self
            .nodes
            .iter()
            .filter(|n| n.kind == AUDIO_INPUT && track_of(n) == name)
            .map(|n| n.key)
            .collect();
        self.remove_nodes_unchecked(&keys);
    }

    /// Adds whatever linked nodes are missing: the video input, the output, and a node per
    /// track. For projects made before nodes were linked, and after opening a video.
    pub fn ensure_linked_nodes(&mut self) {
        if !self.nodes.iter().any(|n| n.kind == VIDEO_INPUT) {
            let pos = self.next_input_position();
            self.add_node(VIDEO_INPUT, pos);
        }
        for track in self.project_tracks.clone() {
            self.link_track(&track);
        }
        if !self.nodes.iter().any(|n| n.kind == OUTPUT) {
            let right = self.nodes.iter().map(|n| n.pos.x).fold(0.0, f32::max);
            let pos = pos2(right + OUTPUT_GAP, self.input_column().1);
            self.add_node(OUTPUT, pos);
        }
    }

    /// The left edge and top of the column of input nodes.
    fn input_column(&self) -> (f32, f32) {
        let inputs = self
            .nodes
            .iter()
            .filter(|n| matches!(n.kind.as_str(), VIDEO_INPUT | AUDIO_INPUT));
        inputs.fold((f32::INFINITY, f32::INFINITY), |(x, y), n| {
            (x.min(n.pos.x), y.min(n.pos.y))
        })
    }

    /// Below the lowest input node, or at the origin if there are none.
    fn next_input_position(&self) -> Pos2 {
        let lowest = self
            .nodes
            .iter()
            .filter(|n| matches!(n.kind.as_str(), VIDEO_INPUT | AUDIO_INPUT))
            .map(|n| n.pos)
            .reduce(|a, b| if b.y > a.y { b } else { a });
        match lowest {
            Some(p) => pos2(self.input_column().0, p.y) + vec2(0.0, INPUT_SPACING),
            None => Pos2::ZERO,
        }
    }

    /// Whether the user can add a node type themselves: not the project's inputs and output,
    /// but the optional audio output.
    pub(super) fn user_addable(kind: &NodeType) -> bool {
        kind.spec.addable
    }
}

#[cfg(test)]
mod tests {
    use rastersong_engine::GraphDesc;

    use super::*;

    const GRAPH: &str = include_str!("../../../../examples/graphs/am_bands.json");

    fn editor(tracks: &[&str]) -> GraphEditor {
        let mut editor = GraphEditor::new(&GraphDesc::from_json(GRAPH).unwrap());
        editor.set_project_inputs(
            Some("clip.mp4".into()),
            tracks.iter().map(|t| t.to_string()).collect(),
        );
        editor
    }

    fn audio_inputs(editor: &GraphEditor) -> Vec<String> {
        editor
            .nodes
            .iter()
            .filter(|n| n.kind == AUDIO_INPUT)
            .map(|n| track_of(n).to_owned())
            .collect()
    }

    #[test]
    fn the_first_track_takes_over_the_starter_audio_input() {
        // The starter graph reads track "audio", which the project doesn't have.
        let mut editor = editor(&[]);
        let audio = editor.key_of("audio").unwrap();
        assert!(!editor.is_linked(audio), "an orphan is an ordinary node");

        editor.set_project_inputs(None, vec!["drums".into()]);
        editor.link_track("drums");
        assert_eq!(audio_inputs(&editor), ["drums"]);
        assert!(editor.is_linked(audio));
        assert_eq!(
            editor.linked_title(editor.node(audio).unwrap()).as_deref(),
            Some("♪ drums")
        );

        // The next track gets a node of its own, below.
        editor.set_project_inputs(None, vec!["drums".into(), "bass".into()]);
        editor.link_track("bass");
        assert_eq!(audio_inputs(&editor), ["drums", "bass"]);
        let bass = editor.nodes.iter().find(|n| track_of(n) == "bass").unwrap();
        assert!(bass.pos.y > editor.node(audio).unwrap().pos.y);
    }

    #[test]
    fn removing_a_track_removes_its_node() {
        let mut editor = editor(&["audio"]);
        editor.set_project_inputs(None, vec![]);
        editor.unlink_track("audio");
        assert!(audio_inputs(&editor).is_empty());
    }

    #[test]
    fn linked_nodes_cant_be_deleted_or_copied() {
        let mut editor = editor(&["audio"]);
        let linked: BTreeSet<NodeKey> = ["video", "audio", "out"]
            .iter()
            .map(|id| editor.key_of(id).unwrap())
            .collect();
        let split = editor.key_of("split").unwrap();
        let mut keys = linked.clone();
        keys.insert(split);

        assert_eq!(
            editor.fragment(&keys).nodes.len(),
            1,
            "only split is copied"
        );
        editor.duplicate(&keys);
        assert_eq!(editor.node_count(), 10);
        editor.remove_nodes(&keys);
        assert!(linked.iter().all(|&k| editor.node(k).is_some()));
        assert!(editor.node(split).is_none());
    }

    #[test]
    fn missing_linked_nodes_are_added() {
        let mut editor =
            GraphEditor::new(&GraphDesc::from_json(r#"{ "version": 1, "nodes": [] }"#).unwrap());
        editor.set_project_inputs(None, vec!["a".into(), "b".into()]);
        editor.ensure_linked_nodes();
        let kinds: Vec<&str> = editor.nodes.iter().map(|n| n.kind.as_str()).collect();
        assert_eq!(kinds, [VIDEO_INPUT, AUDIO_INPUT, AUDIO_INPUT, OUTPUT]);
        assert_eq!(audio_inputs(&editor), ["a", "b"]);
        // Doing it again changes nothing.
        editor.ensure_linked_nodes();
        assert_eq!(editor.node_count(), 4);
    }
}
