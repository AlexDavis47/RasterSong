//! Graph layers: lanes above the tracks holding graph items.
//!
//! A graph item places a graph (a resource) on the timeline. Items on a layer never overlap:
//! placing or moving one onto another trims what it covers. A layer has no item at some frames;
//! it is transparent there. Layers stack from the first (bottom, which reads the track mix) to
//! the last (top, whose outputs are the master). Each item binds the graph's Video Input and
//! Audio Input nodes to *Layer below* or to a track; an input with no binding reads zeros.
//! Bindings belong to the item, so one graph can sit twice reading different tracks.

use std::collections::BTreeMap;

use rastersong_graph::GraphDesc;
use rastersong_graph::nodes::{AUDIO_INPUT, VIDEO_INPUT};
use serde::{Deserialize, Serialize};

use super::{Edge, MIN_ITEM_LENGTH, Project};

/// What a graph's input is filled with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Binding {
    /// The layer below's picture or sound; under the bottom layer, the track mix's picture
    /// (its sound is not available as an input yet, so it reads zeros).
    LayerBelow,
    /// The track of this name; it must be a track of the input's kind.
    Track(String),
}

/// Whether an input node reads pictures or sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Video,
    Audio,
}

/// An input node of a graph: the ports an item's bindings fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputPort {
    pub node: String,
    pub kind: InputKind,
}

/// Graph `desc`'s input nodes in node order.
pub fn input_ports(desc: &GraphDesc) -> Vec<InputPort> {
    desc.nodes
        .iter()
        .filter_map(|n| {
            let kind = match n.kind.as_str() {
                VIDEO_INPUT => InputKind::Video,
                AUDIO_INPUT => InputKind::Audio,
                _ => return None,
            };
            Some(InputPort {
                node: n.id.clone(),
                kind,
            })
        })
        .collect()
}

fn pre_roll_default() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

/// A graph placed on a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphItem {
    /// The graph, by its id among the project's graphs.
    pub graph: u32,
    /// Seconds into the project where the item starts.
    pub position: f64,
    /// Seconds the item lasts.
    pub length: f64,
    /// Seconds into the item's own time where it starts: trimming the left edge moves this, so
    /// what follows doesn't change. Graph Progress counts from here.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub start: f64,
    /// A muted item reads as a gap.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
    /// Whether the graph warms up as if it had been running before the item, so trimming the
    /// left edge never changes what follows. Off, it starts cold at the edge.
    #[serde(default = "pre_roll_default", skip_serializing_if = "is_true")]
    pub pre_roll: bool,
    /// What each input node (by id) reads. An input without a binding reads zeros.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, Binding>,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl GraphItem {
    /// Where the item ends on the timeline.
    pub fn end(&self) -> f64 {
        self.position + self.length
    }

    /// Whether the item plays at timeline time `t`.
    pub fn plays_at(&self, t: f64) -> bool {
        !self.muted && t >= self.position && t < self.end()
    }
}

/// A lane of graph items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphLayer {
    pub name: String,
    /// Items by position, never overlapping.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<GraphItem>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub solo: bool,
}

/// An item as the engine renders it.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderItem {
    pub graph: u32,
    pub position: f64,
    pub length: f64,
    pub start: f64,
    pub pre_roll: bool,
    pub bindings: BTreeMap<String, Binding>,
}

/// What the engine renders when the project has graph items: the layers that sound or show
/// (bottom first; muted layers and items are left out) and the project's graphs.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerSet {
    pub layers: Vec<Vec<RenderItem>>,
    /// Every graph that isn't the open one, by id.
    pub graphs: Vec<(u32, GraphDesc)>,
    /// The id of the open graph, whose description the engine receives separately because it
    /// changes as it is edited.
    pub open: u32,
}

impl Project {
    /// Whether any layer holds a graph item. Without one nothing is applied: the output is the
    /// plain track mix, and the open graph is only a description until it is placed.
    pub fn has_graph_items(&self) -> bool {
        self.layers.iter().any(|l| !l.items.is_empty())
    }

    /// Whether the preview skips every graph and plays the track mix: the graph is bypassed, or
    /// no graph item is placed.
    pub fn plays_track_mix(&self) -> bool {
        self.bypass_graph || !self.has_graph_items()
    }

    /// Whether any layer is soloed.
    pub fn soloing_layers(&self) -> bool {
        self.layers.iter().any(|l| l.solo)
    }

    /// What the engine renders for the layers; `None` without graph items.
    pub fn layer_set(&self) -> Option<LayerSet> {
        if !self.has_graph_items() {
            return None;
        }
        let soloing = self.soloing_layers();
        Some(LayerSet {
            layers: self
                .layers
                .iter()
                .filter(|l| !l.muted && (!soloing || l.solo))
                .map(|l| {
                    l.items
                        .iter()
                        .filter(|i| !i.muted && i.length > 0.0)
                        .map(|i| RenderItem {
                            graph: i.graph,
                            position: i.position,
                            length: i.length,
                            start: i.start,
                            pre_roll: i.pre_roll,
                            bindings: i.bindings.clone(),
                        })
                        .collect()
                })
                .collect(),
            graphs: self
                .graphs
                .iter()
                .map(|g| (g.id, g.graph.clone()))
                .collect(),
            open: self.graph_id,
        })
    }

    /// The description of graph `id`, open or stored.
    pub fn graph_desc(&self, id: u32) -> Option<&GraphDesc> {
        if id == self.graph_id {
            Some(&self.graph)
        } else {
            self.graphs.iter().find(|g| g.id == id).map(|g| &g.graph)
        }
    }

    /// Adds an empty layer on top and returns its index.
    pub fn add_layer(&mut self, name: &str) -> usize {
        let name = (1..)
            .map(|n| {
                if n == 1 {
                    name.to_owned()
                } else {
                    format!("{name} {n}")
                }
            })
            .find(|candidate| !self.layers.iter().any(|l| l.name == *candidate))
            .expect("an unused name");
        self.layers.push(GraphLayer {
            name,
            items: Vec::new(),
            muted: false,
            solo: false,
        });
        self.layers.len() - 1
    }

    /// Removes layer `layer` with its items.
    pub fn remove_layer(&mut self, layer: usize) -> bool {
        if layer >= self.layers.len() {
            return false;
        }
        self.layers.remove(layer);
        true
    }

    /// Renames layer `layer`; false if the name is blank or another layer has it.
    pub fn rename_layer(&mut self, layer: usize, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty()
            || self
                .layers
                .iter()
                .enumerate()
                .any(|(i, l)| i != layer && l.name == name)
        {
            return false;
        }
        match self.layers.get_mut(layer) {
            Some(l) => {
                l.name = name.to_owned();
                true
            }
            None => false,
        }
    }

    /// Places graph `graph` on layer `layer` from `at` for `length` seconds, trimming whatever
    /// it lands on, with every input bound to *Layer below*. Returns the item's index, or
    /// `None` for a layer or graph that doesn't exist.
    pub fn place_graph(&mut self, layer: usize, graph: u32, at: f64, length: f64) -> Option<usize> {
        let desc = self.graph_desc(graph)?;
        let bindings = input_ports(desc)
            .into_iter()
            .map(|p| (p.node, Binding::LayerBelow))
            .collect();
        let item = GraphItem {
            graph,
            position: at.max(0.0),
            length: length.max(MIN_ITEM_LENGTH),
            start: 0.0,
            muted: false,
            pre_roll: true,
            bindings,
        };
        let layer = self.layers.get_mut(layer)?;
        Some(insert_over(&mut layer.items, item))
    }

    /// Moves item `item` of layer `layer` by `delta` seconds (not before the timeline's start),
    /// trimming what it lands on, and returns its new index.
    pub fn move_graph_item(&mut self, layer: usize, item: usize, delta: f64) -> Option<usize> {
        let items = &mut self.layers.get_mut(layer)?.items;
        if item >= items.len() || !delta.is_finite() {
            return None;
        }
        let mut moved = items.remove(item);
        moved.position = (moved.position + delta).max(0.0);
        Some(insert_over(items, moved))
    }

    /// Moves edge `edge` of item `item` to timeline time `to`, within the gaps either side and
    /// leaving the item at least [`MIN_ITEM_LENGTH`] long. Trimming the left edge keeps what
    /// follows where it is.
    pub fn trim_graph_item(&mut self, layer: usize, item: usize, edge: Edge, to: f64) {
        let Some(items) = self.layers.get_mut(layer).map(|l| &mut l.items) else {
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
                // The item's own start can't go below zero: it has no more graph time before it.
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
    pub fn split_graph_item(&mut self, layer: usize, item: usize, at: f64) -> Option<usize> {
        let items = &mut self.layers.get_mut(layer)?.items;
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

    /// Removes item `item` of layer `layer`.
    pub fn delete_graph_item(&mut self, layer: usize, item: usize) -> bool {
        match self.layers.get_mut(layer) {
            Some(l) if item < l.items.len() => {
                l.items.remove(item);
                true
            }
            _ => false,
        }
    }

    /// Binds input node `node` of an item to `binding`, or to nothing for `None`.
    pub fn set_graph_binding(
        &mut self,
        layer: usize,
        item: usize,
        node: &str,
        binding: Option<Binding>,
    ) {
        let Some(it) = self
            .layers
            .get_mut(layer)
            .and_then(|l| l.items.get_mut(item))
        else {
            return;
        };
        match binding {
            Some(b) => {
                it.bindings.insert(node.to_owned(), b);
            }
            None => {
                it.bindings.remove(node);
            }
        }
    }

    /// Removes every item that places graph `graph`; used when the graph is deleted.
    pub fn remove_graph_items(&mut self, graph: u32) {
        for layer in &mut self.layers {
            layer.items.retain(|i| i.graph != graph);
        }
    }

    /// The tracks that bindings refer to and no longer exist, per item: (layer, item, input).
    pub fn dangling_bindings(&self) -> Vec<(usize, usize, String)> {
        let mut found = Vec::new();
        for (l, layer) in self.layers.iter().enumerate() {
            for (i, item) in layer.items.iter().enumerate() {
                for (node, binding) in &item.bindings {
                    if let Binding::Track(name) = binding
                        && !self.has_track(name)
                    {
                        found.push((l, i, node.clone()));
                    }
                }
            }
        }
        found
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

/// `layers` with nonsense (non-finite or negative times, overlaps) repaired, for files.
pub(super) fn sanitize(layers: &mut [GraphLayer]) {
    for layer in layers {
        for item in &mut layer.items {
            let finite = |v: f64| if v.is_finite() { v.max(0.0) } else { 0.0 };
            item.position = finite(item.position);
            item.start = finite(item.start);
            item.length = finite(item.length).max(MIN_ITEM_LENGTH);
        }
        let items = std::mem::take(&mut layer.items);
        for item in items {
            insert_over(&mut layer.items, item);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        let graph = GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [
                { "id": "v", "type": "video_input" },
                { "id": "a", "type": "audio_input" },
                { "id": "o", "type": "output" } ] }"#,
        )
        .unwrap();
        let mut project = Project::new(graph);
        project.add_layer("Layer");
        project
    }

    fn spans(project: &Project) -> Vec<(f64, f64)> {
        project.layers[0]
            .items
            .iter()
            .map(|i| (i.position, i.end()))
            .collect()
    }

    #[test]
    fn placing_binds_every_input_to_the_layer_below() {
        let mut project = project();
        let at = project.place_graph(0, project.graph_id, 1.0, 2.0).unwrap();
        let item = &project.layers[0].items[at];
        assert_eq!(item.bindings.len(), 2);
        assert!(item.bindings.values().all(|b| *b == Binding::LayerBelow));
        assert!(project.place_graph(0, 99, 0.0, 1.0).is_none());
        assert!(project.place_graph(5, project.graph_id, 0.0, 1.0).is_none());
    }

    #[test]
    fn placing_over_items_trims_and_cuts_them() {
        let mut project = project();
        let id = project.graph_id;
        project.place_graph(0, id, 0.0, 4.0);
        project.place_graph(0, id, 4.0, 4.0);
        // Lands across the join: both are trimmed.
        let at = project.place_graph(0, id, 3.0, 2.0).unwrap();
        assert_eq!(spans(&project), [(0.0, 3.0), (3.0, 5.0), (5.0, 8.0)]);
        assert_eq!(at, 1);
        // The tail keeps its place in graph time: its start moved by what was covered.
        assert_eq!(project.layers[0].items[2].start, 1.0);
        // Inside one item: it is cut in two.
        project.place_graph(0, id, 6.0, 1.0);
        assert_eq!(
            spans(&project),
            [(0.0, 3.0), (3.0, 5.0), (5.0, 6.0), (6.0, 7.0), (7.0, 8.0)]
        );
        // Covering one whole item removes it.
        project.place_graph(0, id, 2.5, 3.0);
        assert_eq!(
            spans(&project),
            [(0.0, 2.5), (2.5, 5.5), (5.5, 6.0), (6.0, 7.0), (7.0, 8.0)]
        );
    }

    #[test]
    fn moving_trims_what_it_lands_on_and_stops_at_zero() {
        let mut project = project();
        let id = project.graph_id;
        project.place_graph(0, id, 0.0, 2.0);
        project.place_graph(0, id, 3.0, 2.0);
        let at = project.move_graph_item(0, 1, -2.0).unwrap();
        assert_eq!(spans(&project), [(0.0, 1.0), (1.0, 3.0)]);
        assert_eq!(at, 1);
        project.move_graph_item(0, 1, -10.0);
        assert_eq!(project.layers[0].items.len(), 1);
        assert_eq!(spans(&project), [(0.0, 2.0)]);
    }

    #[test]
    fn trimming_the_left_edge_keeps_what_follows() {
        let mut project = project();
        let id = project.graph_id;
        project.place_graph(0, id, 2.0, 4.0);
        project.place_graph(0, id, 7.0, 1.0);
        project.trim_graph_item(0, 0, Edge::Start, 3.0);
        let item = &project.layers[0].items[0];
        assert_eq!((item.position, item.start, item.length), (3.0, 1.0, 3.0));
        // Back out: the graph's own time can't go below its start.
        project.trim_graph_item(0, 0, Edge::Start, 0.0);
        let item = &project.layers[0].items[0];
        assert_eq!((item.position, item.start), (2.0, 0.0));
        // The right edge stops at the next item.
        project.trim_graph_item(0, 0, Edge::End, 20.0);
        assert_eq!(project.layers[0].items[0].end(), 7.0);
    }

    #[test]
    fn splitting_keeps_bindings_and_graph_time() {
        let mut project = project();
        let id = project.graph_id;
        project.place_graph(0, id, 1.0, 4.0);
        project.set_graph_binding(0, 0, "v", Some(Binding::Track("clip".into())));
        assert_eq!(project.split_graph_item(0, 0, 0.5), None);
        let second = project.split_graph_item(0, 0, 2.0).unwrap();
        let items = &project.layers[0].items;
        assert_eq!((items[0].length, items[second].start), (1.0, 1.0));
        assert_eq!(items[second].bindings, items[0].bindings);
        assert_eq!(items[second].end(), 5.0);
    }

    #[test]
    fn layer_set_leaves_out_muted_layers_items_and_unsoloed_layers() {
        let mut project = project();
        let id = project.graph_id;
        project.place_graph(0, id, 0.0, 2.0);
        project.place_graph(0, id, 3.0, 2.0);
        project.layers[0].items[1].muted = true;
        project.add_layer("Layer");
        project.place_graph(1, id, 0.0, 1.0);
        let set = project.layer_set().unwrap();
        assert_eq!(set.layers.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1]);
        project.layers[1].solo = true;
        assert_eq!(
            project
                .layer_set()
                .unwrap()
                .layers
                .iter()
                .map(Vec::len)
                .collect::<Vec<_>>(),
            [1]
        );
        project.layers[1].muted = true;
        assert_eq!(project.layer_set().unwrap().layers.len(), 0);
        assert!(Project::new(project.graph.clone()).layer_set().is_none());
    }

    #[test]
    fn layers_round_trip_through_a_file_and_overlaps_are_repaired() {
        let mut project = project();
        let id = project.graph_id;
        project.place_graph(0, id, 1.0, 2.0);
        project.layers[0].solo = true;
        let json = serde_json::to_string(&project).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.layers, project.layers);
        let mut messy = project.layers.clone();
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
        project.place_graph(0, id, 0.0, 1.0);
        project.place_graph(0, other, 1.0, 1.0);
        project.remove_graph_items(other);
        assert_eq!(project.layers[0].items.len(), 1);
        assert_eq!(project.layers[0].items[0].graph, id);
    }
}
