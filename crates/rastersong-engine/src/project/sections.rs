//! The Video and Audio sections: one list of tracks, the video tracks first (top first), then
//! the audio tracks. Tracks stay in their section and reorder within it.
//!
//! The mix follows the sections the way editors do: the top video track with an item at a
//! frame wins, and the audio tracks are summed at their volumes. Mute and solo shape only the
//! mix; graphs can read every track as it is (its stem).

use std::ops::Range;

use super::{Project, ProjectTrack, Resource};
use crate::timeline::{Timeline, TrackKind, TrackSpec};

impl Project {
    /// What the engine renders: the timebase and every track that plays something.
    pub fn timeline(&self) -> Timeline {
        self.timeline_with(|_| true)
    }

    /// [`Self::timeline`] without the tracks whose resource `readable` refuses (a file that is
    /// missing): they read as gaps until the file is found. Tracks are listed video first, each
    /// at its level in the mix.
    pub fn timeline_with(&self, readable: impl Fn(&Resource) -> bool) -> Timeline {
        let tracks = (0..self.tracks.len())
            .filter_map(|i| {
                let track = &self.tracks[i];
                let resource = self.track_resource(track).filter(|r| readable(r))?;
                Some(TrackSpec {
                    name: track.name.clone(),
                    kind: track.kind,
                    path: resource.path.clone(),
                    stream: resource.stream,
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
        let videos = self.tracks_of(TrackKind::Video).count();
        match kind {
            TrackKind::Video => 0..videos,
            TrackKind::Audio => videos..self.tracks.len(),
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

    /// Track `i`'s level in the mix: its volume (all or nothing for a video track), or 0 when
    /// it is out of the mix. Graphs read the track as it is, whatever its level.
    pub fn mix_gain(&self, i: usize) -> f32 {
        let track = &self.tracks[i];
        match (self.in_mix(i), track.kind) {
            (false, _) => 0.0,
            (true, TrackKind::Video) => 1.0,
            (true, TrackKind::Audio) => track.volume,
        }
    }

    /// Puts `track` at the bottom of its section and returns its index.
    pub(super) fn insert_new_track(&mut self, track: ProjectTrack) -> usize {
        let at = self.section(track.kind).end;
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

    /// Puts the video tracks before the audio tracks, keeping each section's order: for files.
    pub(super) fn sanitize_sections(&mut self) {
        self.tracks.sort_by_key(|t| t.kind == TrackKind::Audio);
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
        let mut p = project();
        p.tracks.swap(0, 3);
        p.sanitize_sections();
        // b w a v: each kind keeps its order.
        assert_eq!(names(&p), ["w", "v", "b", "a"]);
    }
}
