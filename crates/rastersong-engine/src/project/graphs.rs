//! Graph resources: a project can hold any number of graphs. One is open, in `Project::graph`
//! (what the editor shows and the engine renders until graph layers can place several); the
//! others are kept here, and opening one swaps it with the open graph.

use rastersong_graph::GraphDesc;
use serde::{Deserialize, Serialize};

use super::Project;

/// A new graph: the video wired to Video Output and the sound to Audio Output, so effects are
/// inserted into the wires and a graph that only changes the picture leaves the sound alone.
pub const PASSTHROUGH_GRAPH: &str = r#"{
  "version": 0,
  "nodes": [
    { "id": "video", "type": "video_input" },
    { "id": "audio", "type": "audio_input" },
    { "id": "out", "type": "output" },
    { "id": "sound", "type": "audio_output" }
  ],
  "connections": [
    { "from": "video", "to": "out" },
    { "from": "audio", "to": "sound" }
  ]
}"#;

/// A graph that isn't the open one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredGraph {
    pub id: u32,
    pub name: String,
    pub graph: GraphDesc,
}

/// One line of the Resources panel's graph list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEntry {
    pub id: u32,
    pub name: String,
    /// Whether this is the open graph.
    pub open: bool,
}

pub(super) fn first_graph_id() -> u32 {
    1
}

pub(super) fn default_graph_name() -> String {
    "Graph".to_owned()
}

impl Project {
    /// Every graph, by id, with the open one marked.
    pub fn graph_entries(&self) -> Vec<GraphEntry> {
        let mut entries: Vec<GraphEntry> = std::iter::once(GraphEntry {
            id: self.graph_id,
            name: self.graph_name.clone(),
            open: true,
        })
        .chain(self.graphs.iter().map(|g| GraphEntry {
            id: g.id,
            name: g.name.clone(),
            open: false,
        }))
        .collect();
        entries.sort_by_key(|e| e.id);
        entries
    }

    fn next_graph_id(&self) -> u32 {
        self.graphs
            .iter()
            .map(|g| g.id)
            .chain([self.graph_id])
            .max()
            .map_or(1, |id| id + 1)
    }

    /// The first `name`, `name_2`, `name_3`, … that no graph has.
    pub fn unique_graph_name(&self, name: &str) -> String {
        let name = name.trim();
        let name = if name.is_empty() { "Graph" } else { name };
        (1..)
            .map(|n| {
                if n == 1 {
                    name.to_owned()
                } else {
                    format!("{name}_{n}")
                }
            })
            .find(|candidate| !self.graph_entries().iter().any(|g| g.name == *candidate))
            .unwrap()
    }

    /// Stores a new graph (the passthrough when `graph` is `None`) without opening it, and
    /// returns its id.
    pub fn add_graph(&mut self, name: &str, graph: Option<GraphDesc>) -> u32 {
        let graph = graph
            .unwrap_or_else(|| GraphDesc::from_json(PASSTHROUGH_GRAPH).expect("a valid template"));
        let id = self.next_graph_id();
        let name = self.unique_graph_name(name);
        self.graphs.push(StoredGraph { id, name, graph });
        id
    }

    /// Stores a copy of graph `id` (the open graph as it is now) and returns its id.
    pub fn duplicate_graph(&mut self, id: u32) -> Option<u32> {
        let (name, graph) = if id == self.graph_id {
            (self.graph_name.clone(), self.graph.clone())
        } else {
            let stored = self.graphs.iter().find(|g| g.id == id)?;
            (stored.name.clone(), stored.graph.clone())
        };
        Some(self.add_graph(&format!("{name} copy"), Some(graph)))
    }

    /// Opens graph `id`: it becomes `Project::graph` and the open graph is stored in its place.
    /// False if there is no such stored graph (the open graph is already open).
    pub fn open_graph(&mut self, id: u32) -> bool {
        let Some(i) = self.graphs.iter().position(|g| g.id == id) else {
            return false;
        };
        let stored = self.graphs.remove(i);
        let previous = StoredGraph {
            id: std::mem::replace(&mut self.graph_id, stored.id),
            name: std::mem::replace(&mut self.graph_name, stored.name),
            graph: std::mem::replace(&mut self.graph, stored.graph),
        };
        self.graphs.push(previous);
        true
    }

    /// Renames graph `id`, keeping names unique. False if there is no such graph or the name
    /// is blank.
    pub fn rename_graph(&mut self, id: u32, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || !self.graph_entries().iter().any(|g| g.id == id) {
            return false;
        }
        let current = if id == self.graph_id {
            self.graph_name.clone()
        } else {
            self.graphs
                .iter()
                .find(|g| g.id == id)
                .map(|g| g.name.clone())
                .unwrap_or_default()
        };
        // Its own name doesn't count as taken.
        let name = if current == name {
            current
        } else {
            self.unique_graph_name(name)
        };
        if id == self.graph_id {
            self.graph_name = name;
        } else if let Some(g) = self.graphs.iter_mut().find(|g| g.id == id) {
            g.name = name;
        }
        true
    }

    /// Removes a stored graph. The open graph can't be removed (open another first).
    pub fn remove_graph(&mut self, id: u32) -> bool {
        let before = self.graphs.len();
        self.graphs.retain(|g| g.id != id);
        self.remove_graph_items(id);
        self.graphs.len() != before
    }
}
