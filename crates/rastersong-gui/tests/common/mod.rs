//! Helpers the test binaries share.

use rastersong_engine::{ItemLengths, Project, ProjectTrack, ResourceId, TrackKind};

/// The project's tracks of one kind, top first, as the tests look them up.
pub trait TrackLists {
    #[allow(dead_code)]
    fn graphs(&self) -> Vec<&ProjectTrack>;
    fn videos(&self) -> Vec<&ProjectTrack>;
    fn audios(&self) -> Vec<&ProjectTrack>;
    /// The `n`th track playing `kind`, to change.
    fn track_of_mut(&mut self, kind: TrackKind, n: usize) -> &mut ProjectTrack;
}

impl TrackLists for Project {
    fn graphs(&self) -> Vec<&ProjectTrack> {
        self.tracks_of(TrackKind::Graph).collect()
    }

    fn videos(&self) -> Vec<&ProjectTrack> {
        self.tracks_of(TrackKind::Video).collect()
    }

    fn audios(&self) -> Vec<&ProjectTrack> {
        self.tracks_of(TrackKind::Audio).collect()
    }

    fn track_of_mut(&mut self, kind: TrackKind, n: usize) -> &mut ProjectTrack {
        let name = self
            .tracks_of(kind)
            .nth(n)
            .expect("no such track")
            .name
            .clone();
        let i = self.track_index(&name).unwrap();
        &mut self.tracks[i]
    }
}

/// Adds an empty graph track named `name` on top of the Graphs section, as the app's New graph
/// track does, and returns its name.
#[allow(dead_code)]
pub fn add_graph_track(project: &mut Project, name: &str) -> String {
    project.add_empty_track_at(TrackKind::Graph, name, Some(0))
}

/// Places graph `graph` on graph track `track` from `at` for `length` seconds, trimming what it
/// lands on, and returns the item's place.
#[allow(dead_code)]
pub fn place_graph(
    project: &mut Project,
    track: &str,
    graph: ResourceId,
    at: f64,
    length: f64,
) -> Option<usize> {
    let lengths = ItemLengths {
        new_graph_length: length,
        ..Default::default()
    };
    project.place_resource(track, graph, at, &lengths)
}
