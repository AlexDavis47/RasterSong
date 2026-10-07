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
    COMBINE, ChannelMap, Channels, Connection, Diagnostic, FORMAT_VERSION, GeneratorLayout,
    GraphDesc, Grouping, Interpolation, MAX_CHANNELS, Modulation, NodeDesc, NodeStats, NodeType,
    OutputLevel, OutputSpec, ParamValue, Registry, SPLIT, Tag,
};

pub use canvas::CanvasContext;
pub use inspector::InspectorContext;
pub use linked::LinkedRename;
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
    pub grouping: Grouping,
    pub channels: Channels,
    pub layout: GeneratorLayout,
    /// Whether the node is skipped: its input passes straight to its output.
    pub bypass: bool,
    /// Top-left corner in graph space.
    pub pos: Pos2,
    /// How connected signals move parameters, by parameter name.
    pub modulation: BTreeMap<String, Modulation>,
    /// The number parameters rounded to whole numbers.
    pub integer: BTreeSet<String>,
    /// Slider ranges the user set, by parameter name.
    pub ranges: BTreeMap<String, [f64; 2]>,
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
    registry: &'static Registry,
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
    /// Whether Duplicate and Paste keep a node's input connections (the user's setting; the
    /// Shift variants of the shortcuts do the opposite for one action).
    pub keep_connections: bool,
    /// The project's limit on pre-rendered warmup frames, to flag nodes that need more.
    pub max_warmup_frames: u32,
    /// The project's video file name and audio track names, for the nodes linked to them.
    project_video: Option<String>,
    project_tracks: Vec<String>,
    /// Renames the user made on linked nodes, for the app to apply to the project.
    renames: Vec<LinkedRename>,
    /// Problems found when loading a graph (e.g. connections to ports that don't exist).
    pub warnings: Vec<String>,
    /// What the last compile of the graph found out about each node: its signals' tags and
    /// warnings. Nodes that weren't compiled (not feeding the output, bypassed) have none.
    compiled: Vec<NodeStats>,
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
            registry: Registry::shared(),
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
            keep_connections: true,
            max_warmup_frames: rastersong_engine::DEFAULT_MAX_WARMUP_FRAMES,
            project_video: None,
            project_tracks: Vec::new(),
            renames: Vec::new(),
            warnings: Vec::new(),
            compiled: Vec::new(),
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
                grouping: node.grouping,
                channels: node.channels,
                layout: node.layout,
                bypass: node.bypass,
                pos: node.position.map_or(positions[i], |[x, y]| pos2(x, y)),
                modulation: node.modulation.clone(),
                integer: node.integer.iter().cloned().collect(),
                ranges: node.ranges.clone(),
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
            (Some(port), true) => kind.spec.inputs.iter().position(|i| i.name == port)?,
            (Some(port), false) => editor_outputs(kind).iter().position(|o| o.name == port)?,
        };
        let count = if input {
            kind.spec.inputs.len()
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
                    _ if input => k.spec.inputs[index].name.to_owned(),
                    _ => k.spec.outputs[index].name.to_owned(),
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
                    grouping: n.grouping,
                    channels: n.channels,
                    layout: n.layout,
                    bypass: n.bypass,
                    label: n.label.clone(),
                    position: Some([n.pos.x.round(), n.pos.y.round()]),
                    modulation: n.modulation.clone(),
                    integer: n.integer.iter().cloned().collect(),
                    ranges: n.ranges.clone(),
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

    /// Takes what the engine's last compile found out about each node.
    pub fn set_compiled(&mut self, stats: Vec<NodeStats>) {
        self.compiled = stats;
    }

    /// What the last compile found out about `key`, if it was compiled.
    pub fn compiled(&self, key: NodeKey) -> Option<&NodeStats> {
        let id = &self.node(key)?.id;
        self.compiled.iter().find(|s| *s.node == **id)
    }

    /// The compile notes and warnings about `key`.
    pub fn diagnostics(&self, key: NodeKey) -> &[Diagnostic] {
        self.compiled(key).map_or(&[], |s| &s.diagnostics)
    }

    /// How many of a channel node's ports the editor shows, and the compiled signal that names
    /// them: Split shows one output per channel of its input; Combine one input per channel plus
    /// a free one. A connected port is always shown. `None` for other nodes, which show every
    /// port.
    fn channel_ports(&self, key: NodeKey, input: bool) -> Option<(usize, Option<Tag>)> {
        let node = self.node(key)?;
        let compiled = self.compiled(key);
        let connected = |port: usize| {
            self.wires.iter().any(|w| {
                if input {
                    w.to == (key, port)
                } else {
                    w.from == (key, port)
                }
            })
        };
        let last_connected = (0..MAX_CHANNELS).rev().find(|&p| connected(p));
        let (count, tag) = match (node.kind.as_str(), input) {
            (SPLIT, false) => {
                let input = compiled.and_then(|s| s.inputs.first());
                let channels = input.map_or(3, |l| l.samples_per_pixel as usize);
                (channels, input.map(|l| l.tag))
            }
            (COMBINE, true) => {
                let output = compiled.and_then(|s| s.outputs.first());
                let spare = last_connected.map_or(2, |last| last + 2);
                (spare.max(2), output.map(|l| l.tag))
            }
            _ => return None,
        };
        let count = count.max(last_connected.map_or(0, |p| p + 1));
        Some((count.clamp(1, MAX_CHANNELS), tag))
    }
}

/// The editor's label for channel `index` of a signal tagged `tag`: R, G, B for RGB, L, R for
/// stereo, else its number.
pub(crate) fn channel_label(tag: Option<Tag>, index: usize) -> &'static str {
    const NUMBERS: [&str; MAX_CHANNELS] = ["1", "2", "3", "4", "5", "6", "7", "8"];
    match tag.map(|t| t.channels) {
        Some(ChannelMap::Rgb) if index < 3 => ["R", "G", "B"][index],
        Some(ChannelMap::Stereo) if index < 2 => ["L", "R"][index],
        _ => NUMBERS[index.min(MAX_CHANNELS - 1)],
    }
}

impl GraphEditor {
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
            grouping: Grouping::Pixels,
            channels: Channels::Together,
            layout: GeneratorLayout::default(),
            bypass: false,
            pos,
            modulation: BTreeMap::new(),
            integer: BTreeSet::new(),
            ranges: BTreeMap::new(),
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

    /// Removes nodes like [`Self::remove_nodes`], but reconnects around each one: whatever read its
    /// first output reads what fed its main input instead.
    pub fn remove_nodes_and_repair(&mut self, keys: &BTreeSet<NodeKey>) {
        let keys: Vec<NodeKey> = keys
            .iter()
            .copied()
            .filter(|&k| !self.is_linked(k))
            .collect();
        for key in keys {
            let feed = self.wires.iter().find(|w| w.to == (key, 0)).map(|w| w.from);
            if let Some(from) = feed {
                let readers: Vec<(NodeKey, usize)> = self
                    .wires
                    .iter()
                    .filter(|w| w.from == (key, 0))
                    .map(|w| w.to)
                    .collect();
                for to in readers {
                    self.connect(from, to);
                }
            }
            self.remove_nodes_unchecked(&BTreeSet::from([key]));
        }
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
    /// the copies. With `keep_inputs`, the copies are fed by the same sources as the originals.
    pub fn duplicate(&mut self, keys: &BTreeSet<NodeKey>, keep_inputs: bool) {
        let fragment = self.fragment(keys);
        self.insert(&fragment, DUPLICATE_OFFSET, keep_inputs);
    }

    /// The given nodes and the connections between them, as a graph of their own: what Copy
    /// puts on the clipboard. Also holds the connections into them from nodes outside the
    /// selection, which [`Self::insert`] reconnects only when asked to; they name nodes the
    /// fragment doesn't contain.
    pub fn fragment(&self, keys: &BTreeSet<NodeKey>) -> GraphDesc {
        let mut graph = self.to_desc();
        let ids: BTreeSet<String> = keys
            .iter()
            .filter(|&&k| !self.is_linked(k))
            .filter_map(|&k| self.node(k).map(|n| n.id.clone()))
            .collect();
        let node_of = |endpoint: &str| endpoint.split('.').next().unwrap_or("").to_owned();
        graph.nodes.retain(|n| ids.contains(&n.id));
        graph.connections.retain(|c| ids.contains(&node_of(&c.to)));
        graph
    }

    /// Adds the nodes of `fragment` (with fresh ids) and its connections, moved by `offset`, and
    /// selects them. Nodes of unknown types and connections that don't resolve are skipped.
    /// Connections into the fragment from nodes outside it are made only if `keep_inputs` is set
    /// and the source still exists here. Returns the keys of the new nodes.
    pub fn insert(
        &mut self,
        fragment: &GraphDesc,
        offset: Vec2,
        keep_inputs: bool,
    ) -> Vec<NodeKey> {
        let existing: Vec<(String, NodeKey)> =
            self.nodes.iter().map(|n| (n.id.clone(), n.key)).collect();
        let existing: HashMap<&str, NodeKey> = existing
            .iter()
            .map(|(id, key)| (id.as_str(), *key))
            .collect();
        let positions = auto_layout(fragment);
        let mut keys = HashMap::new();
        for (i, desc) in fragment.nodes.iter().enumerate() {
            // The project's inputs and output come from the project, not the clipboard.
            let addable = self
                .registry
                .get(&desc.kind)
                .is_some_and(Self::user_addable);
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
            node.grouping = desc.grouping;
            node.channels = desc.channels;
            node.layout = desc.layout;
            node.bypass = desc.bypass;
            node.label = desc.label.clone();
            node.modulation = desc.modulation.clone();
            node.integer = desc.integer.iter().cloned().collect();
            node.ranges = desc.ranges.clone();
            node.exposed = desc.exposed.as_ref().map(|e| e.iter().cloned().collect());
            keys.insert(desc.id.as_str(), key);
        }
        for c in &fragment.connections {
            let source = c.from.split('.').next().unwrap_or("");
            let sources = if keys.contains_key(source) {
                &keys
            } else if keep_inputs {
                &existing
            } else {
                continue;
            };
            if let (Some(from), Some(to)) = (
                self.endpoint(sources, &c.from, false),
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
    /// (graph space). Text that isn't a graph is ignored. With `keep_inputs`, pasted nodes are
    /// fed by the nodes they were copied from, where those still exist.
    pub fn paste(&mut self, text: &str, at: Pos2, keep_inputs: bool) -> bool {
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
        !self.insert(&fragment, at - corner, keep_inputs).is_empty()
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

    /// Removes the selected nodes and reconnects around them.
    pub fn delete_selection_and_repair(&mut self) {
        let selected = self.selected.clone();
        self.remove_nodes_and_repair(&selected);
    }

    /// Bypasses the selected nodes, or restores them if they are all bypassed already. The
    /// project's output has nothing to pass through and is left alone.
    pub fn toggle_bypass_selection(&mut self) {
        let selected = self.selected.clone();
        self.toggle_bypass(&selected);
    }

    /// Bypasses `nodes`, or restores them if they are all bypassed already.
    pub(crate) fn toggle_bypass(&mut self, nodes: &BTreeSet<NodeKey>) {
        let keys: Vec<NodeKey> = nodes
            .iter()
            .copied()
            .filter(|&k| self.node(k).is_some_and(|n| n.kind != linked::OUTPUT))
            .collect();
        let all = keys.iter().all(|&k| self.node(k).is_some_and(|n| n.bypass));
        for k in keys {
            self.node_mut(k).unwrap().bypass = !all;
        }
    }

    /// Duplicates the selected nodes, keeping their input connections as the setting says;
    /// `invert` does the opposite for this one duplicate.
    pub fn duplicate_selection(&mut self, invert: bool) {
        let selected = self.selected.clone();
        self.duplicate(&selected, self.keep_connections != invert);
    }

    /// Points audio inputs that read track `old` at `new` (after a track is renamed).
    pub fn rename_track(&mut self, old: &str, new: &str) {
        for node in self
            .nodes
            .iter_mut()
            .filter(|n| n.kind == linked::AUDIO_INPUT)
        {
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
pub(crate) fn editor_outputs(kind: &NodeType) -> &'static [OutputSpec] {
    // Nothing reads an output node.
    if kind.spec.category == rastersong_engine::Category::Output {
        &[]
    } else {
        kind.spec.outputs
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

    #[test]
    fn channel_nodes_show_a_port_per_channel() {
        use rastersong_engine::Layout;
        // am_bands splits the video three ways and combines three channels.
        let mut editor = GraphEditor::new(&graph());
        let split = editor.key_of("split").unwrap();
        let combine = editor.key_of("combine").unwrap();
        // Before a compile: three outputs, and the connected inputs plus a free one.
        assert_eq!(editor.channel_ports(split, false), Some((3, None)));
        assert_eq!(editor.channel_ports(combine, true).unwrap().0, 4);
        assert_eq!(editor.channel_ports(split, true), None);
        // Compiled with stereo coming in, Split shows two, named L and R.
        let stereo = Layout::audio_channels(10, 2);
        editor.set_compiled(vec![rastersong_engine::NodeStats {
            node: "split".into(),
            latency_frames: 0.0,
            warmup_frames: 0,
            inputs: vec![stereo],
            outputs: Vec::new(),
            diagnostics: vec![Diagnostic::note("a note")],
        }]);
        // A connected port stays shown even past the channel count.
        let (count, tag) = editor.channel_ports(split, false).unwrap();
        assert_eq!(count, 3);
        assert_eq!(channel_label(tag, 0), "L");
        assert_eq!(channel_label(tag, 2), "3");
        assert_eq!(editor.diagnostics(split), [Diagnostic::note("a note")]);
        assert!(editor.diagnostics(combine).is_empty());
    }

    /// Connections with every port written out, sorted, for comparing graphs.
    fn canonical(g: &GraphDesc) -> Vec<(String, String)> {
        let registry = Registry::shared();
        let full = |e: &str, input: bool| {
            if e.contains('.') {
                return e.to_owned();
            }
            let kind = &g.nodes.iter().find(|n| n.id == e).unwrap().kind;
            let kind = registry.get(kind).unwrap();
            format!(
                "{e}.{}",
                if input {
                    kind.spec.inputs[0].name
                } else {
                    kind.spec.outputs[0].name
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
    fn slider_ranges_survive_the_editor() {
        let json = r#"{ "version": 0, "nodes": [ { "id": "d", "type": "delay", "ranges": { "time": [0, 3] } } ] }"#;
        let editor = GraphEditor::new(&GraphDesc::from_json(json).unwrap());
        let key = editor.key_of("d").unwrap();
        assert_eq!(editor.node(key).unwrap().ranges["time"], [0.0, 3.0]);
        assert_eq!(editor.to_desc().nodes[0].ranges["time"], [0.0, 3.0]);
    }

    #[test]
    fn integer_parameters_survive_the_editor() {
        let graph = GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [ { "id": "d", "type": "delay", "integer": ["time"] } ] }"#,
        )
        .unwrap();
        let editor = GraphEditor::new(&graph);
        let key = editor.key_of("d").unwrap();
        assert!(editor.node(key).unwrap().integer.contains("time"));
        assert_eq!(editor.to_desc().nodes[0].integer, ["time"]);
    }

    #[test]
    fn drops_connections_to_unknown_ports() {
        let graph = GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [ { "id": "v", "type": "video_input" }, { "id": "o", "type": "output" } ],
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
            GraphEditor::new(&GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        let a = editor.add_node("delay", Pos2::ZERO).unwrap();
        let b = editor.add_node("delay", Pos2::ZERO).unwrap();
        assert_eq!(editor.node(a).unwrap().id, "delay");
        assert_eq!(editor.node(b).unwrap().id, "delay_2");
        assert_eq!(editor.add_node("nope", Pos2::ZERO), None);
    }

    #[test]
    fn deleting_with_repair_reconnects_around_the_node() {
        let mut editor = GraphEditor::new(&graph());
        let split = editor.key_of("split").unwrap();
        let video = editor.key_of("video").unwrap();
        let readers: Vec<_> = editor
            .wires()
            .iter()
            .filter(|w| w.from == (split, 0))
            .map(|w| w.to)
            .collect();
        assert!(!readers.is_empty());
        editor.remove_nodes_and_repair(&BTreeSet::from([split]));
        assert!(editor.node(split).is_none());
        for to in readers {
            assert!(
                editor
                    .wires()
                    .iter()
                    .any(|w| w.from == (video, 0) && w.to == to)
            );
        }
    }

    #[test]
    fn bypass_toggles_and_is_saved() {
        let mut editor = GraphEditor::new(&graph());
        let split = editor.key_of("split").unwrap();
        editor.selected.insert(split);
        editor.toggle_bypass_selection();
        let desc = editor.to_desc();
        assert!(desc.nodes.iter().any(|n| n.id == "split" && n.bypass));
        assert!(
            GraphEditor::new(&desc)
                .node(split)
                .is_some_and(|n| n.bypass)
        );
        editor.toggle_bypass_selection();
        assert!(!editor.node(split).unwrap().bypass);
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
        editor.duplicate(&BTreeSet::from([split, red]), false);
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
        assert!(editor.paste(&text, target, false));
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

        assert!(!editor.paste("not a graph", target, true));
        assert_eq!(editor.node_count(), 11);
    }

    #[test]
    fn duplicate_and_paste_can_keep_input_connections() {
        let mut editor = GraphEditor::new(&graph());
        let red = editor.key_of("am_red").unwrap();
        let inputs =
            |editor: &GraphEditor, key| editor.wires().iter().filter(|w| w.to.0 == key).count();
        let original = inputs(&editor, red);
        assert!(original > 0, "am_red has inputs to keep");

        let before: BTreeSet<NodeKey> = editor.nodes.iter().map(|n| n.key).collect();
        editor.duplicate(&BTreeSet::from([red]), false);
        let copy = *editor.selected().iter().next().unwrap();
        assert!(!before.contains(&copy));
        assert_eq!(inputs(&editor, copy), 0, "disconnected copy");

        editor.selected = BTreeSet::from([red]);
        editor.duplicate_selection(false);
        let copy = *editor.selected().iter().next().unwrap();
        assert_eq!(inputs(&editor, copy), original, "setting on keeps inputs");
        editor.selected = BTreeSet::from([red]);
        editor.duplicate_selection(true);
        let copy = *editor.selected().iter().next().unwrap();
        assert_eq!(inputs(&editor, copy), 0, "Shift inverts");

        editor.selected = BTreeSet::from([red]);
        let text = editor.copy_selection().unwrap();
        assert!(editor.paste(&text, pos2(0.0, 0.0), true));
        let pasted = *editor.selected().iter().next().unwrap();
        assert_eq!(inputs(&editor, pasted), original);

        // Pasting where the sources don't exist skips the connections without failing.
        let mut other =
            GraphEditor::new(&GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        assert!(other.paste(&text, pos2(0.0, 0.0), true));
        assert_eq!(other.wires().len(), 0);
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
