//! Graph tracks: the Graphs section above the video tracks, holding graph items.
//!
//! Items on one graph track never overlap: placing or moving one trims what it lands on. At each
//! frame the topmost active item's outputs are the master. What a graph reads is set on its input
//! nodes, not on its items. See [Composition](../../../../docs/engine.md#composition).

use rastersong_graph::nodes::{AUDIO_SOURCE, SOURCE_PARAM, VIDEO_SOURCE};
use rastersong_graph::{GraphDesc, ParamValue};
use serde::{Deserialize, Serialize};

use super::{Edge, MIN_ITEM_LENGTH, Project};
use crate::composition::{ComposedGraphTrack, Composition, MediaTrack};
use crate::timeline::{GraphItem, Source};

/// A track of graph items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphTrack {
    /// What the timeline and sources call the track. Unique among all tracks.
    pub name: String,
    /// Items by position, never overlapping.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<GraphItem>,
    /// A muted graph track is skipped: what is below it shows through.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
    /// The track's height on the timeline, in points. `None` is the app's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
}

impl GraphTrack {
    pub fn new(name: String) -> Self {
        Self {
            name,
            items: Vec::new(),
            muted: false,
            height: None,
        }
    }
}

impl Project {
    /// The description of graph `id`, open or stored.
    pub fn graph_desc(&self, id: u32) -> Option<&GraphDesc> {
        if id == self.graph_id {
            Some(&self.graph)
        } else {
            self.graphs.iter().find(|g| g.id == id).map(|g| &g.graph)
        }
    }

    /// Whether any graph item plays. Without one nothing is applied: the output is the plain
    /// mix.
    pub fn has_graph_items(&self) -> bool {
        self.graph_tracks
            .iter()
            .any(|t| !t.muted && t.items.iter().any(|i| !i.muted))
    }

    /// Whether the preview skips every graph and plays the mix: no graph item plays.
    pub fn plays_track_mix(&self) -> bool {
        !self.has_graph_items()
    }

    /// The graph track called `name`.
    pub fn graph_track(&self, name: &str) -> Option<&GraphTrack> {
        self.graph_tracks.iter().find(|t| t.name == name)
    }

    fn graph_track_mut(&mut self, name: &str) -> Option<&mut GraphTrack> {
        self.graph_tracks.iter_mut().find(|t| t.name == name)
    }

    /// Adds an empty graph track at the top of the Graphs section, named `name`, `name_2`, …
    /// (unique among all tracks), and returns its name.
    pub fn add_graph_track(&mut self, name: &str) -> String {
        self.add_graph_track_at(name, 0)
    }

    /// Adds an empty graph track named `name`, `name_2`, … before the graph track now at index
    /// `at` (`graph_tracks.len()` for the bottom), and returns its name.
    pub fn add_graph_track_at(&mut self, name: &str, at: usize) -> String {
        let name = self.unique_name(name);
        let at = at.min(self.graph_tracks.len());
        self.graph_tracks.insert(at, GraphTrack::new(name.clone()));
        name
    }

    /// Removes the graph track at `index` with its items.
    pub fn remove_graph_track(&mut self, index: usize) -> Option<GraphTrack> {
        (index < self.graph_tracks.len()).then(|| self.graph_tracks.remove(index))
    }

    /// Moves the graph track at `from` so it lands before the one now at `slot`
    /// (`graph_tracks.len()` for the bottom). Returns where it ends up, or `None` when nothing
    /// moved.
    pub fn move_graph_track(&mut self, from: usize, slot: usize) -> Option<usize> {
        let len = self.graph_tracks.len();
        if from >= len || slot > len {
            return None;
        }
        let to = if slot > from { slot - 1 } else { slot };
        if to == from {
            return None;
        }
        let track = self.graph_tracks.remove(from);
        self.graph_tracks.insert(to, track);
        Some(to)
    }

    /// Places graph `graph` on graph track `track` from `at` for `length` seconds, trimming
    /// whatever it lands on. Returns the item's index, or `None` for a track or graph that
    /// doesn't exist.
    pub fn place_graph(&mut self, track: &str, graph: u32, at: f64, length: f64) -> Option<usize> {
        self.graph_desc(graph)?;
        let item = GraphItem::new(graph, at.max(0.0), length.max(MIN_ITEM_LENGTH));
        let track = self.graph_track_mut(track)?;
        Some(insert_over(&mut track.items, item))
    }

    /// Moves item `item` of graph track `track` by `delta` seconds (not before the timeline's
    /// start), trimming what it lands on, and returns its new index.
    pub fn move_graph_item(&mut self, track: &str, item: usize, delta: f64) -> Option<usize> {
        let items = &mut self.graph_track_mut(track)?.items;
        if item >= items.len() || !delta.is_finite() {
            return None;
        }
        let mut moved = items.remove(item);
        moved.position = (moved.position + delta).max(0.0);
        Some(insert_over(items, moved))
    }

    /// Moves item `item` of graph track `from` to graph track `to`, `delta` seconds later,
    /// trimming what it lands on there. Returns its index on `to`.
    pub fn move_graph_item_to(
        &mut self,
        from: &str,
        item: usize,
        to: &str,
        delta: f64,
    ) -> Option<usize> {
        if from == to {
            return self.move_graph_item(from, item, delta);
        }
        self.graph_track(to)?;
        let items = &mut self.graph_track_mut(from)?.items;
        if item >= items.len() || !delta.is_finite() {
            return None;
        }
        let mut moved = items.remove(item);
        moved.position = (moved.position + delta).max(0.0);
        Some(insert_over(&mut self.graph_track_mut(to)?.items, moved))
    }

    /// Moves edge `edge` of item `item` to timeline time `to`, within the gaps either side and
    /// leaving the item at least [`MIN_ITEM_LENGTH`] long. Trimming the left edge keeps what
    /// follows where it is.
    pub fn trim_graph_item(&mut self, track: &str, item: usize, edge: Edge, to: f64) {
        let Some(items) = self.graph_track_mut(track).map(|t| &mut t.items) else {
            return;
        };
        if item >= items.len() || !to.is_finite() {
            return;
        }
        let before = item.checked_sub(1).map_or(0.0, |i| items[i].end());
        let after = items.get(item + 1).map_or(f64::INFINITY, |i| i.position);
        let it = &mut items[item];
        match edge {
            Edge::Start => {
                // The graph's own time can't go below zero: it has nothing before it.
                let earliest = before.max(it.position - it.start);
                let latest = it.end() - MIN_ITEM_LENGTH;
                if latest < earliest {
                    return;
                }
                let position = to.clamp(earliest, latest);
                it.start += position - it.position;
                it.length += it.position - position;
                it.position = position;
            }
            Edge::End => {
                let end = to.clamp(it.position + MIN_ITEM_LENGTH, after);
                it.length = end - it.position;
            }
        }
    }

    /// Splits item `item` at timeline time `at`; returns the index of the second half, or
    /// `None` when `at` doesn't fall inside the item.
    pub fn split_graph_item(&mut self, track: &str, item: usize, at: f64) -> Option<usize> {
        let items = &mut self.graph_track_mut(track)?.items;
        let first = items.get(item)?;
        if !(at > first.position + MIN_ITEM_LENGTH && at < first.end() - MIN_ITEM_LENGTH) {
            return None;
        }
        let mut second = first.clone();
        second.start += at - first.position;
        second.length = first.end() - at;
        second.position = at;
        items[item].length = at - items[item].position;
        items.insert(item + 1, second);
        Some(item + 1)
    }

    /// Pastes a copy of `item` on graph track `track` from `at` seconds, trimming whatever it
    /// lands on, and returns its index. The copy keeps the item's graph and trim.
    pub fn paste_graph_item(&mut self, track: &str, item: &GraphItem, at: f64) -> Option<usize> {
        let item = GraphItem {
            position: at.max(0.0),
            ..item.clone()
        };
        Some(insert_over(&mut self.graph_track_mut(track)?.items, item))
    }

    /// Removes item `item` of graph track `track`.
    pub fn delete_graph_item(&mut self, track: &str, item: usize) -> bool {
        match self.graph_track_mut(track) {
            Some(t) if item < t.items.len() => {
                t.items.remove(item);
                true
            }
            _ => false,
        }
    }

    /// The graph item `item` of graph track `track`.
    pub fn graph_item(&self, track: &str, item: usize) -> Option<&GraphItem> {
        self.graph_track(track)?.items.get(item)
    }

    pub fn graph_item_mut(&mut self, track: &str, item: usize) -> Option<&mut GraphItem> {
        self.graph_track_mut(track)?.items.get_mut(item)
    }

    /// Replaces the open graph with `graph`, its edited version.
    pub fn set_open_graph(&mut self, graph: GraphDesc) {
        self.graph = graph;
    }

    /// Removes every item that places graph `graph`; used when the graph is deleted.
    pub fn remove_graph_items(&mut self, graph: u32) {
        for track in &mut self.graph_tracks {
            track.items.retain(|i| i.graph != graph);
        }
    }

    /// Whether `source` names something the project has: a track for a stem, a graph track for
    /// a graph track's output. Whether a graph track is below the item reading it depends on
    /// where the item is, so it isn't checked.
    pub fn source_exists(&self, source: &Source) -> bool {
        match source {
            Source::Track(name) => self.has_media_track(name),
            Source::Graph { track, .. } => self.graph_track(track).is_some(),
            _ => true,
        }
    }

    /// Points every Video Source and Audio Source reading track `old` (a stem or a graph
    /// track), in every graph, at `new`.
    pub(super) fn rename_sources(&mut self, old: &str, new: &str) {
        let graphs =
            std::iter::once(&mut self.graph).chain(self.graphs.iter_mut().map(|g| &mut g.graph));
        for node in graphs
            .flat_map(|g| &mut g.nodes)
            .filter(|n| n.kind == VIDEO_SOURCE || n.kind == AUDIO_SOURCE)
        {
            let Some(ParamValue::Text(source)) = node.params.get_mut(SOURCE_PARAM) else {
                continue;
            };
            for prefix in ["track:", "graph:"] {
                if source.strip_prefix(prefix) == Some(old) {
                    *source = format!("{prefix}{new}");
                }
            }
        }
    }

    /// What the engine composes: the video and audio tracks (solos leave out the other tracks
    /// of their kind from the mix), the graph tracks that play, and the stored graphs.
    pub fn composition(&self) -> Composition {
        let tracks = self
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| MediaTrack {
                name: t.name.clone(),
                kind: t.kind,
                volume: t.volume,
                in_mix: self.in_mix(i),
                bus: t.bus.clone(),
            })
            .collect();
        let graph_tracks = self
            .graph_tracks
            .iter()
            .filter(|t| !t.muted)
            .map(|t| ComposedGraphTrack {
                name: t.name.clone(),
                items: t.items.iter().filter(|i| !i.muted).cloned().collect(),
            })
            .collect();
        Composition {
            tracks,
            graph_tracks,
            graphs: self
                .graphs
                .iter()
                .map(|g| (g.id, g.graph.clone()))
                .collect(),
            open: self.graph_id,
        }
    }
}

/// Puts `item` among `items` (sorted, not overlapping), trimming what it covers: an item it
/// lies inside is cut in two. Returns the index `item` ends up at.
fn insert_over(items: &mut Vec<GraphItem>, item: GraphItem) -> usize {
    let (start, end) = (item.position, item.end());
    let mut kept = Vec::with_capacity(items.len() + 2);
    for other in items.drain(..) {
        if other.end() <= start || other.position >= end {
            kept.push(other);
            continue;
        }
        if other.position < start {
            let mut head = other.clone();
            head.length = start - other.position;
            if head.length >= MIN_ITEM_LENGTH {
                kept.push(head);
            }
        }
        if other.end() > end {
            let mut tail = other.clone();
            tail.start += end - other.position;
            tail.length = other.end() - end;
            tail.position = end;
            if tail.length >= MIN_ITEM_LENGTH {
                kept.push(tail);
            }
        }
    }
    kept.push(item);
    kept.sort_by(|a, b| a.position.total_cmp(&b.position));
    *items = kept;
    items
        .iter()
        .position(|i| i.position == start && i.length == end - start)
        .unwrap_or(0)
}

/// `tracks` with nonsense (non-finite or negative times, overlaps) repaired, for files.
pub(super) fn sanitize(tracks: &mut [GraphTrack]) {
    for track in tracks {
        let items = std::mem::take(&mut track.items);
        for item in items {
            let mut item = item.sanitized();
            item.length = item.length.max(MIN_ITEM_LENGTH);
            insert_over(&mut track.items, item);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composition::input_ports;
    use crate::timeline::TrackKind;

    fn project() -> Project {
        let graph = GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [
                { "id": "v", "type": "video_input" },
                { "id": "a", "type": "audio_input" },
                { "id": "k", "type": "audio_source", "params": { "source": "track:kick" } },
                { "id": "o", "type": "output" } ] }"#,
        )
        .unwrap();
        let mut project = Project::new(graph);
        project.add_graph_track("Graphs");
        project
    }

    fn spans(project: &Project) -> Vec<(f64, f64)> {
        project.graph_tracks[0]
            .items
            .iter()
            .map(|i| (i.position, i.end()))
            .collect()
    }

    #[test]
    fn graph_tracks_are_added_at_any_place() {
        let mut project = project();
        assert_eq!(project.add_graph_track_at("Graphs", 99), "Graphs_2");
        project.add_graph_track_at("Mid", 1);
        project.add_graph_track("Top");
        let names: Vec<&str> = project
            .graph_tracks
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(names, ["Top", "Graphs", "Mid", "Graphs_2"]);
    }

    #[test]
    fn a_pasted_graph_item_keeps_its_trim_and_trims_what_it_lands_on() {
        let mut project = project();
        let id = project.graph_id;
        project.place_graph("Graphs", id, 0.0, 4.0).unwrap();
        let mut copied = project.graph_tracks[0].items[0].clone();
        copied.start = 0.5;
        copied.length = 1.0;
        let at = project.paste_graph_item("Graphs", &copied, 2.0).unwrap();
        let pasted = &project.graph_tracks[0].items[at];
        assert_eq!(
            (pasted.position, pasted.start, pasted.length),
            (2.0, 0.5, 1.0)
        );
        assert_eq!(spans(&project), [(0.0, 2.0), (2.0, 3.0), (3.0, 4.0)]);
        assert!(project.paste_graph_item("Nope", &copied, 0.0).is_none());
    }

    #[test]
    fn placing_needs_the_track_and_the_graph() {
        let mut project = project();
        let id = project.graph_id;
        assert!(project.place_graph("Graphs", id, 1.0, 2.0).is_some());
        assert!(project.place_graph("Graphs", 99, 0.0, 1.0).is_none());
        assert!(project.place_graph("Nope", id, 0.0, 1.0).is_none());
    }

    #[test]
    fn placing_over_items_trims_and_cuts_them() {
        let mut project = project();
        let id = project.graph_id;
        project.place_graph("Graphs", id, 0.0, 4.0);
        project.place_graph("Graphs", id, 4.0, 4.0);
        // Lands across the join: both are trimmed.
        let at = project.place_graph("Graphs", id, 3.0, 2.0).unwrap();
        assert_eq!(spans(&project), [(0.0, 3.0), (3.0, 5.0), (5.0, 8.0)]);
        assert_eq!(at, 1);
        // The tail keeps its place in graph time: its start moved by what was covered.
        assert_eq!(project.graph_tracks[0].items[2].start, 1.0);
        // Inside one item: it is cut in two.
        project.place_graph("Graphs", id, 6.0, 1.0);
        assert_eq!(
            spans(&project),
            [(0.0, 3.0), (3.0, 5.0), (5.0, 6.0), (6.0, 7.0), (7.0, 8.0)]
        );
    }

    #[test]
    fn moving_trims_what_it_lands_on_and_can_change_track() {
        let mut project = project();
        let id = project.graph_id;
        project.place_graph("Graphs", id, 0.0, 2.0);
        project.place_graph("Graphs", id, 3.0, 2.0);
        let at = project.move_graph_item("Graphs", 1, -2.0).unwrap();
        assert_eq!(spans(&project), [(0.0, 1.0), (1.0, 3.0)]);
        assert_eq!(at, 1);
        let top = project.add_graph_track("Graphs");
        assert_eq!(top, "Graphs_2");
        assert_eq!(
            project.graph_tracks[0].name, "Graphs_2",
            "new tracks go on top"
        );
        assert_eq!(project.move_graph_item_to("Graphs", 0, &top, 1.0), Some(0));
        assert_eq!(project.graph_tracks[0].items[0].position, 1.0);
        assert_eq!(project.graph_tracks[1].items.len(), 1);
    }

    #[test]
    fn trimming_the_left_edge_keeps_what_follows() {
        let mut project = project();
        let id = project.graph_id;
        project.place_graph("Graphs", id, 2.0, 4.0);
        project.place_graph("Graphs", id, 7.0, 1.0);
        project.trim_graph_item("Graphs", 0, Edge::Start, 3.0);
        let item = &project.graph_tracks[0].items[0];
        assert_eq!((item.position, item.start, item.length), (3.0, 1.0, 3.0));
        project.trim_graph_item("Graphs", 0, Edge::Start, 0.0);
        let item = &project.graph_tracks[0].items[0];
        assert_eq!((item.position, item.start), (2.0, 0.0));
        // The right edge stops at the next item.
        project.trim_graph_item("Graphs", 0, Edge::End, 20.0);
        assert_eq!(project.graph_tracks[0].items[0].end(), 7.0);
    }

    #[test]
    fn splitting_keeps_graph_time() {
        let mut project = project();
        let id = project.graph_id;
        project.place_graph("Graphs", id, 1.0, 4.0);
        assert_eq!(project.split_graph_item("Graphs", 0, 0.5), None);
        let second = project.split_graph_item("Graphs", 0, 2.0).unwrap();
        let items = &project.graph_tracks[0].items;
        assert_eq!((items[0].length, items[second].start), (1.0, 1.0));
    }

    #[test]
    fn renaming_a_track_follows_through_to_the_source_nodes_of_every_graph() {
        let mut project = project();
        project.add_track(TrackKind::Audio, "kick", "kick.wav");
        let lower = project.add_graph_track("Lower");
        let stored = project.add_graph("B", None);
        let mut b = project.graph_desc(stored).unwrap().clone();
        b.nodes
            .push(rastersong_graph::NodeDesc::new("g".into(), VIDEO_SOURCE));
        b.nodes.last_mut().unwrap().params.insert(
            SOURCE_PARAM.to_owned(),
            ParamValue::Text(format!("graph:{lower}")),
        );
        project
            .graphs
            .iter_mut()
            .find(|g| g.id == stored)
            .unwrap()
            .graph = b;
        let kick = Source::Track("kick".into());
        assert!(project.source_exists(&kick));
        assert!(project.rename_track("kick", "drum"));
        assert!(project.rename_track(&lower, "Bottom"));
        assert!(!project.source_exists(&kick));
        let sources = |desc: &GraphDesc| -> Vec<Source> {
            input_ports(desc)
                .into_iter()
                .map(|p| p.source)
                .filter(|s| *s != Source::Below)
                .collect()
        };
        assert_eq!(sources(&project.graph), [Source::Track("drum".into())]);
        assert_eq!(
            sources(project.graph_desc(stored).unwrap()),
            [Source::Graph {
                track: "Bottom".into(),
                port: "Video".into()
            }]
        );
        assert!(project.source_exists(&Source::Graph {
            track: "Bottom".into(),
            port: "Video".into()
        }));
    }

    #[test]
    fn the_composition_leaves_out_muted_graph_tracks_and_items() {
        let mut project = project();
        let id = project.graph_id;
        project.place_graph("Graphs", id, 0.0, 2.0);
        project.place_graph("Graphs", id, 3.0, 2.0);
        project.graph_tracks[0].items[1].muted = true;
        project.add_graph_track("Top");
        project.place_graph("Top", id, 0.0, 1.0);
        let lens = |p: &Project| -> Vec<usize> {
            p.composition()
                .graph_tracks
                .iter()
                .map(|t| t.items.len())
                .collect()
        };
        assert_eq!(lens(&project), [1, 1]);
        project.graph_tracks[0].muted = true;
        assert_eq!(lens(&project), [1]);
        project.graph_tracks[1].muted = true;
        assert!(lens(&project).is_empty());
        assert!(project.plays_track_mix());
    }

    #[test]
    fn graph_tracks_round_trip_and_overlaps_are_repaired() {
        let mut project = project();
        let id = project.graph_id;
        project.place_graph("Graphs", id, 1.0, 2.0);
        let json = serde_json::to_string(&project).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.graph_tracks, project.graph_tracks);
        let mut messy = project.graph_tracks.clone();
        let mut again = messy[0].items[0].clone();
        again.position = 2.0;
        messy[0].items.push(again);
        sanitize(&mut messy);
        assert_eq!(messy[0].items.len(), 2);
        assert!(messy[0].items[0].end() <= messy[0].items[1].position);
    }

    #[test]
    fn deleting_a_graph_removes_its_items() {
        let mut project = project();
        let id = project.graph_id;
        let other = project.add_graph("B", None);
        project.place_graph("Graphs", id, 0.0, 1.0);
        project.place_graph("Graphs", other, 1.0, 1.0);
        project.remove_graph_items(other);
        assert_eq!(project.graph_tracks[0].items.len(), 1);
        assert_eq!(project.graph_tracks[0].items[0].graph, id);
    }
}
