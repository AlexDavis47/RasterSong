//! The node editor: the graph as the user edits it, drawn and edited on a pannable, zoomable
//! canvas. The project's [`GraphDesc`] stays the model; the editor converts to and from it.

mod canvas;
mod inspector;
mod linked;
mod modulation;
mod param_field;
mod search;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use eframe::egui::{Pos2, Rect, Vec2, pos2, vec2};
use rastersong_engine::{
    Channels, Connection, FORMAT_VERSION, GraphDesc, Interpolation, Modulation, NodeDesc, NodeType,
    OutputLevel, ParamValue, Registry,
};

pub use canvas::CanvasContext;
pub use inspector::InspectorContext;
pub use modulation::{PARAM_PORT, as_param, param_port};

/// Identifies a node in the editor, stable across renames.
pub type NodeKey = u64;

#[derive(Debug, Clone, PartialEq)]
pub struct EditorNode {
    pub key: NodeKey,
    /// The id in graph files and errors.
    pub id: String,
    pub kind: String,
    /// The user's name for the node, shown instead of the type's.
    pub label: Option<String>,
    pub params: BTreeMap<String, ParamValue>,
    pub interpolation: Interpolation,
    pub channels: Channels,
    /// Top-left corner in graph space.
    pub pos: Pos2,
    /// How connected signals move parameters, by parameter name.
    pub modulation: BTreeMap<String, Modulation>,
    /// The parameters showing pins, when the user changed them from the type's defaults.
    pub exposed: Option<BTreeSet<String>>,
}

/// A connection from an output (node, port) to an input (node, port).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Wire {
    pub from: (NodeKey, usize),
    pub to: (NodeKey, usize),
}

/// The pan and zoom of the canvas: screen = canvas origin + offset + graph × zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub offset: Vec2,
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            offset: vec2(40.0, 40.0),
            zoom: 1.0,
        }
    }
}

pub struct GraphEditor {
    registry: Registry,
    nodes: Vec<EditorNode>,
    wires: Vec<Wire>,
    next_key: NodeKey,
    selected: BTreeSet<NodeKey>,
    /// The node shown in the inspector.
    active: Option<NodeKey>,
    /// When each node was last brought to the front; nodes are drawn in this order (unraised
    /// ones first, in graph order). Kept apart from `nodes` so raising a node doesn't reorder
    /// the graph, which would count as an edit.
    raised: HashMap<NodeKey, u64>,
    view: View,
    /// Fit the view to the graph the next time it's drawn.
    fit_pending: bool,
    interaction: canvas::Interaction,
    search: Option<search::SearchMenu>,
    node_menu: Option<search::NodeMenu>,
    /// The canvas and node layout as last drawn, for hit-testing from outside (tests).
    last_canvas: Rect,
    last_geometry: Vec<canvas::Geometry>,
    /// Text last copied, for the Edit menu's Paste (keyboard paste reads the system clipboard).
    clipboard: Option<String>,
    /// The project's video file name and audio track names, for the nodes linked to them.
    project_video: Option<String>,
    project_tracks: Vec<String>,
    /// Problems found when loading a graph (e.g. connections to ports that don't exist).
    pub warnings: Vec<String>,
}

/// How far duplicates are placed from their originals, in graph space.
const DUPLICATE_OFFSET: Vec2 = vec2(30.0, 30.0);

impl std::fmt::Debug for GraphEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphEditor")
            .field("nodes", &self.nodes.len())
            .field("wires", &self.wires.len())
            .finish_non_exhaustive()
    }
}

impl GraphEditor {
    pub fn new(graph: &GraphDesc) -> Self {
        let mut editor = Self {
            registry: Registry::default(),
            nodes: Vec::new(),
            wires: Vec::new(),
            next_key: 1,
            selected: BTreeSet::new(),
            active: None,
            raised: HashMap::new(),
            view: View::default(),
            fit_pending: true,
            interaction: canvas::Interaction::default(),
            search: None,
            node_menu: None,
            last_canvas: Rect::NOTHING,
            last_geometry: Vec::new(),
            clipboard: None,
            project_video: None,
            project_tracks: Vec::new(),
            warnings: Vec::new(),
        };
        editor.load(graph);
        editor
    }

    /// Replaces the edited graph.
    pub fn load(&mut self, graph: &GraphDesc) {
        self.selected.clear();
        self.active = None;
        self.raised.clear();
        self.fit_pending = true;
        self.set_graph(graph);
    }

    /// Replaces the edited graph with an earlier state of the same one (undo or redo), keeping
    /// the view, and the selection where its nodes still exist.
    pub fn restore(&mut self, graph: &GraphDesc) {
        let id_of = |editor: &Self, key: NodeKey| editor.node(key).map(|n| n.id.clone());
        let selected: Vec<String> = self
            .selected
            .iter()
            .filter_map(|&k| id_of(self, k))
            .collect();
        let active = self.active.and_then(|k| id_of(self, k));
        self.set_graph(graph);
        self.selected = selected.iter().filter_map(|id| self.key_of(id)).collect();
        self.active = active.and_then(|id| self.key_of(&id));
        self.raised.clear();
    }

    fn set_graph(&mut self, graph: &GraphDesc) {
        self.nodes.clear();
        self.wires.clear();
        self.selected.clear();
        self.active = None;
        self.search = None;
        self.node_menu = None;
        self.interaction = canvas::Interaction::Idle;
        self.warnings.clear();
        let positions = auto_layout(graph);
        let mut keys = HashMap::new();
        for (i, node) in graph.nodes.iter().enumerate() {
            let key = self.fresh_key();
            keys.insert(node.id.as_str(), key);
            self.nodes.push(EditorNode {
                key,
                id: node.id.clone(),
                kind: node.kind.clone(),
                label: node.label.clone(),
                params: node.params.clone(),
                interpolation: node.interpolation,
                channels: node.channels,
                pos: node.position.map_or(positions[i], |[x, y]| pos2(x, y)),
                modulation: node.modulation.clone(),
                exposed: node.exposed.as_ref().map(|e| e.iter().cloned().collect()),
            });
        }
        for c in &graph.connections {
            match (
                self.endpoint(&keys, &c.from, false),
                self.endpoint(&keys, &c.to, true),
            ) {
                (Some(from), Some(to)) => self.connect(from, to),
                _ => self
                    .warnings
                    .push(format!("dropped connection {} -> {}", c.from, c.to)),
            }
        }
    }

    /// Draws `key` above every other node.
    fn raise(&mut self, key: NodeKey) {
        let top = self.raised.values().max().map_or(1, |z| z + 1);
        self.raised.insert(key, top);
    }

    /// Node indices in drawing order, back to front.
    fn draw_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.nodes.len()).collect();
        order.sort_by_key(|&i| self.raised.get(&self.nodes[i].key).copied().unwrap_or(0));
        order
    }

    fn fresh_key(&mut self) -> NodeKey {
        self.next_key += 1;
        self.next_key
    }

    /// Resolves `"node.port"` to a node and port index.
    fn endpoint(
        &self,
        keys: &HashMap<&str, NodeKey>,
        endpoint: &str,
        input: bool,
    ) -> Option<(NodeKey, usize)> {
        let (id, port) = match endpoint.split_once('.') {
            Some((id, port)) => (id, Some(port)),
            None => (endpoint, None),
        };
        let key = *keys.get(id)?;
        let kind = self.kind_of(key)?;
        // A parameter: `node.@name`.
        if let (true, Some(param)) = (input, port.and_then(|p| p.strip_prefix('@'))) {
            let index = kind
                .spec
                .params
                .iter()
                .position(|s| s.name == param && s.modulatable)?;
            return Some((key, modulation::param_port(index)));
        }
        let index = match (port, input) {
            (None, _) => 0,
            (Some(port), true) => kind.inputs.iter().position(|i| i.name == port)?,
            (Some(port), false) => editor_outputs(kind).iter().position(|&o| o == port)?,
        };
        let count = if input {
            kind.inputs.len()
        } else {
            editor_outputs(kind).len()
        };
        (index < count).then_some((key, index))
    }

    /// The edited graph, including names and positions.
    pub fn to_desc(&self) -> GraphDesc {
        let port = |(key, index): (NodeKey, usize), input: bool| -> String {
            let node = self.node(key).expect("wires only join existing nodes");
            let name = self
                .kind_of(key)
                .map(|k| match modulation::as_param(index) {
                    Some(param) if input => format!("@{}", k.spec.params[param].name),
                    _ if input => k.inputs[index].name.to_owned(),
                    _ => k.outputs[index].to_owned(),
                });
            format!("{}.{}", node.id, name.as_deref().unwrap_or("?"))
        };
        let mut wires = self.wires.clone();
        wires.sort_by_key(|w| {
            (
                self.index_of(w.to.0),
                w.to.1,
                self.index_of(w.from.0),
                w.from.1,
            )
        });
        GraphDesc {
            version: FORMAT_VERSION,
            nodes: self
                .nodes
                .iter()
                .map(|n| NodeDesc {
                    id: n.id.clone(),
                    kind: n.kind.clone(),
                    params: n.params.clone(),
                    interpolation: n.interpolation,
                    channels: n.channels,
                    label: n.label.clone(),
                    position: Some([n.pos.x.round(), n.pos.y.round()]),
                    modulation: n.modulation.clone(),
                    exposed: n.exposed.as_ref().map(|e| e.iter().cloned().collect()),
                })
                .collect(),
            connections: wires
                .iter()
                .map(|w| Connection {
                    from: port(w.from, false),
                    to: port(w.to, true),
                })
                .collect(),
        }
    }

    pub fn node(&self, key: NodeKey) -> Option<&EditorNode> {
        self.nodes.iter().find(|n| n.key == key)
    }

    fn node_mut(&mut self, key: NodeKey) -> Option<&mut EditorNode> {
        self.nodes.iter_mut().find(|n| n.key == key)
    }

    fn index_of(&self, key: NodeKey) -> usize {
        self.nodes
            .iter()
            .position(|n| n.key == key)
            .unwrap_or(usize::MAX)
    }

    fn kind_of(&self, key: NodeKey) -> Option<&NodeType> {
        self.node(key).and_then(|n| self.registry.get(&n.kind))
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn wires(&self) -> &[Wire] {
        &self.wires
    }

    pub fn key_of(&self, id: &str) -> Option<NodeKey> {
        self.nodes.iter().find(|n| n.id == id).map(|n| n.key)
    }

    /// The node shown in the inspector.
    pub fn active(&self) -> Option<&EditorNode> {
        self.active.and_then(|k| self.node(k))
    }

    pub fn set_active(&mut self, key: Option<NodeKey>) {
        self.active = key;
        if let Some(key) = key {
            self.selected = BTreeSet::from([key]);
        }
    }

    pub fn selected(&self) -> &BTreeSet<NodeKey> {
        &self.selected
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn search_open(&self) -> bool {
        self.search.is_some()
    }

    /// The canvas as last drawn, in screen space.
    pub fn canvas_rect(&self) -> Rect {
        self.last_canvas
    }

    /// Where a node was last drawn, in screen space.
    pub fn node_screen_rect(&self, key: NodeKey) -> Option<Rect> {
        let g = self.last_geometry.iter().find(|g| g.key == key)?;
        let to_screen = |p| canvas::graph_to_screen(self.view, self.last_canvas, p);
        Some(Rect::from_min_max(
            to_screen(g.rect.min),
            to_screen(g.rect.max),
        ))
    }

    /// Where a pin was last drawn, in screen space. For inputs, `port` is the input's index, or
    /// a parameter's port ([`param_port`]).
    pub fn pin_screen_pos(&self, key: NodeKey, input: bool, port: usize) -> Option<Pos2> {
        let g = self.last_geometry.iter().find(|g| g.key == key)?;
        let p = if input {
            g.inputs.iter().find(|pin| pin.port == port)?.pos
        } else {
            g.outputs.get(port)?.0
        };
        Some(canvas::graph_to_screen(self.view, self.last_canvas, p))
    }

    /// Adds a node of `kind` at `pos` (graph space) with a fresh id. Returns its key.
    pub fn add_node(&mut self, kind: &str, pos: Pos2) -> Option<NodeKey> {
        self.registry.get(kind)?;
        let id = (1..)
            .map(|n| {
                if n == 1 {
                    kind.to_owned()
                } else {
                    format!("{kind}_{n}")
                }
            })
            .find(|candidate| !self.nodes.iter().any(|n| &n.id == candidate))
            .unwrap();
        let key = self.fresh_key();
        self.nodes.push(EditorNode {
            key,
            id,
            kind: kind.to_owned(),
            label: None,
            params: BTreeMap::new(),
            interpolation: Interpolation::Hold,
            channels: Channels::Together,
            pos,
            modulation: BTreeMap::new(),
            exposed: None,
        });
        Some(key)
    }

    /// Connects an output to an input. An input takes one connection; a new one replaces the old.
    pub fn connect(&mut self, from: (NodeKey, usize), to: (NodeKey, usize)) {
        if from.0 == to.0 {
            return;
        }
        self.wires.retain(|w| w.to != to);
        self.wires.push(Wire { from, to });
    }

    pub fn disconnect_input(&mut self, to: (NodeKey, usize)) -> Option<Wire> {
        let index = self.wires.iter().position(|w| w.to == to)?;
        Some(self.wires.remove(index))
    }

    /// Removes nodes and their wires. Nodes linked to the project are kept.
    pub fn remove_nodes(&mut self, keys: &BTreeSet<NodeKey>) {
        let keys: BTreeSet<NodeKey> = keys
            .iter()
            .copied()
            .filter(|&k| !self.is_linked(k))
            .collect();
        self.remove_nodes_unchecked(&keys);
    }

    fn remove_nodes_unchecked(&mut self, keys: &BTreeSet<NodeKey>) {
        self.nodes.retain(|n| !keys.contains(&n.key));
        self.wires
            .retain(|w| !keys.contains(&w.from.0) && !keys.contains(&w.to.0));
        self.selected.retain(|k| !keys.contains(k));
        if self.active.is_some_and(|k| keys.contains(&k)) {
            self.active = None;
        }
    }

    /// Copies the given nodes (and the wires between them) next to the originals, and selects
    /// the copies.
    pub fn duplicate(&mut self, keys: &BTreeSet<NodeKey>) {
        let fragment = self.fragment(keys);
        self.insert(&fragment, DUPLICATE_OFFSET);
    }

    /// The given nodes and the connections between them, as a graph of their own: what Copy
    /// puts on the clipboard.
    pub fn fragment(&self, keys: &BTreeSet<NodeKey>) -> GraphDesc {
        let mut graph = self.to_desc();
        let ids: BTreeSet<String> = keys
            .iter()
            .filter(|&&k| !self.is_linked(k))
            .filter_map(|&k| self.node(k).map(|n| n.id.clone()))
            .collect();
        let node_of = |endpoint: &str| endpoint.split('.').next().unwrap_or("").to_owned();
        graph.nodes.retain(|n| ids.contains(&n.id));
        graph
            .connections
            .retain(|c| ids.contains(&node_of(&c.from)) && ids.contains(&node_of(&c.to)));
        graph
    }

    /// Adds the nodes of `fragment` (with fresh ids) and its connections, moved by `offset`, and
    /// selects them. Nodes of unknown types and connections that don't resolve are skipped.
    /// Returns the keys of the new nodes.
    pub fn insert(&mut self, fragment: &GraphDesc, offset: Vec2) -> Vec<NodeKey> {
        let positions = auto_layout(fragment);
        let mut keys = HashMap::new();
        for (i, desc) in fragment.nodes.iter().enumerate() {
            // The project's inputs and output come from the project, not the clipboard.
            let addable = self
                .registry
                .get(&desc.kind)
                .is_some_and(|k| Self::user_addable(k.spec.category));
            if !addable {
                continue;
            }
            let pos = desc.position.map_or(positions[i], |[x, y]| pos2(x, y)) + offset;
            let Some(key) = self.add_node(&desc.kind, pos) else {
                continue;
            };
            let node = self.node_mut(key).unwrap();
            node.params = desc.params.clone();
            node.interpolation = desc.interpolation;
            node.channels = desc.channels;
            node.label = desc.label.clone();
            node.modulation = desc.modulation.clone();
            node.exposed = desc.exposed.as_ref().map(|e| e.iter().cloned().collect());
            keys.insert(desc.id.as_str(), key);
        }
        for c in &fragment.connections {
            if let (Some(from), Some(to)) = (
                self.endpoint(&keys, &c.from, false),
                self.endpoint(&keys, &c.to, true),
            ) {
                self.connect(from, to);
            }
        }
        let added: Vec<NodeKey> = fragment
            .nodes
            .iter()
            .filter_map(|n| keys.get(n.id.as_str()).copied())
            .collect();
        self.selected = added.iter().copied().collect();
        self.active = (added.len() == 1).then(|| added[0]);
        added
    }

    /// The selected nodes as clipboard text, or `None` if nothing is selected.
    pub fn copy_selection(&mut self) -> Option<String> {
        if self.selected.is_empty() {
            return None;
        }
        let text = self.fragment(&self.selected).to_json();
        self.clipboard = Some(text.clone());
        Some(text)
    }

    /// Pastes clipboard text (from [`Self::copy_selection`]) with its top-left node at `at`
    /// (graph space). Text that isn't a graph is ignored.
    pub fn paste(&mut self, text: &str, at: Pos2) -> bool {
        let Ok(fragment) = GraphDesc::from_json(text) else {
            return false;
        };
        let corner = fragment
            .nodes
            .iter()
            .filter_map(|n| n.position)
            .map(|[x, y]| pos2(x, y))
            .reduce(|a, b| a.min(b))
            .unwrap_or(Pos2::ZERO);
        !self.insert(&fragment, at - corner).is_empty()
    }

    /// The text last copied from this editor, for pasting from the Edit menu.
    pub fn clipboard(&self) -> Option<&str> {
        self.clipboard.as_deref()
    }

    /// Where pasted nodes go when the pointer isn't over the canvas: the middle of the view.
    pub fn view_center(&self) -> Pos2 {
        ((self.last_canvas.size() / 2.0 - self.view.offset) / self.view.zoom).to_pos2()
    }

    /// Selects every node.
    pub fn select_all(&mut self) {
        self.selected = self.nodes.iter().map(|n| n.key).collect();
    }

    /// Removes the selected nodes.
    pub fn delete_selection(&mut self) {
        let selected = self.selected.clone();
        self.remove_nodes(&selected);
    }

    /// Duplicates the selected nodes.
    pub fn duplicate_selection(&mut self) {
        let selected = self.selected.clone();
        self.duplicate(&selected);
    }

    /// Points audio inputs that read track `old` at `new` (after a track is renamed).
    pub fn rename_track(&mut self, old: &str, new: &str) {
        for node in self.nodes.iter_mut().filter(|n| n.kind == "audio_input") {
            let reads_old = match node.params.get("source") {
                Some(ParamValue::Text(name)) => name == old,
                _ => old == rastersong_engine::DEFAULT_AUDIO_TRACK,
            };
            if reads_old {
                node.params
                    .insert("source".into(), ParamValue::Text(new.to_owned()));
            }
        }
    }

    /// Levels by output, from a rendered frame.
    fn level_map(levels: &[OutputLevel]) -> HashMap<(&str, usize), f32> {
        levels
            .iter()
            .map(|l| ((&*l.node, l.output), l.rms))
            .collect()
    }
}

/// Output ports the editor shows. The output node's own port is how the graph returns its
/// result; nothing connects to it.
pub(crate) fn editor_outputs(kind: &NodeType) -> &'static [&'static str] {
    if kind.kind == "output" {
        &[]
    } else {
        kind.outputs
    }
}

/// Removes everything that doesn't affect rendering (positions, names and the order nodes and
/// connections are listed in), so moving or renaming nodes doesn't re-render.
pub fn without_layout(graph: &GraphDesc) -> GraphDesc {
    let mut graph = graph.clone();
    for node in &mut graph.nodes {
        node.position = None;
        node.label = None;
        node.exposed = None;
    }
    graph.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    graph
        .connections
        .sort_by(|a, b| (&a.to, &a.from).cmp(&(&b.to, &b.from)));
    graph
}

/// Positions for nodes that don't have one: columns by depth from the sources.
fn auto_layout(graph: &GraphDesc) -> Vec<Pos2> {
    let index: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let node_of = |endpoint: &str| index.get(endpoint.split('.').next().unwrap_or("")).copied();
    let mut depth = vec![0usize; graph.nodes.len()];
    // Longest path, relaxed repeatedly (graphs are small; cycles stop after `len` rounds).
    for _ in 0..graph.nodes.len() {
        for c in &graph.connections {
            if let (Some(from), Some(to)) = (node_of(&c.from), node_of(&c.to)) {
                depth[to] = depth[to].max(depth[from] + 1);
            }
        }
    }
    let mut rows = HashMap::new();
    depth
        .iter()
        .map(|&d| {
            let row = rows.entry(d).or_insert(0usize);
            *row += 1;
            pos2(d as f32 * 190.0, (*row - 1) as f32 * 110.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRAPH: &str = include_str!("../../../../examples/graphs/am_bands.json");

    fn graph() -> GraphDesc {
        GraphDesc::from_json(GRAPH).unwrap()
    }

    /// Connections with every port written out, sorted, for comparing graphs.
    fn canonical(g: &GraphDesc) -> Vec<(String, String)> {
        let registry = Registry::default();
        let full = |e: &str, input: bool| {
            if e.contains('.') {
                return e.to_owned();
            }
            let kind = &g.nodes.iter().find(|n| n.id == e).unwrap().kind;
            let kind = registry.get(kind).unwrap();
            format!(
                "{e}.{}",
                if input {
                    kind.inputs[0].name
                } else {
                    kind.outputs[0]
                }
            )
        };
        let mut c: Vec<_> = g
            .connections
            .iter()
            .map(|c| (full(&c.from, false), full(&c.to, true)))
            .collect();
        c.sort();
        c
    }

    #[test]
    fn round_trips_graphs() {
        let graph = graph();
        let editor = GraphEditor::new(&graph);
        assert!(editor.warnings.is_empty(), "{:?}", editor.warnings);
        let back = editor.to_desc();
        assert_eq!(without_layout(&back).nodes, without_layout(&graph).nodes);
        assert_eq!(canonical(&back), canonical(&graph));
        // Loading the editor's own output gives the same graph again, positions included.
        assert_eq!(GraphEditor::new(&back).to_desc(), back);
    }

    #[test]
    fn drops_connections_to_unknown_ports() {
        let graph = GraphDesc::from_json(
            r#"{ "version": 1, "nodes": [ { "id": "v", "type": "video_input" }, { "id": "o", "type": "output" } ],
                 "connections": [ { "from": "v.nope", "to": "o" } ] }"#,
        )
        .unwrap();
        let editor = GraphEditor::new(&graph);
        assert_eq!(editor.warnings.len(), 1);
        assert!(editor.to_desc().connections.is_empty());
    }

    #[test]
    fn new_nodes_get_unique_ids() {
        let mut editor =
            GraphEditor::new(&GraphDesc::from_json(r#"{ "version": 1, "nodes": [] }"#).unwrap());
        let a = editor.add_node("delay", Pos2::ZERO).unwrap();
        let b = editor.add_node("delay", Pos2::ZERO).unwrap();
        assert_eq!(editor.node(a).unwrap().id, "delay");
        assert_eq!(editor.node(b).unwrap().id, "delay_2");
        assert_eq!(editor.add_node("nope", Pos2::ZERO), None);
    }

    #[test]
    fn inputs_take_one_connection() {
        let mut editor = GraphEditor::new(&graph());
        let video = editor.key_of("video").unwrap();
        let audio = editor.key_of("audio").unwrap();
        let split = editor.key_of("split").unwrap();
        editor.connect((audio, 0), (split, 0));
        let into_split: Vec<_> = editor
            .wires()
            .iter()
            .filter(|w| w.to == (split, 0))
            .collect();
        assert_eq!(into_split.len(), 1);
        assert_eq!(into_split[0].from.0, audio);
        editor.connect((video, 0), (video, 0));
        assert!(
            !editor.wires().iter().any(|w| w.from.0 == w.to.0),
            "no self-connections"
        );
    }

    #[test]
    fn duplicating_keeps_internal_wires_and_removing_drops_wires() {
        let mut editor = GraphEditor::new(&graph());
        let split = editor.key_of("split").unwrap();
        let red = editor.key_of("am_red").unwrap();
        let wires_before = editor.wires().len();
        editor.duplicate(&BTreeSet::from([split, red]));
        assert_eq!(editor.node_count(), 11);
        // split.r -> am_red.carrier is duplicated between the copies.
        assert_eq!(editor.wires().len(), wires_before + 1);

        editor.remove_nodes(&BTreeSet::from([split]));
        assert!(
            !editor
                .wires()
                .iter()
                .any(|w| w.from.0 == split || w.to.0 == split)
        );
    }

    #[test]
    fn copy_and_paste_keeps_internal_wires_and_places_at_the_target() {
        let mut editor = GraphEditor::new(&graph());
        let split = editor.key_of("split").unwrap();
        let red = editor.key_of("am_red").unwrap();
        let wires = editor.wires().len();
        editor.selected = BTreeSet::from([split, red]);
        let text = editor.copy_selection().unwrap();

        let target = pos2(1000.0, 500.0);
        assert!(editor.paste(&text, target));
        assert_eq!(editor.node_count(), 11);
        assert_eq!(
            editor.wires().len(),
            wires + 1,
            "split.r -> am_red.carrier is copied"
        );
        let pasted: Vec<_> = editor
            .selected()
            .iter()
            .map(|&k| editor.node(k).unwrap())
            .collect();
        assert_eq!(pasted.len(), 2);
        assert!(
            pasted.iter().all(|n| n.id != "split" && n.id != "am_red"),
            "fresh ids"
        );
        let corner = pasted
            .iter()
            .map(|n| n.pos)
            .reduce(|a, b| a.min(b))
            .unwrap();
        assert_eq!(corner, target);

        assert!(!editor.paste("not a graph", target));
        assert_eq!(editor.node_count(), 11);
    }

    #[test]
    fn restoring_keeps_the_view_and_selection() {
        let mut editor = GraphEditor::new(&graph());
        let before = editor.to_desc();
        editor.view = View {
            offset: vec2(5.0, 6.0),
            zoom: 1.5,
        };
        editor.fit_pending = false;
        let split = editor.key_of("split").unwrap();
        editor.set_active(Some(split));
        editor.remove_nodes(&BTreeSet::from([editor.key_of("out").unwrap()]));

        editor.restore(&before);
        assert_eq!(editor.to_desc(), before);
        assert_eq!(editor.view().zoom, 1.5);
        assert!(!editor.fit_pending);
        assert_eq!(editor.active().map(|n| n.id.as_str()), Some("split"));
    }

    #[test]
    fn layout_changes_do_not_count_as_edits() {
        let graph = graph();
        let mut moved = graph.clone();
        moved.nodes[0].position = Some([500.0, 500.0]);
        moved.nodes[0].label = Some("Main video".into());
        assert_eq!(without_layout(&moved), without_layout(&graph));
    }

    #[test]
    fn node_order_does_not_count_as_an_edit() {
        let graph = graph();
        let mut reordered = graph.clone();
        reordered.nodes.reverse();
        reordered.connections.reverse();
        assert_eq!(without_layout(&reordered), without_layout(&graph));
    }

    #[test]
    fn raising_a_node_keeps_the_graph_order() {
        let mut editor = GraphEditor::new(&graph());
        let before = editor.to_desc();
        let first = editor.nodes[0].key;
        editor.raise(first);
        assert_eq!(editor.to_desc(), before);
        assert_eq!(
            *editor.draw_order().last().unwrap(),
            0,
            "drawn last, on top"
        );
    }

    #[test]
    fn renaming_a_track_updates_audio_inputs() {
        let mut editor = GraphEditor::new(&graph());
        editor.rename_track("audio", "drums");
        let audio = editor.node(editor.key_of("audio").unwrap()).unwrap();
        assert_eq!(
            audio.params.get("source"),
            Some(&ParamValue::Text("drums".into()))
        );
    }

    #[test]
    fn auto_layout_puts_nodes_in_columns_by_depth() {
        let graph = graph();
        let positions = auto_layout(&graph);
        let x = |id: &str| positions[graph.nodes.iter().position(|n| n.id == id).unwrap()].x;
        assert!(x("video") < x("split") && x("split") < x("am_red") && x("am_red") < x("combine"));
        assert!(x("combine") < x("out"));
    }
}
