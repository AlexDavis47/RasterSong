//! Graph resources: a project holds any number of graphs, as resources beside its media.
//! Items on graph tracks place them. Which graph the editor shows is the app's business,
//! not the project's.

use rastersong_graph::GraphDesc;

use super::Project;
use super::resources::{ResourceContent, ResourceId};
use crate::timeline::TrackKind;

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

/// What a graph is called when it is given no name.
pub const DEFAULT_GRAPH_NAME: &str = "Graph";

impl Project {
    /// Adds a graph resource (the passthrough when `graph` is `None`), named `name` or
    /// [`DEFAULT_GRAPH_NAME`] when that is blank, and returns its id.
    pub fn add_graph(&mut self, name: &str, graph: Option<GraphDesc>) -> ResourceId {
        let graph = graph
            .unwrap_or_else(|| GraphDesc::from_json(PASSTHROUGH_GRAPH).expect("a valid template"));
        let name = if name.trim().is_empty() {
            DEFAULT_GRAPH_NAME
        } else {
            name
        };
        self.push_resource(name, ResourceContent::Graph { graph })
    }

    /// The graph of graph resource `id`.
    pub fn graph(&self, id: ResourceId) -> Option<&GraphDesc> {
        self.resource(id)?.graph()
    }

    pub fn graph_mut(&mut self, id: ResourceId) -> Option<&mut GraphDesc> {
        self.resource_mut(id)?.graph_mut()
    }

    /// Every graph resource's id, in the Resources panel's order.
    pub fn graph_ids(&self) -> impl Iterator<Item = ResourceId> + '_ {
        self.resources
            .iter()
            .filter(|r| r.kind() == TrackKind::Graph)
            .map(|r| r.id)
    }

    /// Every graph, in the Resources panel's order.
    pub(super) fn graphs_mut(&mut self) -> impl Iterator<Item = &mut GraphDesc> {
        self.resources.iter_mut().filter_map(|r| r.graph_mut())
    }
}
