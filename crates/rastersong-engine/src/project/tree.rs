//! The track tree: one list of tracks, top first, in which folders hold the deeper tracks below
//! them.
//!
//! The tree is stored flat, as the timeline shows it: each track has a depth, and a folder holds
//! the run of tracks after it that are deeper than it. A list is a valid tree when the first track
//! is at the top (depth 0) and each track is at most one level deeper than the track above it,
//! and only when that track is a folder. Moving a track moves its whole subtree.
//!
//! The track mix follows the tree: a folder mixes what is in it, and a track left out of the mix
//! (muted, its master send off, or left out by a solo) leaves out everything in it. Until the
//! renderer mixes folders itself (roadmap stage 2b), the levels are multiplied down the tree into
//! each track's own level.

use std::ops::RangeInclusive;

use rastersong_graph::GraphDesc;
use rastersong_lang::tr;

use super::{Project, ProjectTrack, Resource};
use crate::timeline::{Timeline, TrackKind, TrackSpec};

/// The depths track `from` (with its subtree) can take when dropped before the track now at
/// `slot` in a tree given as each track's `(depth, folder)`, top first; `None` when that is
/// inside itself. The range keeps the tree valid: no deeper than inside the track it lands
/// under, and no shallower than leaves the track below it in nothing.
pub fn drop_depths(tree: &[(u32, bool)], from: usize, slot: usize) -> Option<RangeInclusive<u32>> {
    if from >= tree.len() || slot > tree.len() {
        return None;
    }
    let depth = tree[from].0;
    let end = (from + 1..tree.len())
        .find(|&j| tree[j].0 <= depth)
        .unwrap_or(tree.len());
    if (from + 1..end).contains(&slot) {
        return None;
    }
    let (last_depth, last_folder) = tree[end - 1];
    // How far below its own depth the block reaches: the track under it may be that deep.
    let reach = last_depth - depth + u32::from(last_folder);
    // The tree with the block lifted out, and where it goes back in.
    let rest: Vec<(u32, bool)> = tree[..from].iter().chain(&tree[end..]).copied().collect();
    let at = if slot <= from {
        slot
    } else {
        slot - (end - from)
    };
    let max = at
        .checked_sub(1)
        .map_or(0, |a| rest[a].0 + u32::from(rest[a].1));
    let below = rest.get(at).map_or(0, |t| t.0);
    let min = below.saturating_sub(reach).min(max);
    Some(min..=max)
}

impl Project {
    /// What the engine renders: the timebase and every track that plays something.
    pub fn timeline(&self) -> Timeline {
        self.timeline_with(|_| true)
    }

    /// [`Self::timeline`] without the tracks whose resource `readable` refuses (a file that is
    /// missing): they read as gaps until the file is found. Tracks are listed top first, each
    /// at its level in the track mix through the folders it is in, on its top-level folder's
    /// bus.
    pub fn timeline_with(&self, readable: impl Fn(&Resource) -> bool) -> Timeline {
        let soloing_video = self.soloing(TrackKind::Video);
        let soloing_audio = self.soloing(TrackKind::Audio);
        let tracks = (0..self.tracks.len())
            .filter_map(|i| {
                let track = &self.tracks[i];
                let resource = self.track_resource(track).filter(|r| readable(r))?;
                let kind = resource.kind.track_kind();
                let soloing = match kind {
                    TrackKind::Video => soloing_video,
                    TrackKind::Audio => soloing_audio,
                };
                Some(TrackSpec {
                    name: track.name.clone(),
                    kind,
                    path: resource.path.clone(),
                    stream: resource.stream,
                    items: track.items.clone(),
                    bus: self.tracks[self.top_of(i)].bus.clone(),
                    gain: self.mix_gain(i, soloing),
                })
            })
            .collect();
        Timeline {
            timebase: self.timebase,
            tracks,
            buses: self.buses.clone(),
        }
    }

    /// What track `track` plays: video or audio, by its resource; `None` for a folder or an
    /// empty track.
    pub fn track_kind(&self, track: &ProjectTrack) -> Option<TrackKind> {
        self.track_resource(track).map(|r| r.kind.track_kind())
    }

    /// The tracks that play `kind`, top first.
    pub fn tracks_of(&self, kind: TrackKind) -> impl Iterator<Item = &ProjectTrack> {
        self.tracks
            .iter()
            .filter(move |t| self.track_kind(t) == Some(kind))
    }

    /// Where the track called `name` is in the list.
    pub fn track_index(&self, name: &str) -> Option<usize> {
        self.tracks.iter().position(|t| t.name == name)
    }

    /// The folder track `i` is in, if any.
    pub fn parent_of(&self, i: usize) -> Option<usize> {
        let depth = self.tracks.get(i)?.depth;
        (0..i).rev().find(|&j| self.tracks[j].depth < depth)
    }

    /// The top-level track that track `i` is in, or `i` itself at the top.
    fn top_of(&self, i: usize) -> usize {
        (0..=i)
            .rev()
            .find(|&j| self.tracks[j].depth == 0)
            .unwrap_or(i)
    }

    /// One past the last track of track `i`'s subtree: `i + 1` for anything but a folder with
    /// tracks in it.
    pub fn subtree_end(&self, i: usize) -> usize {
        let depth = self.tracks[i].depth;
        (i + 1..self.tracks.len())
            .find(|&j| self.tracks[j].depth <= depth)
            .unwrap_or(self.tracks.len())
    }

    /// The tracks the timeline shows, by index, top first: every track but those in a collapsed
    /// folder.
    pub fn shown_tracks(&self) -> Vec<usize> {
        let mut shown = Vec::new();
        let mut i = 0;
        while i < self.tracks.len() {
            shown.push(i);
            i = if self.tracks[i].collapsed {
                self.subtree_end(i)
            } else {
                i + 1
            };
        }
        shown
    }

    /// Track `i` and the folders it is in, innermost first.
    fn path_up(&self, i: usize) -> impl Iterator<Item = &ProjectTrack> {
        std::iter::successors(Some(i), move |&j| self.parent_of(j)).map(|j| &self.tracks[j])
    }

    /// Whether track `i` counts as soloed: it, or a folder it is in, is.
    pub(super) fn soloed(&self, i: usize) -> bool {
        self.path_up(i).any(|t| t.solo)
    }

    /// Whether any track of `kind` counts as soloed, leaving the others of its kind out of the
    /// track mix.
    pub fn soloing(&self, kind: TrackKind) -> bool {
        (0..self.tracks.len())
            .any(|i| self.track_kind(&self.tracks[i]) == Some(kind) && self.soloed(i))
    }

    /// Track `i`'s level in the track mix when `soloing` says whether a track of its kind is
    /// soloed: its volume times its folders' (a video track's own volume is ignored), or 0 when
    /// it or a folder it is in is muted or sends nowhere, or a solo leaves it out. Graphs read
    /// the track as it is, whatever its level.
    pub fn mix_gain(&self, i: usize, soloing: bool) -> f32 {
        let track = &self.tracks[i];
        if soloing && !self.soloed(i) {
            return 0.0;
        }
        let video = self.track_kind(track) == Some(TrackKind::Video);
        self.path_up(i)
            .enumerate()
            .map(|(n, t)| {
                if t.muted || !t.master_send {
                    0.0
                } else if n == 0 && video {
                    1.0
                } else {
                    t.volume
                }
            })
            .product()
    }

    /// A new project: `graph` open, and the tracks laid out as *Video*, *Audio* and *Control*
    /// folders. New video and audio tracks go in the first two; Control sends nowhere, for what
    /// drives graphs without being seen or heard.
    pub fn with_template(graph: GraphDesc) -> Self {
        let mut project = Self::new(graph);
        for (key, kind) in [
            ("project.folder.video", Some(TrackKind::Video)),
            ("project.folder.audio", Some(TrackKind::Audio)),
            ("project.folder.control", None),
        ] {
            let mut folder = ProjectTrack::new_folder(tr(key).to_owned());
            folder.new_tracks = kind;
            folder.master_send = kind.is_some();
            project.tracks.push(folder);
        }
        project
    }

    /// Where a new track of `kind` goes: `(index, depth)`, at the bottom of the first folder
    /// that takes new tracks of its kind, else at the bottom of the list.
    pub fn new_track_slot(&self, kind: Option<TrackKind>) -> (usize, u32) {
        let home = kind.and_then(|k| {
            self.tracks
                .iter()
                .position(|t| t.folder && t.new_tracks == Some(k))
        });
        match home {
            Some(f) => (self.subtree_end(f), self.tracks[f].depth + 1),
            None => (self.tracks.len(), 0),
        }
    }

    /// Puts `track` where a new track of `kind` goes and returns its index.
    pub(super) fn insert_new_track(
        &mut self,
        kind: Option<TrackKind>,
        mut track: ProjectTrack,
    ) -> usize {
        let (at, depth) = self.new_track_slot(kind);
        track.depth = depth;
        self.tracks.insert(at, track);
        at
    }

    /// Adds an empty folder at the bottom and returns its name, `Folder`, `Folder_2`, …
    pub fn add_folder(&mut self, name: &str) -> String {
        let name = self.unique_name(name);
        self.tracks.push(ProjectTrack::new_folder(name.clone()));
        name
    }

    /// Moves track `from`, with everything in it, so it lands before the track now at `slot`
    /// (`tracks.len()` for the bottom). It goes `depth` deep when given, else as deep as it can
    /// (inside a folder it lands under), within [`drop_depths`]. Returns where it ends up, or
    /// `None` when nothing moved (a track can't go into itself).
    pub fn move_track(&mut self, from: usize, slot: usize, depth: Option<u32>) -> Option<usize> {
        if from >= self.tracks.len() || slot > self.tracks.len() {
            return None;
        }
        let shape: Vec<(u32, bool)> = self.tracks.iter().map(|t| (t.depth, t.folder)).collect();
        let range = drop_depths(&shape, from, slot)?;
        let end = self.subtree_end(from);
        let block: Vec<ProjectTrack> = self.tracks.drain(from..end).collect();
        let at = if slot <= from {
            slot
        } else {
            slot - block.len()
        };
        let root = block[0].depth;
        let new_root = depth
            .unwrap_or(*range.end())
            .clamp(*range.start(), *range.end());
        let moved = block.into_iter().map(|mut t| {
            t.depth = t.depth - root + new_root;
            t
        });
        self.tracks.splice(at..at, moved);
        (at != from || new_root != root).then_some(at)
    }

    /// Removes track `i` and returns it. What was in a folder moves up a level, in its place.
    pub fn remove_track(&mut self, i: usize) -> Option<ProjectTrack> {
        if i >= self.tracks.len() {
            return None;
        }
        let end = self.subtree_end(i);
        for track in &mut self.tracks[i + 1..end] {
            track.depth -= 1;
        }
        let removed = self.tracks.remove(i);
        self.tidy_groups();
        Some(removed)
    }

    /// Repairs a tree read from a file: folders hold nothing of their own, the first track is at
    /// the top, and no track is deeper than the tree allows.
    pub(super) fn sanitize_tree(&mut self) {
        let mut allowed = 0;
        for track in &mut self.tracks {
            if track.folder {
                track.resource = None;
                track.items.clear();
            } else {
                track.collapsed = false;
            }
            track.depth = track.depth.min(allowed);
            allowed = track.depth + u32::from(track.folder);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ResourceKind;

    fn project() -> Project {
        Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap())
    }

    /// `(name, depth)` of every track, top first.
    fn shape(p: &Project) -> Vec<(String, u32)> {
        p.tracks.iter().map(|t| (t.name.clone(), t.depth)).collect()
    }

    fn named(list: &[(&str, u32)]) -> Vec<(String, u32)> {
        list.iter().map(|&(n, d)| (n.to_owned(), d)).collect()
    }

    /// A folder `F` holding video `v` and audio `a`, then audio `b` at the top.
    fn tree() -> Project {
        let mut p = project();
        p.add_folder("F");
        p.add_track(TrackKind::Video, "v", "v.mp4").depth = 1;
        p.add_track(TrackKind::Audio, "a", "a.wav").depth = 1;
        p.add_track(TrackKind::Audio, "b", "b.wav");
        p
    }

    #[test]
    fn folders_hold_the_deeper_tracks_below_them() {
        let p = tree();
        assert_eq!(p.parent_of(1), Some(0));
        assert_eq!(p.parent_of(2), Some(0));
        assert_eq!(p.parent_of(3), None);
        assert_eq!(p.subtree_end(0), 3);
        assert_eq!(p.subtree_end(1), 2);
        assert_eq!(p.track_kind(&p.tracks[0]), None);
        assert_eq!(p.track_kind(&p.tracks[1]), Some(TrackKind::Video));
        let audio: Vec<&str> = p
            .tracks_of(TrackKind::Audio)
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(audio, ["a", "b"]);
        let mut p = p;
        p.tracks[0].collapsed = true;
        assert_eq!(p.shown_tracks(), [0, 3]);
    }

    #[test]
    fn a_folders_level_mute_send_and_solo_reach_everything_in_it() {
        let mut p = tree();
        let gains = |p: &Project| -> Vec<(String, f32)> {
            p.timeline()
                .tracks
                .iter()
                .map(|t| (t.name.clone(), t.gain))
                .collect()
        };
        let pairs = |list: &[(&str, f32)]| -> Vec<(String, f32)> {
            list.iter().map(|&(n, g)| (n.to_owned(), g)).collect()
        };
        // Folders play nothing themselves, so the engine isn't told about them.
        assert_eq!(gains(&p), pairs(&[("v", 1.0), ("a", 1.0), ("b", 1.0)]));
        // A folder's volume scales its audio; a video track's own picture is all or nothing.
        p.tracks[0].volume = 0.5;
        p.tracks[2].volume = 0.5;
        assert_eq!(gains(&p), pairs(&[("v", 0.5), ("a", 0.25), ("b", 1.0)]));
        // A folder that sends nowhere takes what is in it out of the mix.
        p.tracks[0].master_send = false;
        assert_eq!(gains(&p), pairs(&[("v", 0.0), ("a", 0.0), ("b", 1.0)]));
        p.tracks[0].master_send = true;
        p.tracks[0].volume = 1.0;
        // Soloing the folder solos the audio in it, so the other audio track drops out.
        p.tracks[0].solo = true;
        assert_eq!(gains(&p), pairs(&[("v", 1.0), ("a", 0.5), ("b", 0.0)]));
        // Muting it mutes both kinds.
        p.tracks[0].muted = true;
        assert_eq!(gains(&p), pairs(&[("v", 0.0), ("a", 0.0), ("b", 0.0)]));
    }

    #[test]
    fn top_level_tracks_choose_the_bus_for_everything_in_them() {
        let mut p = tree();
        p.tracks[0].bus = "Stems".into();
        // A bus set inside a folder doesn't count: the folder's does.
        p.tracks[2].bus = "Other".into();
        let buses: Vec<String> = p.timeline().tracks.iter().map(|t| t.bus.clone()).collect();
        assert_eq!(buses, ["Stems", "Stems", "Main"]);
        assert_eq!(p.bus_users("Stems").0, ["F"]);
    }

    #[test]
    fn moving_a_folder_takes_its_tracks_and_a_track_cant_go_into_itself() {
        let mut p = tree();
        // The folder to the bottom: its tracks come with it.
        assert_eq!(p.move_track(0, 4, None), Some(1));
        assert_eq!(shape(&p), named(&[("b", 0), ("F", 0), ("v", 1), ("a", 1)]));
        // Not into itself.
        assert_eq!(p.move_track(1, 3, None), None);
        // A track dropped under a folder header goes first inside it.
        assert_eq!(p.move_track(0, 2, None), Some(1));
        assert_eq!(shape(&p), named(&[("F", 0), ("b", 1), ("v", 1), ("a", 1)]));
        // Out of the folder to the top level by asking for depth 0 at the bottom.
        assert_eq!(p.move_track(1, 4, Some(0)), Some(3));
        assert_eq!(shape(&p), named(&[("F", 0), ("v", 1), ("a", 1), ("b", 0)]));
        // A depth the tree can't have is clamped: nothing at the top can be inside anything.
        assert_eq!(p.move_track(3, 0, Some(5)), Some(0));
        assert_eq!(shape(&p), named(&[("b", 0), ("F", 0), ("v", 1), ("a", 1)]));
        // In the middle of a folder, it can only be inside it.
        assert_eq!(p.move_track(0, 3, Some(0)), Some(2));
        assert_eq!(shape(&p), named(&[("F", 0), ("v", 1), ("b", 1), ("a", 1)]));
        // Dropping where it is changes nothing.
        assert_eq!(p.move_track(2, 2, None), None);
    }

    #[test]
    fn drop_depths_follow_the_neighbours() {
        // F(folder) v a, then b, as (depth, folder).
        let tree = [(0, true), (1, false), (1, false), (0, false)];
        // Under the folder header: only inside it.
        assert_eq!(drop_depths(&tree, 3, 1), Some(1..=1));
        // After the folder's last track: inside it or beside it.
        assert_eq!(drop_depths(&tree, 3, 3), Some(0..=1));
        // At the top: the top.
        assert_eq!(drop_depths(&tree, 3, 0), Some(0..=0));
        // Not into itself.
        assert_eq!(drop_depths(&tree, 0, 2), None);
        // A folder, with its tracks, under a track that isn't one: beside it.
        assert_eq!(drop_depths(&tree, 0, 4), Some(0..=0));
        // Between a folder and its first track: only inside, or that track would be left out.
        let nested = [(0, true), (1, true), (2, false), (0, false)];
        assert_eq!(drop_depths(&nested, 3, 2), Some(2..=2));
    }

    #[test]
    fn the_template_has_three_folders_and_new_tracks_land_in_theirs() {
        let mut p = Project::with_template(
            GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap(),
        );
        let names: Vec<&str> = p.tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Video", "Audio", "Control"]);
        assert!(p.tracks.iter().all(|t| t.folder));
        assert!(!p.tracks[2].master_send, "Control reaches no output");
        let clip = p.add_resource(ResourceKind::Video, "clip", "clip.mp4", None);
        let song = p.add_resource(ResourceKind::Audio, "song", "song.wav", None);
        p.add_track_for(song, 0.0);
        p.add_track_for(clip, 0.0);
        p.add_track_for(clip, 0.0);
        p.add_empty_track();
        assert_eq!(
            shape(&p),
            named(&[
                ("Video", 0),
                ("clip", 1),
                ("clip_2", 1),
                ("Audio", 0),
                ("song", 1),
                ("Control", 0),
                ("Track", 0),
            ])
        );
    }

    #[test]
    fn removing_a_folder_keeps_what_was_in_it() {
        let mut p = tree();
        assert_eq!(p.remove_track(0).unwrap().name, "F");
        assert_eq!(shape(&p), named(&[("v", 0), ("a", 0), ("b", 0)]));
        assert!(p.remove_track(9).is_none());
    }

    #[test]
    fn a_tree_read_from_a_file_is_repaired() {
        let mut p = tree();
        p.tracks[0].resource = p.tracks[1].resource;
        p.tracks[3].depth = 4;
        p.tracks[1].collapsed = true;
        p.sanitize_tree();
        assert!(p.tracks[0].resource.is_none() && p.tracks[0].items.is_empty());
        assert!(!p.tracks[1].collapsed, "only folders collapse");
        assert_eq!(
            p.tracks[3].depth, 1,
            "no deeper than the folder above allows"
        );
        // Inside a non-folder is nowhere: the depth drops to the track above's.
        p.tracks.swap(0, 3);
        p.sanitize_tree();
        assert_eq!(shape(&p)[0], ("b".to_owned(), 0));
        assert_eq!(p.tracks[1].depth, 0);
    }
}
