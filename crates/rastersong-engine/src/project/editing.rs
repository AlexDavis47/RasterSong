//! Editing the items of a project's tracks: moving, trimming, stretching, splitting, deleting,
//! copying and pasting them, and grouping them.
//!
//! Every edit takes the items it acts on and adds the other items of their groups (see
//! [`Project::with_grouped`]), so grouped items move, trim, split and delete together. Edits that
//! need item lengths take `duration`, which gives a track's resource length in seconds, `None`
//! while unknown (its items then last forever).

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use super::{Project, ProjectTrack};
use crate::timeline::Item;

/// The shortest an item can be trimmed or stretched to, in seconds.
pub const MIN_ITEM_LENGTH: f64 = 0.01;

/// The slowest and fastest an edge drag can stretch an item to play, in resource seconds per
/// timeline second.
pub const RATE_RANGE: RangeInclusive<f64> = 0.05..=20.0;

/// How close (seconds) the edge of a grouped item must be to the edge being trimmed to be trimmed
/// with it.
const EDGE_MATCH: f64 = 1e-3;

/// How far inside an item a split must fall, in seconds; a split at its very edge does nothing.
const SPLIT_MARGIN: f64 = 1e-6;

/// An item, named by its track and its place in the track's items.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemRef {
    pub track: String,
    pub item: usize,
}

impl ItemRef {
    pub fn new(track: impl Into<String>, item: usize) -> Self {
        Self {
            track: track.into(),
            item,
        }
    }
}

/// One end of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

/// Where an item lies on the timeline, in seconds.
fn span(item: &Item, duration: Option<f64>) -> (f64, f64) {
    (
        item.position,
        item.timeline_end(duration.unwrap_or(f64::INFINITY)),
    )
}

/// The correction that snaps the nearest of `edges` onto a target, or 0 when none is within
/// `threshold` seconds. Targets are `targets` and, for each edge, `grid(edge)` (the nearest grid
/// line to it).
pub fn snap_offset(
    edges: &[f64],
    targets: &[f64],
    grid: impl Fn(f64) -> f64,
    threshold: f64,
) -> f64 {
    edges
        .iter()
        .flat_map(|&e| {
            targets
                .iter()
                .copied()
                .chain(std::iter::once(grid(e)))
                .map(move |t| t - e)
        })
        .filter(|d| d.is_finite() && d.abs() <= threshold)
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
        .unwrap_or(0.0)
}

impl Project {
    fn item_ref(&self, at: &ItemRef) -> Option<(&ProjectTrack, &Item)> {
        let track = self.track(&at.track)?;
        Some((track, track.items.get(at.item)?))
    }

    fn track_mut(&mut self, name: &str) -> Option<&mut ProjectTrack> {
        self.tracks_mut().find(|t| t.name == name)
    }

    fn item_mut(&mut self, at: &ItemRef) -> Option<&mut Item> {
        self.track_mut(&at.track)?.items.get_mut(at.item)
    }

    /// `items`, with the other items of their groups, sorted and without repeats. References to
    /// items that don't exist are dropped.
    pub fn with_grouped(&self, items: &[ItemRef]) -> Vec<ItemRef> {
        let mut all: Vec<ItemRef> = Vec::new();
        let mut groups = Vec::new();
        for at in items {
            if let Some((_, item)) = self.item_ref(at) {
                all.push(at.clone());
                groups.extend(item.group);
            }
        }
        for track in self.tracks() {
            for (k, item) in track.items.iter().enumerate() {
                if item.group.is_some_and(|g| groups.contains(&g)) {
                    all.push(ItemRef::new(track.name.clone(), k));
                }
            }
        }
        all.sort();
        all.dedup();
        all
    }

    /// A group number no item has.
    fn new_group(&self) -> u32 {
        self.tracks()
            .flat_map(|t| &t.items)
            .filter_map(|i| i.group)
            .max()
            .map_or(1, |g| g + 1)
    }

    /// Groups `items`, with the other items of the groups they are already in, into one group.
    /// Returns it, or `None` when it would hold fewer than two items.
    pub fn group_items(&mut self, items: &[ItemRef]) -> Option<u32> {
        let members = self.with_grouped(items);
        if members.len() < 2 {
            return None;
        }
        let group = self.new_group();
        for at in &members {
            if let Some(item) = self.item_mut(at) {
                item.group = Some(group);
            }
        }
        Some(group)
    }

    /// Ungroups the groups `items` are in. Returns whether any was.
    pub fn ungroup_items(&mut self, items: &[ItemRef]) -> bool {
        let mut any = false;
        for at in self.with_grouped(items) {
            if let Some(item) = self.item_mut(&at) {
                any |= item.group.take().is_some();
            }
        }
        any
    }

    /// Ends the groups left with one item.
    pub(super) fn tidy_groups(&mut self) {
        let mut sizes: BTreeMap<u32, usize> = BTreeMap::new();
        for group in self.tracks().flat_map(|t| &t.items).filter_map(|i| i.group) {
            *sizes.entry(group).or_default() += 1;
        }
        for item in self.tracks_mut().flat_map(|t| &mut t.items) {
            if item.group.is_some_and(|g| sizes[&g] < 2) {
                item.group = None;
            }
        }
    }

    /// Moves item `item` of track `name` by `delta` seconds; see [`Self::move_items`].
    pub fn move_item(&mut self, name: &str, item: usize, delta: f64) -> f64 {
        self.move_items(&[ItemRef::new(name, item)], delta)
    }

    /// Moves `items` by `delta` seconds, with the other items of their groups, and returns how
    /// far they moved: no item moves before the start of the timeline, so a move left stops when
    /// the first of them reaches it.
    pub fn move_items(&mut self, items: &[ItemRef], delta: f64) -> f64 {
        let moving = self.with_grouped(items);
        if moving.is_empty() {
            return 0.0;
        }
        let earliest = moving
            .iter()
            .filter_map(|at| self.item_ref(at))
            .map(|(_, i)| i.position)
            .fold(f64::INFINITY, f64::min);
        let delta = delta.max(-earliest);
        if !delta.is_finite() || delta == 0.0 {
            return 0.0;
        }
        for at in &moving {
            if let Some(item) = self.item_mut(at) {
                item.position = (item.position + delta).max(0.0);
            }
        }
        delta
    }

    /// Drags edge `edge` of item `at` to timeline time `to`, with the same edge of the other
    /// items of its group that lies at the same time. A trim moves the edge over the resource,
    /// keeping its speed, and stops at the resource's ends; a `stretch` (Alt+drag) keeps the
    /// item's in and out points and changes its rate instead, like tape, within
    /// [`RATE_RANGE`]. The far edge stays where it is either way, and no item gets shorter than
    /// [`MIN_ITEM_LENGTH`].
    pub fn trim_item(
        &mut self,
        at: &ItemRef,
        edge: Edge,
        to: f64,
        stretch: bool,
        duration: impl Fn(&ProjectTrack) -> Option<f64>,
    ) {
        let edge_of = |project: &Self, at: &ItemRef| {
            let (track, item) = project.item_ref(at)?;
            let (start, end) = span(item, duration(track));
            Some((
                match edge {
                    Edge::Start => start,
                    Edge::End => end,
                },
                duration(track),
            ))
        };
        let Some((from, _)) = edge_of(self, at) else {
            return;
        };
        let targets: Vec<(ItemRef, Option<f64>)> = self
            .with_grouped(std::slice::from_ref(at))
            .into_iter()
            .filter_map(|other| {
                let (this, length) = edge_of(self, &other)?;
                ((this - from).abs() <= EDGE_MATCH).then_some((other, length))
            })
            .collect();
        for (at, length) in targets {
            if let Some(item) = self.item_mut(&at) {
                trim(item, edge, to, stretch, length.unwrap_or(f64::INFINITY));
            }
        }
    }

    /// Splits `items`, and the other items of their groups, at timeline time `at`. Items that `at`
    /// doesn't fall inside are left alone. The halves after the split make groups of their own,
    /// so a grouped picture and its sound stay together on both sides. Returns where `items` are
    /// afterwards: both halves of each split one, so the selection stays on the same stretch of
    /// timeline.
    pub fn split_items(
        &mut self,
        items: &[ItemRef],
        at: f64,
        duration: impl Fn(&ProjectTrack) -> Option<f64>,
    ) -> Vec<ItemRef> {
        let targets = self.with_grouped(items);
        let mut next = self.new_group();
        let mut after: BTreeMap<u32, u32> = BTreeMap::new();
        let mut result = Vec::new();
        let lengths: Vec<(String, Option<f64>)> = self
            .tracks()
            .map(|t| (t.name.clone(), duration(t)))
            .collect();
        for (name, length) in lengths {
            let Some(track) = self.track_mut(&name) else {
                continue;
            };
            let length = length.unwrap_or(f64::INFINITY);
            let mut kept = Vec::with_capacity(track.items.len());
            for (k, item) in std::mem::take(&mut track.items).into_iter().enumerate() {
                let wanted = items.contains(&ItemRef::new(name.clone(), k));
                if wanted {
                    result.push(ItemRef::new(name.clone(), kept.len()));
                }
                let split = targets.contains(&ItemRef::new(name.clone(), k))
                    && at > item.position + SPLIT_MARGIN
                    && at < item.timeline_end(length) - SPLIT_MARGIN;
                if !split {
                    kept.push(item);
                    continue;
                }
                let cut = item.source_time(at);
                kept.push(Item {
                    end: Some(cut),
                    ..item.clone()
                });
                if wanted {
                    result.push(ItemRef::new(name.clone(), kept.len()));
                }
                let group = item.group.map(|g| {
                    *after.entry(g).or_insert_with(|| {
                        next += 1;
                        next - 1
                    })
                });
                kept.push(Item {
                    position: at,
                    start: cut,
                    group,
                    ..item
                });
            }
            track.items = kept;
        }
        self.tidy_groups();
        result
    }

    /// Removes `items` and the other items of their groups.
    pub fn delete_items(&mut self, items: &[ItemRef]) {
        // Sorted, so removing from the back keeps the earlier indices valid.
        for at in self.with_grouped(items).iter().rev() {
            if let Some(track) = self.track_mut(&at.track)
                && at.item < track.items.len()
            {
                track.items.remove(at.item);
            }
        }
        self.tidy_groups();
    }

    /// Copies of `items` and the other items of their groups, each with the name of its track,
    /// for [`Self::paste_items`].
    pub fn copy_items(&self, items: &[ItemRef]) -> Vec<(String, Item)> {
        self.with_grouped(items)
            .iter()
            .filter_map(|at| {
                self.item_ref(at)
                    .map(|(_, i)| (at.track.clone(), i.clone()))
            })
            .collect()
    }

    /// Adds copied items back to the tracks they came from, moved together so the earliest
    /// starts at `at`, on top of what is there. Copied groups become new groups. Items of tracks
    /// that no longer exist are dropped. Returns where the new items are.
    pub fn paste_items(&mut self, copied: &[(String, Item)], at: f64) -> Vec<ItemRef> {
        let earliest = copied
            .iter()
            .map(|(_, i)| i.position)
            .fold(f64::INFINITY, f64::min);
        let mut next = self.new_group();
        let mut groups: BTreeMap<u32, u32> = BTreeMap::new();
        let mut pasted = Vec::new();
        for (name, item) in copied {
            let Some(track) = self.track_mut(name) else {
                continue;
            };
            let group = item.group.map(|g| {
                *groups.entry(g).or_insert_with(|| {
                    next += 1;
                    next - 1
                })
            });
            track.items.push(Item {
                position: (item.position - earliest + at).max(0.0),
                group,
                ..item.clone()
            });
            pasted.push(ItemRef::new(name.clone(), track.items.len() - 1));
        }
        self.tidy_groups();
        pasted
    }
}

/// Moves edge `edge` of `item` to timeline time `to`, for a resource `length` seconds long; see
/// [`Project::trim_item`].
fn trim(item: &mut Item, edge: Edge, to: f64, stretch: bool, length: f64) {
    if !to.is_finite() {
        return;
    }
    let end = item.timeline_end(length);
    match (edge, stretch) {
        (Edge::Start, false) => {
            // Back to the resource's start, or the timeline's.
            let earliest = (item.position - item.start / item.rate).max(0.0);
            let latest = end - MIN_ITEM_LENGTH;
            if latest < earliest {
                return;
            }
            let position = to.clamp(earliest, latest);
            item.start = (item.start + (position - item.position) * item.rate).max(0.0);
            item.position = position;
        }
        (Edge::End, false) => {
            let shortest = item.position + MIN_ITEM_LENGTH;
            let latest = (item.position + (length - item.start) / item.rate).max(shortest);
            let end = to.clamp(shortest, latest);
            item.end = Some(item.start + (end - item.position) * item.rate);
        }
        (edge, true) => {
            let played = item.out(length) - item.start;
            if !end.is_finite() || played <= 0.0 {
                return;
            }
            let shortest = (played / RATE_RANGE.end()).max(MIN_ITEM_LENGTH);
            let longest = played / RATE_RANGE.start();
            let new_length = match edge {
                // The start can't go before the timeline's.
                Edge::Start => (end - to).clamp(shortest, longest.min(end).max(shortest)),
                Edge::End => (to - item.position).clamp(shortest, longest),
            };
            item.end = Some(item.out(length));
            item.rate = played / new_length;
            if edge == Edge::Start {
                item.position = (end - new_length).max(0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GraphDesc;
    use crate::timeline::TrackKind;

    /// Tracks `v` (video, 10 s) and `a`, `b` (audio, 2 s each), with `items` placed whole.
    fn project(items: &[(&str, f64)]) -> Project {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        project.add_track(TrackKind::Video, "v", "v.mp4");
        for name in ["a", "b"] {
            project.add_track(TrackKind::Audio, name, format!("{name}.wav"));
        }
        for track in project.tracks_mut() {
            track.items.clear();
        }
        for &(name, position) in items {
            project
                .track_mut(name)
                .unwrap()
                .items
                .push(Item::whole(position));
        }
        project
    }

    fn duration(track: &ProjectTrack) -> Option<f64> {
        Some(if track.name == "v" { 10.0 } else { 2.0 })
    }

    /// Each track's items as (position, start, end, rate).
    fn items(project: &Project, name: &str) -> Vec<(f64, f64, Option<f64>, f64)> {
        project
            .track(name)
            .unwrap()
            .items
            .iter()
            .map(|i| (i.position, i.start, i.end, i.rate))
            .collect()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn moving_several_items_stops_when_the_first_reaches_the_start() {
        let mut p = project(&[("a", 1.0), ("a", 5.0), ("b", 3.0)]);
        let selection = [ItemRef::new("a", 1), ItemRef::new("b", 0)];
        assert_eq!(p.move_items(&selection, 1.0), 1.0);
        assert_eq!(p.move_items(&selection, -10.0), -4.0);
        assert_eq!(items(&p, "a")[1].0, 2.0);
        assert_eq!(items(&p, "b")[0].0, 0.0);
        // The unselected item stayed.
        assert_eq!(items(&p, "a")[0].0, 1.0);
    }

    #[test]
    fn trimming_moves_an_edge_over_the_resource_within_its_ends() {
        let mut p = project(&[("a", 1.0)]);
        let a = ItemRef::new("a", 0);
        p.trim_item(&a, Edge::Start, 1.5, false, duration);
        assert_eq!(items(&p, "a"), [(1.5, 0.5, None, 1.0)]);
        // Not before the resource's start.
        p.trim_item(&a, Edge::Start, 0.0, false, duration);
        assert_eq!(items(&p, "a"), [(1.0, 0.0, None, 1.0)]);
        p.trim_item(&a, Edge::End, 2.5, false, duration);
        assert_eq!(items(&p, "a"), [(1.0, 0.0, Some(1.5), 1.0)]);
        // Not past the resource's end, nor shorter than the minimum.
        p.trim_item(&a, Edge::End, 9.0, false, duration);
        assert_eq!(items(&p, "a"), [(1.0, 0.0, Some(2.0), 1.0)]);
        p.trim_item(&a, Edge::End, 0.0, false, duration);
        assert!(close(items(&p, "a")[0].2.unwrap(), MIN_ITEM_LENGTH));
    }

    #[test]
    fn stretching_changes_the_rate_and_keeps_the_far_edge() {
        let mut p = project(&[("a", 1.0)]);
        let a = ItemRef::new("a", 0);
        // Dragging the end from 3 to 5: twice as long, half the speed.
        p.trim_item(&a, Edge::End, 5.0, true, duration);
        assert_eq!(items(&p, "a"), [(1.0, 0.0, Some(2.0), 0.5)]);
        // Dragging the start from 1 to 4: the end stays at 5, 1 s for 2 s of resource.
        p.trim_item(&a, Edge::Start, 4.0, true, duration);
        let (position, _, _, rate) = items(&p, "a")[0];
        assert!(close(position, 4.0) && close(rate, 2.0));
        // Within the rate range.
        p.trim_item(&a, Edge::End, 4.0 + 1e-6, true, duration);
        assert!(close(items(&p, "a")[0].3, *RATE_RANGE.end()));
    }

    #[test]
    fn grouped_items_move_together_until_ungrouped() {
        let mut p = project(&[("v", 0.0), ("a", 0.0), ("a", 5.0), ("b", 1.0)]);
        let pair = [ItemRef::new("v", 0), ItemRef::new("a", 0)];
        assert_eq!(p.group_items(&pair[..1]), None);
        let group = p.group_items(&pair).unwrap();
        assert_eq!(
            p.with_grouped(&pair[1..]),
            [ItemRef::new("a", 0), ItemRef::new("v", 0)]
        );
        assert_eq!(p.move_item("a", 0, 1.0), 1.0);
        assert_eq!(items(&p, "v")[0].0, 1.0);
        assert_eq!(items(&p, "a")[1].0, 5.0);
        assert_eq!(items(&p, "b")[0].0, 1.0);
        // Grouping an item of a group with another takes in the whole group.
        let bigger = p
            .group_items(&[ItemRef::new("a", 0), ItemRef::new("b", 0)])
            .unwrap();
        assert_ne!(bigger, group);
        assert_eq!(p.with_grouped(&[ItemRef::new("b", 0)]).len(), 3);
        assert!(p.ungroup_items(&[ItemRef::new("v", 0)]));
        assert!(!p.ungroup_items(&[ItemRef::new("v", 0)]));
        assert_eq!(p.move_item("a", 0, 1.0), 1.0);
        assert_eq!(items(&p, "v")[0].0, 1.0);
    }

    #[test]
    fn trimming_takes_the_matching_edges_of_grouped_items() {
        let mut p = project(&[("v", 0.0), ("a", 0.0), ("b", 0.5)]);
        p.group_items(&[
            ItemRef::new("v", 0),
            ItemRef::new("a", 0),
            ItemRef::new("b", 0),
        ]);
        p.trim_item(&ItemRef::new("a", 0), Edge::Start, 0.25, false, duration);
        assert_eq!(items(&p, "v")[0].0, 0.25);
        assert_eq!(items(&p, "a")[0].0, 0.25);
        // b's start wasn't at the same time.
        assert_eq!(items(&p, "b")[0].0, 0.5);
    }

    #[test]
    fn splitting_cuts_grouped_items_and_keeps_both_halves_selected() {
        let mut p = project(&[("v", 0.0), ("a", 0.0), ("a", 4.0)]);
        p.group_items(&[ItemRef::new("v", 0), ItemRef::new("a", 0)]);
        let selection = p.split_items(&[ItemRef::new("a", 0), ItemRef::new("a", 1)], 1.5, duration);
        assert_eq!(
            items(&p, "a"),
            [
                (0.0, 0.0, Some(1.5), 1.0),
                (1.5, 1.5, None, 1.0),
                (4.0, 0.0, None, 1.0)
            ]
        );
        assert_eq!(
            selection,
            [
                ItemRef::new("a", 0),
                ItemRef::new("a", 1),
                ItemRef::new("a", 2)
            ]
        );
        // The grouped video was cut with it, and each side is a group of its own.
        assert_eq!(
            items(&p, "v"),
            [(0.0, 0.0, Some(1.5), 1.0), (1.5, 1.5, None, 1.0)]
        );
        assert_eq!(
            p.with_grouped(&[ItemRef::new("a", 1)]),
            [ItemRef::new("a", 1), ItemRef::new("v", 1)]
        );
        assert_eq!(
            p.with_grouped(&[ItemRef::new("v", 0)]),
            [ItemRef::new("a", 0), ItemRef::new("v", 0)]
        );
        // A split at an edge, or outside, does nothing.
        p.split_items(&[ItemRef::new("a", 1)], 1.5, duration);
        assert_eq!(items(&p, "a").len(), 3);
    }

    #[test]
    fn a_split_follows_the_rate() {
        let mut p = project(&[("a", 1.0)]);
        p.track_mut("a").unwrap().items[0].rate = 2.0;
        p.split_items(&[ItemRef::new("a", 0)], 1.5, duration);
        assert_eq!(
            items(&p, "a"),
            [(1.0, 0.0, Some(1.0), 2.0), (1.5, 1.0, None, 2.0)]
        );
    }

    #[test]
    fn deleting_takes_grouped_items_and_copies_paste_at_a_time() {
        let mut p = project(&[("v", 0.0), ("a", 0.0), ("a", 4.0), ("b", 1.0)]);
        p.group_items(&[ItemRef::new("v", 0), ItemRef::new("a", 1)]);
        let copied = p.copy_items(&[ItemRef::new("a", 1)]);
        // The video is in a's second item's group.
        assert_eq!(copied.len(), 2);
        let pasted = p.paste_items(&copied, 20.0);
        assert_eq!(pasted, [ItemRef::new("a", 2), ItemRef::new("v", 1)]);
        // The earliest copied item (the video at 0) lands at 20; a's keeps its offset.
        assert_eq!(items(&p, "v")[1].0, 20.0);
        assert_eq!(items(&p, "a")[2].0, 24.0);
        // The pasted pair is a group of its own.
        assert_eq!(p.with_grouped(&[ItemRef::new("v", 1)]).len(), 2);
        assert_eq!(p.with_grouped(&[ItemRef::new("a", 1)]).len(), 2);
        assert_ne!(
            p.track("v").unwrap().items[0].group,
            p.track("v").unwrap().items[1].group
        );
        p.delete_items(&[ItemRef::new("a", 1)]);
        assert_eq!(items(&p, "a").len(), 2);
        assert_eq!(items(&p, "v"), [(20.0, 0.0, None, 1.0)]);
        assert_eq!(items(&p, "b").len(), 1);
        // A track that is gone takes nothing.
        assert!(
            p.paste_items(&[("gone".into(), Item::whole(0.0))], 0.0)
                .is_empty()
        );
    }

    #[test]
    fn snapping_picks_the_nearest_target_within_reach() {
        let none = |e: f64| e + 100.0;
        assert!(close(
            snap_offset(&[1.0, 3.0], &[3.2, 0.95], none, 0.1),
            -0.05
        ));
        assert_eq!(snap_offset(&[1.0], &[2.0], none, 0.1), 0.0);
        // The grid line nearest each edge counts too.
        let grid = |e: f64| e.round();
        assert!(close(snap_offset(&[1.04], &[], grid, 0.1), -0.04));
    }
}
