//! The Graphs, Video and Audio sections: one list of tracks, a section of each kind in that
//! order, each top first. Tracks stay in their section and reorder within it.
//!
//! The mix follows the sections the way editors do: the top video track with an item at a
//! frame wins, and the audio tracks are summed at their volumes. Mute and solo shape only the
//! mix; graphs can read every video and audio track as it is (its stem). Over the mix, the
//! topmost graph track with an item playing wins; a muted graph track is skipped, and a solo
//! leaves out the other graph tracks.

use std::ops::Range;

use super::{Project, ProjectTrack, Resource};
use crate::composition::{ComposedGraph, ComposedGraphTrack, Composition, MediaTrack};
use crate::timeline::{Timeline, TrackKind, TrackSpec};

impl Project {
    /// What the engine renders: the timebase and every track that plays something.
    pub fn timeline(&self) -> Timeline {
        self.timeline_with(|_| true)
    }

    /// [`Self::timeline`] without the tracks whose resource `readable` refuses (a file that is
    /// missing): they read as gaps until the file is found. Video and audio tracks are listed
    /// video first, each at its level in the mix.
    pub fn timeline_with(&self, readable: impl Fn(&Resource) -> bool) -> Timeline {
        let tracks = (0..self.tracks.len())
            .filter_map(|i| {
                let track = &self.tracks[i];
                if !track.kind.reads_file() {
                    return None;
                }
                let resource = self.track_resource(track).filter(|r| readable(r))?;
                let file = resource.file()?;
                Some(TrackSpec {
                    name: track.name.clone(),
                    kind: track.kind,
                    path: file.path.clone(),
                    stream: file.stream,
                    items: track.items.clone(),
                    bus: track.bus.clone(),
                    gain: self.mix_gain(i),
                })
            })
            .collect();
        Timeline {
            timebase: self.timebase,
            tracks,
            buses: self.buses.clone(),
        }
    }

    /// The tracks of `kind`, top first.
    pub fn tracks_of(&self, kind: TrackKind) -> impl Iterator<Item = &ProjectTrack> {
        self.tracks.iter().filter(move |t| t.kind == kind)
    }

    /// Where the tracks of `kind` are in the list.
    pub fn section(&self, kind: TrackKind) -> Range<usize> {
        let start = self.tracks.iter().filter(|t| t.kind < kind).count();
        start..start + self.tracks_of(kind).count()
    }

    /// What the engine composes: the video and audio tracks (solos leave out the other tracks
    /// of their kind from the mix), the graph tracks in the mix with the items that play, and
    /// every graph.
    pub fn composition(&self) -> Composition {
        let mut tracks = Vec::new();
        let mut graph_tracks = Vec::new();
        for (i, t) in self.tracks.iter().enumerate() {
            if t.kind == TrackKind::Graph {
                if self.in_mix(i) {
                    graph_tracks.push(ComposedGraphTrack {
                        name: t.name.clone(),
                        items: t.items.iter().filter(|i| !i.muted).cloned().collect(),
                    });
                }
                continue;
            }
            tracks.push(MediaTrack {
                name: t.name.clone(),
                kind: t.kind,
                volume: t.volume,
                in_mix: self.in_mix(i),
                bus: t.bus.clone(),
            });
        }
        Composition {
            tracks,
            graph_tracks,
            graphs: self
                .resources
                .iter()
                .filter_map(|r| {
                    Some(ComposedGraph {
                        id: r.id,
                        name: r.name.clone(),
                        desc: r.graph()?.clone(),
                    })
                })
                .collect(),
        }
    }

    /// Where the track called `name` is in the list.
    pub fn track_index(&self, name: &str) -> Option<usize> {
        self.tracks.iter().position(|t| t.name == name)
    }

    /// Whether any track of `kind` is soloed, leaving the others of its kind out of the mix.
    pub fn soloing(&self, kind: TrackKind) -> bool {
        self.tracks_of(kind).any(|t| t.solo)
    }

    /// Whether track `i` is in the mix: not muted, and not left out by a solo on another track
    /// of its kind.
    pub fn in_mix(&self, i: usize) -> bool {
        let track = &self.tracks[i];
        !track.muted && (track.solo || !self.soloing(track.kind))
    }

    /// Track `i`'s level in the mix: its volume where its kind [has one](TrackKind::has_level)
    /// (all or nothing otherwise), or 0 when it is out of the mix. Graphs read the track as it
    /// is, whatever its level.
    pub fn mix_gain(&self, i: usize) -> f32 {
        let track = &self.tracks[i];
        match (self.in_mix(i), track.kind.has_level()) {
            (false, _) => 0.0,
            (true, false) => 1.0,
            (true, true) => track.volume,
        }
    }

    /// Puts `track` at the bottom of its section and returns its index.
    pub(super) fn insert_new_track(&mut self, track: ProjectTrack) -> usize {
        self.insert_new_track_at(track, None)
    }

    /// Puts `track` before the track now at index `slot` when that is in its section (or just
    /// past its last), else at the bottom of its section. Returns its index.
    pub(super) fn insert_new_track_at(
        &mut self,
        track: ProjectTrack,
        slot: Option<usize>,
    ) -> usize {
        let section = self.section(track.kind);
        let at = slot
            .filter(|s| (section.start..=section.end).contains(s))
            .unwrap_or(section.end);
        self.tracks.insert(at, track);
        at
    }

    /// Moves track `from` so it lands before the track now at `slot` (`tracks.len()` for the
    /// bottom), kept within its section. Returns where it ends up, or `None` when nothing moved.
    pub fn move_track(&mut self, from: usize, slot: usize) -> Option<usize> {
        let kind = self.tracks.get(from)?.kind;
        let section = self.section(kind);
        let slot = slot.clamp(section.start, section.end);
        let to = if slot > from { slot - 1 } else { slot };
        if to == from {
            return None;
        }
        let track = self.tracks.remove(from);
        self.tracks.insert(to, track);
        Some(to)
    }

    /// Removes track `i` and returns it.
    pub fn remove_track(&mut self, i: usize) -> Option<ProjectTrack> {
        if i >= self.tracks.len() {
            return None;
        }
        let removed = self.tracks.remove(i);
        self.tidy_groups();
        Some(removed)
    }

    /// Puts the sections in order, keeping each one's: for files.
    pub(super) fn sanitize_sections(&mut self) {
        self.tracks.sort_by_key(|t| t.kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GraphDesc;

    fn project() -> Project {
        let mut p = Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        p.add_track(TrackKind::Video, "v", "v.mp4");
        p.add_track(TrackKind::Audio, "a", "a.wav");
        p.add_track(TrackKind::Video, "w", "w.mp4");
        p.add_track(TrackKind::Audio, "b", "b.wav");
        p
    }

    /// [`project`] with graph tracks `g` and `h` on top.
    fn with_graphs() -> Project {
        let mut p = project();
        p.add_empty_track(TrackKind::Graph, "g");
        p.add_empty_track(TrackKind::Graph, "h");
        p
    }

    fn graph_lengths() -> crate::ItemLengths {
        crate::ItemLengths {
            new_graph_length: 2.0,
            ..Default::default()
        }
    }

    fn names(p: &Project) -> Vec<&str> {
        p.tracks.iter().map(|t| t.name.as_str()).collect()
    }

    #[test]
    fn new_tracks_go_to_the_bottom_of_their_section() {
        let p = project();
        assert_eq!(names(&p), ["v", "w", "a", "b"]);
        assert_eq!(p.section(TrackKind::Video), 0..2);
        assert_eq!(p.section(TrackKind::Audio), 2..4);
    }

    #[test]
    fn tracks_move_only_within_their_section() {
        let mut p = project();
        // An audio track dragged to the top stops at the top of the audio section.
        assert_eq!(p.move_track(3, 0), Some(2));
        assert_eq!(names(&p), ["v", "w", "b", "a"]);
        // A video track dragged to the bottom stops at the bottom of the video section.
        assert_eq!(p.move_track(0, 4), Some(1));
        assert_eq!(names(&p), ["w", "v", "b", "a"]);
        assert_eq!(p.move_track(1, 1), None);
        assert!(p.move_track(9, 0).is_none());
    }

    #[test]
    fn mute_and_solo_shape_the_mix_per_kind() {
        let mut p = project();
        let gains = |p: &Project| {
            (0..p.tracks.len())
                .map(|i| p.mix_gain(i))
                .collect::<Vec<_>>()
        };
        p.tracks[3].volume = 0.5;
        assert_eq!(gains(&p), [1.0, 1.0, 1.0, 0.5]);
        // A video track's own volume doesn't matter; an audio solo leaves the video alone.
        p.tracks[0].volume = 0.25;
        p.tracks[2].solo = true;
        assert_eq!(gains(&p), [1.0, 1.0, 1.0, 0.0]);
        // A muted soloed track is still muted.
        p.tracks[2].muted = true;
        assert_eq!(gains(&p), [1.0, 1.0, 0.0, 0.0]);
        assert!(!p.in_mix(3));
    }

    #[test]
    fn a_file_with_its_sections_mixed_up_is_put_in_order() {
        let mut p = with_graphs();
        p.tracks.swap(0, 5);
        p.sanitize_sections();
        // b h v w a g: each kind keeps its order.
        assert_eq!(names(&p), ["h", "g", "v", "w", "b", "a"]);
    }

    #[test]
    fn graph_tracks_are_a_section_on_top() {
        let mut p = with_graphs();
        assert_eq!(names(&p), ["g", "h", "v", "w", "a", "b"]);
        assert_eq!(p.section(TrackKind::Graph), 0..2);
        assert_eq!(p.section(TrackKind::Video), 2..4);
        // A graph track stays in its section too.
        assert_eq!(p.move_track(0, 6), Some(1));
        assert_eq!(names(&p), ["h", "g", "v", "w", "a", "b"]);
        assert_eq!(
            p.add_empty_track_at(TrackKind::Graph, "top", Some(0)),
            "top"
        );
        // A slot outside the section puts it at the bottom of its own.
        assert_eq!(
            p.add_empty_track_at(TrackKind::Graph, "low", Some(5)),
            "low"
        );
        assert_eq!(names(&p)[..4], ["top", "h", "g", "low"]);
    }

    #[test]
    fn graph_tracks_solo() {
        let mut p = with_graphs();
        for track in ["g", "h"] {
            p.place_resource(track, Project::FIRST_GRAPH, 0.0, &graph_lengths())
                .unwrap();
        }
        let playing = |p: &Project| -> Vec<String> {
            p.composition()
                .graph_tracks
                .into_iter()
                .map(|t| t.name)
                .collect()
        };
        assert_eq!(playing(&p), ["g", "h"]);
        p.tracks[1].solo = true;
        assert_eq!(playing(&p), ["h"]);
        // A graph solo leaves the video and audio tracks alone.
        assert!((2..6).all(|i| p.in_mix(i)));
        p.tracks[1].muted = true;
        assert!(playing(&p).is_empty());
        assert!(p.plays_track_mix());
    }

    #[test]
    fn the_composition_leaves_out_muted_graph_tracks_and_items() {
        let mut p = with_graphs();
        let graph = Project::FIRST_GRAPH;
        p.place_resource("h", graph, 0.0, &graph_lengths());
        p.place_resource("h", graph, 3.0, &graph_lengths());
        p.tracks[1].items[1].muted = true;
        p.place_resource("g", graph, 0.0, &graph_lengths());
        let lens = |p: &Project| -> Vec<usize> {
            p.composition()
                .graph_tracks
                .iter()
                .map(|t| t.items.len())
                .collect()
        };
        assert_eq!(lens(&p), [1, 1]);
        p.tracks[1].muted = true;
        assert_eq!(lens(&p), [1]);
        p.tracks[0].muted = true;
        assert!(lens(&p).is_empty());
        assert!(p.plays_track_mix());
        // The video and audio tracks are all there, the graph tracks never.
        assert_eq!(p.composition().tracks.len(), 4);
        assert_eq!(p.timeline().tracks.len(), 4);
    }
}
