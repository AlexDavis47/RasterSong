//! Helpers the test binaries share.

use rastersong_engine::{Project, ProjectTrack, TrackKind};

/// The project's tracks of one kind, top first, as the tests look them up.
pub trait TrackLists {
    fn videos(&self) -> Vec<&ProjectTrack>;
    fn audios(&self) -> Vec<&ProjectTrack>;
    /// The `n`th track playing `kind`, to change.
    fn track_of_mut(&mut self, kind: TrackKind, n: usize) -> &mut ProjectTrack;
}

impl TrackLists for Project {
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
