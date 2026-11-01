//! Editing the items of a project's tracks, of every kind: placing, moving, trimming,
//! stretching, splitting, deleting, copying and pasting them, and grouping them.
//!
//! Items on a track never overlap: whatever an edit puts down trims what it lands on, cutting an
//! item it lands inside in two, and a trim stops at the items either side. Every edit takes the
//! items it acts on and adds the other items of their groups (see [`Project::with_grouped`]), so
//! grouped items move, trim, split and delete together. Edits that need item lengths take
//! [`ItemLengths`].

use std::collections::{BTreeMap, HashMap};
use std::ops::RangeInclusive;

use super::{Project, ProjectTrack, ResourceId};
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

/// How far (seconds) two items may run into each other and still count as only touching, so
/// rounding never trims a sliver off a neighbour.
const OVERLAP_MARGIN: f64 = 1e-9;

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

/// The lengths edits need: how long each file's resource is, and how long a graph is placed for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemLengths {
    /// How long each video and audio resource lasts, in seconds, once its file has been read.
    /// An item without an out point of a resource not listed has no known length: until it has,
    /// it trims nothing it lies over, and nothing put down over it trims it.
    pub durations: HashMap<ResourceId, f64>,
    /// How long a graph placed by itself lasts, in seconds (at least [`MIN_ITEM_LENGTH`]).
    pub new_graph_length: f64,
}

impl ItemLengths {
    /// How long resource `id` lasts: its file's length, or forever for a graph (which runs for
    /// as long as its item lasts) and a file not read yet.
    pub fn duration(&self, id: ResourceId) -> f64 {
        self.durations.get(&id).copied().unwrap_or(f64::INFINITY)
    }

    /// Where `item` ends on the timeline, if that is known: it has an out point, or its
    /// resource's length is known.
    pub fn end_of(&self, item: &Item) -> Option<f64> {
        let duration = match item.end {
            Some(_) => f64::INFINITY,
            None => *self.durations.get(&item.resource)?,
        };
        Some(item.timeline_end(duration))
    }
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

    /// The item `at` names.
    pub fn item(&self, at: &ItemRef) -> Option<&Item> {
        self.item_ref(at).map(|(_, item)| item)
    }

    pub fn item_mut(&mut self, at: &ItemRef) -> Option<&mut Item> {
        self.track_mut(&at.track)?.items.get_mut(at.item)
    }

    pub(super) fn track_mut(&mut self, name: &str) -> Option<&mut ProjectTrack> {
        self.tracks_mut().find(|t| t.name == name)
    }

    /// `items`, with the other items of their groups, sorted and without repeats. References to
    /// items that don't exist are dropped.
    pub fn with_grouped(&self, items: &[ItemRef]) -> Vec<ItemRef> {
        let mut all: Vec<ItemRef> = Vec::new();
        let mut groups = Vec::new();
        for at in items {
            if let Some(item) = self.item(at) {
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

    /// Whether putting items of `incoming` on track `track` (by index), once `leaving` have left
    /// it, would give a video or audio track items of two resources. Until the renderer reads a
    /// file per item, a video or audio track plays one resource.
    pub(super) fn mixes_resources(
        &self,
        track: usize,
        incoming: &[ResourceId],
        leaving: &[ItemRef],
    ) -> bool {
        let Some(t) = self.tracks.get(track) else {
            return false;
        };
        if !t.kind.reads_file() {
            return false;
        }
        let staying = t
            .items
            .iter()
            .enumerate()
            .filter(|(k, _)| !leaving.contains(&ItemRef::new(t.name.clone(), *k)))
            .map(|(_, i)| i.resource);
        let mut all = staying.chain(incoming.iter().copied());
        all.next()
            .is_some_and(|first| all.any(|other| other != first))
    }

    /// Puts resource `id` on track `track` from `position` seconds, trimming what it lands on: a
    /// file's whole length, or a graph for [`ItemLengths::new_graph_length`]. Returns the new
    /// item's place, or `None` when the track can't take it (see
    /// [`Self::can_place_resource`]).
    pub fn place_resource(
        &mut self,
        track: &str,
        id: ResourceId,
        position: f64,
        lengths: &ItemLengths,
    ) -> Option<usize> {
        if !self.can_place_resource(track, id) {
            return None;
        }
        let position = position.max(0.0);
        let item = if self.resource(id)?.kind().has_duration() {
            Item::whole(id, position)
        } else {
            Item::graph(id, position, lengths.new_graph_length.max(MIN_ITEM_LENGTH))
        };
        let track = self.track_mut(track)?;
        Some(insert_over(&mut track.items, vec![item], lengths)[0])
    }

    /// Moves `items`, with the other items of their groups, by `delta` seconds and by `rows`
    /// tracks (down for positive), trimming what they land on. No item moves before the start of
    /// the timeline, so a move left stops when the first of them reaches it, and none leaves its
    /// section: a move that would take one onto a track of another kind, or one that can't take
    /// it, moves nothing. Returns where `items` are afterwards.
    pub fn move_items(
        &mut self,
        items: &[ItemRef],
        delta: f64,
        rows: isize,
        lengths: &ItemLengths,
    ) -> Vec<ItemRef> {
        let unmoved: Vec<ItemRef> = items
            .iter()
            .filter(|at| self.item(at).is_some())
            .cloned()
            .collect();
        let moving = self.with_grouped(items);
        if moving.is_empty() || !delta.is_finite() {
            return unmoved;
        }
        let earliest = moving
            .iter()
            .filter_map(|at| self.item(at))
            .map(|i| i.position)
            .fold(f64::INFINITY, f64::min);
        let delta = delta.max(-earliest);
        if delta == 0.0 && rows == 0 {
            return unmoved;
        }
        // Each moving item's new track, by index.
        let mut to = Vec::with_capacity(moving.len());
        for at in &moving {
            let Some(from) = self.track_index(&at.track) else {
                return unmoved;
            };
            let section = self.section(self.tracks[from].kind);
            match from.checked_add_signed(rows) {
                Some(target) if section.contains(&target) => to.push(target),
                _ => return unmoved,
            }
        }
        for &target in &to {
            let incoming: Vec<ResourceId> = moving
                .iter()
                .zip(&to)
                .filter(|(_, t)| **t == target)
                .filter_map(|(at, _)| self.item(at).map(|i| i.resource))
                .collect();
            if self.mixes_resources(target, &incoming, &moving) {
                return unmoved;
            }
        }
        // Take every moving item out, back to front so the earlier places stay right, then put
        // each track's arrivals down together.
        let mut taken: Vec<(usize, Item)> = Vec::with_capacity(moving.len());
        for (at, &target) in moving.iter().zip(&to).rev() {
            let Some(track) = self.track_mut(&at.track) else {
                continue;
            };
            let mut item = track.items.remove(at.item);
            item.position = (item.position + delta).max(0.0);
            taken.push((target, item));
        }
        taken.reverse();
        let mut landed: Vec<ItemRef> = vec![ItemRef::new("", 0); moving.len()];
        let mut targets = to.clone();
        targets.sort_unstable();
        targets.dedup();
        for target in targets {
            let (which, arrivals): (Vec<usize>, Vec<Item>) = taken
                .iter()
                .enumerate()
                .filter(|(_, (t, _))| *t == target)
                .map(|(k, (_, item))| (k, item.clone()))
                .unzip();
            let track = &mut self.tracks[target];
            let places = insert_over(&mut track.items, arrivals, lengths);
            for (k, place) in which.into_iter().zip(places) {
                landed[k] = ItemRef::new(track.name.clone(), place);
            }
        }
        items
            .iter()
            .filter_map(|at| moving.iter().position(|m| m == at))
            .map(|k| landed[k].clone())
            .collect()
    }

    /// Drags edge `edge` of item `at` to timeline time `to`, with the same edge of the other
    /// items of its group that lies at the same time. A trim moves the edge over the resource,
    /// keeping its speed, and stops at the resource's ends and at the items either side; a
    /// `stretch` (Alt+drag, for kinds that [stretch](crate::TrackKind::stretches)) keeps the
    /// item's in and out points and changes its rate instead, like tape, within [`RATE_RANGE`].
    /// The far edge stays where it is either way, and no item gets shorter than
    /// [`MIN_ITEM_LENGTH`].
    pub fn trim_item(
        &mut self,
        at: &ItemRef,
        edge: Edge,
        to: f64,
        stretch: bool,
        lengths: &ItemLengths,
    ) {
        let edge_of = |item: &Item| match edge {
            Edge::Start => item.position,
            Edge::End => lengths.end_of(item).unwrap_or(f64::INFINITY),
        };
        let Some(from) = self.item(at).map(edge_of) else {
            return;
        };
        let targets: Vec<ItemRef> = self
            .with_grouped(std::slice::from_ref(at))
            .into_iter()
            .filter(|other| {
                self.item(other)
                    .is_some_and(|i| (edge_of(i) - from).abs() <= EDGE_MATCH)
            })
            .collect();
        for at in targets {
            let Some(track) = self.track_mut(&at.track) else {
                continue;
            };
            // Items are in order and never overlap: the room is the gap either side.
            let before = at.item.checked_sub(1).map_or(0.0, |k| {
                let prev = &track.items[k];
                lengths.end_of(prev).unwrap_or(prev.position)
            });
            let after = track
                .items
                .get(at.item + 1)
                .map_or(f64::INFINITY, |i| i.position);
            let stretch = stretch && track.kind.stretches();
            let item = &mut track.items[at.item];
            let length = lengths.duration(item.resource);
            trim(item, edge, to, stretch, length, (before, after));
        }
    }

    /// Splits `items`, and the other items of their groups, at timeline time `at`. Items that `at`
    /// doesn't fall inside are left alone. The halves after the split make groups of their own,
    /// so a grouped picture and its sound stay together on both sides. Returns where `items` are
    /// afterwards: the first half of each split one, so a selection kept from it moves one side
    /// of the cut, not both.
    pub fn split_items(
        &mut self,
        items: &[ItemRef],
        at: f64,
        lengths: &ItemLengths,
    ) -> Vec<ItemRef> {
        let targets = self.with_grouped(items);
        let mut next = self.new_group();
        let mut after: BTreeMap<u32, u32> = BTreeMap::new();
        let mut result = Vec::new();
        for track in &mut self.tracks {
            let name = track.name.clone();
            let mut kept = Vec::with_capacity(track.items.len());
            for (k, item) in std::mem::take(&mut track.items).into_iter().enumerate() {
                let wanted = items.contains(&ItemRef::new(name.clone(), k));
                if wanted {
                    result.push(ItemRef::new(name.clone(), kept.len()));
                }
                let end = lengths.end_of(&item).unwrap_or(f64::INFINITY);
                let split = targets.contains(&ItemRef::new(name.clone(), k))
                    && at > item.position + SPLIT_MARGIN
                    && at < end - SPLIT_MARGIN;
                if !split {
                    kept.push(item);
                    continue;
                }
                let cut = item.source_time(at);
                kept.push(Item {
                    end: Some(cut),
                    ..item.clone()
                });
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
            .filter_map(|at| self.item(at).map(|i| (at.track.clone(), i.clone())))
            .collect()
    }

    /// Puts copied items back on the tracks they came from, moved together so the earliest
    /// starts at `at`, trimming what they land on. Copied groups become new groups. Items of
    /// tracks that no longer exist, or can't take them now, are dropped. Returns where the new
    /// items are, in the order copied.
    pub fn paste_items(
        &mut self,
        copied: &[(String, Item)],
        at: f64,
        lengths: &ItemLengths,
    ) -> Vec<ItemRef> {
        let earliest = copied
            .iter()
            .map(|(_, i)| i.position)
            .fold(f64::INFINITY, f64::min);
        let mut next = self.new_group();
        let mut groups: BTreeMap<u32, u32> = BTreeMap::new();
        // The copies each track takes, with their place in `copied`.
        let mut arrivals: BTreeMap<usize, Vec<(usize, Item)>> = BTreeMap::new();
        for (k, (name, item)) in copied.iter().enumerate() {
            let Some(track) = self.track_index(name) else {
                continue;
            };
            let kind = self.tracks[track].kind;
            let fits = self
                .resource(item.resource)
                .is_some_and(|r| r.kind() == kind);
            if !fits || self.mixes_resources(track, &[item.resource], &[]) {
                continue;
            }
            let group = item.group.map(|g| {
                *groups.entry(g).or_insert_with(|| {
                    next += 1;
                    next - 1
                })
            });
            arrivals.entry(track).or_default().push((
                k,
                Item {
                    position: (item.position - earliest + at).max(0.0),
                    group,
                    ..item.clone()
                },
            ));
        }
        let mut pasted: Vec<(usize, ItemRef)> = Vec::new();
        for (track, copies) in arrivals {
            let (which, items): (Vec<usize>, Vec<Item>) = copies.into_iter().unzip();
            let track = &mut self.tracks[track];
            let places = insert_over(&mut track.items, items, lengths);
            pasted.extend(
                which
                    .into_iter()
                    .zip(places)
                    .map(|(k, place)| (k, ItemRef::new(track.name.clone(), place))),
            );
        }
        self.tidy_groups();
        pasted.sort_by_key(|(k, _)| *k);
        pasted.into_iter().map(|(_, at)| at).collect()
    }

    /// Repairs every track's items, for files: nonsense times replaced, a graph item given an out
    /// point and its own rate, and the items put in order with overlaps trimmed (where lengths
    /// are known without the files: a file's item without an out point is left as it is).
    pub(super) fn sanitize_items(&mut self) {
        let lengths = ItemLengths::default();
        for track in &mut self.tracks {
            let mut items: Vec<Item> = std::mem::take(&mut track.items)
                .into_iter()
                .map(Item::sanitized)
                .collect();
            if !track.kind.stretches() {
                for item in &mut items {
                    item.rate = 1.0;
                }
            }
            if !track.kind.has_duration() {
                for item in &mut items {
                    let shortest = item.start + MIN_ITEM_LENGTH;
                    item.end = Some(item.end.map_or(shortest, |e| e.max(shortest)));
                }
            }
            items.sort_by(|a, b| a.position.total_cmp(&b.position));
            for item in items {
                insert_over(&mut track.items, vec![item], &lengths);
            }
        }
    }
}

/// Puts `new` among `items` (in order, not overlapping), trimming what they cover: an item one
/// lies inside is cut in two, both halves keeping its group. `new` must not overlap one another.
/// An item whose length isn't known neither trims nor is trimmed. Returns where each of `new`
/// ends up.
fn insert_over(items: &mut Vec<Item>, new: Vec<Item>, lengths: &ItemLengths) -> Vec<usize> {
    let count = new.len();
    let spans: Vec<(f64, f64)> = new
        .iter()
        .filter_map(|i| Some((i.position, lengths.end_of(i)?)))
        .collect();
    let mut kept = std::mem::take(items);
    for &(start, end) in &spans {
        kept = kept
            .into_iter()
            .flat_map(|other| uncovered(other, start, end, lengths))
            .collect();
    }
    let mut all: Vec<(Item, Option<usize>)> = kept
        .into_iter()
        .map(|i| (i, None))
        .chain(new.into_iter().enumerate().map(|(k, i)| (i, Some(k))))
        .collect();
    all.sort_by(|a, b| a.0.position.total_cmp(&b.0.position));
    let mut places = vec![0; count];
    for (place, (_, k)) in all.iter().enumerate() {
        if let Some(k) = k {
            places[*k] = place;
        }
    }
    *items = all.into_iter().map(|(i, _)| i).collect();
    places
}

/// What is left of `item` once timeline `start..end` is covered: all of it, the part before, the
/// part after (from its place in the resource there), both, or nothing. Parts shorter than
/// [`MIN_ITEM_LENGTH`] are dropped.
fn uncovered(item: Item, start: f64, end: f64, lengths: &ItemLengths) -> Vec<Item> {
    let Some(item_end) = lengths.end_of(&item) else {
        return vec![item];
    };
    if item_end <= start + OVERLAP_MARGIN || item.position >= end - OVERLAP_MARGIN {
        return vec![item];
    }
    let mut parts = Vec::with_capacity(2);
    if start - item.position >= MIN_ITEM_LENGTH {
        parts.push(Item {
            end: Some(item.source_time(start)),
            ..item.clone()
        });
    }
    if item_end - end >= MIN_ITEM_LENGTH {
        parts.push(Item {
            position: end,
            start: item.source_time(end),
            ..item
        });
    }
    parts
}

/// Moves edge `edge` of `item` to timeline time `to`, for a resource `length` seconds long,
/// within `room` (where the items either side end and start); see [`Project::trim_item`].
fn trim(item: &mut Item, edge: Edge, to: f64, stretch: bool, length: f64, room: (f64, f64)) {
    if !to.is_finite() {
        return;
    }
    let (before, after) = room;
    let end = item.timeline_end(length);
    match (edge, stretch) {
        (Edge::Start, false) => {
            // Back to the resource's start, the timeline's, or the item before.
            let earliest = (item.position - item.start / item.rate)
                .max(0.0)
                .max(before);
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
            let latest = (item.position + (length - item.start) / item.rate)
                .min(after)
                .max(shortest);
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
                Edge::Start => (end - to).clamp(shortest, longest.min(end - before).max(shortest)),
                Edge::End => (to - item.position)
                    .clamp(shortest, longest.min(after - item.position).max(shortest)),
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

    /// The graph [`project`] places.
    const GRAPH: ResourceId = Project::FIRST_GRAPH;

    /// Graph tracks `g` and `h`, video track `v` (10 s) and audio tracks `a`, `b` (2 s each, of
    /// one file each), with `items` placed whole: the graph for 4 s on graph tracks.
    fn project(items: &[(&str, f64)]) -> Project {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        for name in ["g", "h"] {
            project.add_empty_track(TrackKind::Graph, name);
        }
        project.add_track(TrackKind::Video, "v", "v.mp4");
        for name in ["a", "b"] {
            project.add_track(TrackKind::Audio, name, format!("{name}.wav"));
        }
        for track in project.tracks_mut() {
            track.items.clear();
        }
        for &(name, position) in items {
            let id = resource_of(&project, name);
            project
                .place_resource(name, id, position, &lengths())
                .unwrap();
        }
        project
    }

    /// What track `name` of [`project`] plays.
    fn resource_of(project: &Project, name: &str) -> ResourceId {
        let file = match name {
            "g" | "h" => return GRAPH,
            "v" => "v.mp4".to_owned(),
            other => format!("{other}.wav"),
        };
        project
            .resources
            .iter()
            .find(|r| r.path().is_some_and(|p| p.to_str() == Some(file.as_str())))
            .unwrap()
            .id
    }

    /// The video (resource 2) lasts 10 s, the audio (3 and 4) 2 s; a graph is placed for 4 s.
    fn lengths() -> ItemLengths {
        let durations = (2..=4)
            .map(|id| (ResourceId(id), if id == 2 { 10.0 } else { 2.0 }))
            .collect();
        ItemLengths {
            durations,
            new_graph_length: 4.0,
        }
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

    /// Where each item of track `name` lies on the timeline.
    fn spans(project: &Project, name: &str) -> Vec<(f64, f64)> {
        let lengths = lengths();
        project
            .track(name)
            .unwrap()
            .items
            .iter()
            .map(|i| (i.position, lengths.end_of(i).unwrap()))
            .collect()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn at(track: &str, item: usize) -> ItemRef {
        ItemRef::new(track, item)
    }

    #[test]
    fn moving_several_items_stops_when_the_first_reaches_the_start() {
        let mut p = project(&[("a", 1.0), ("a", 5.0), ("b", 3.0)]);
        let selection = [at("a", 1), at("b", 0)];
        let moved = p.move_items(&selection, 1.0, 0, &lengths());
        assert_eq!(moved, selection);
        assert_eq!(items(&p, "a")[1].0, 6.0);
        let moved = p.move_items(&moved, -10.0, 0, &lengths());
        assert_eq!(moved, selection);
        assert_eq!(items(&p, "a")[1].0, 2.0);
        assert_eq!(items(&p, "b")[0].0, 0.0);
        // The unselected item stayed, trimmed where the moved one landed on it.
        assert_eq!(items(&p, "a")[0], (1.0, 0.0, Some(1.0), 1.0));
    }

    #[test]
    fn placing_media_over_items_trims_and_cuts_them() {
        let mut p = project(&[("v", 0.0)]);
        let clip = resource_of(&p, "v");
        // Lands inside: the item is cut in two, the tail playing on from where it was.
        assert_eq!(p.place_resource("v", clip, 3.0, &lengths()), Some(1));
        assert_eq!(spans(&p, "v"), [(0.0, 3.0), (3.0, 13.0)]);
        // At half speed the first lasts twice as long; one landing over the join trims what it
        // covers of each side.
        p.track_mut("v").unwrap().items[0].rate = 0.5;
        assert_eq!(spans(&p, "v")[0], (0.0, 6.0));
        p.move_items(&[at("v", 1)], 3.0, 0, &lengths());
        assert_eq!(p.place_resource("v", clip, 4.0, &lengths()), Some(1));
        assert_eq!(spans(&p, "v"), [(0.0, 4.0), (4.0, 14.0), (14.0, 16.0)]);
        // A cut follows the rate: 4 s of timeline at half speed is 2 s of the file.
        assert_eq!(items(&p, "v")[0], (0.0, 0.0, Some(2.0), 0.5));
        assert_eq!(items(&p, "v")[2], (14.0, 8.0, None, 1.0));
        // An item covered whole is gone.
        p.place_resource("v", clip, 13.0, &lengths());
        assert_eq!(spans(&p, "v"), [(0.0, 4.0), (4.0, 13.0), (13.0, 23.0)]);
    }

    #[test]
    fn placing_a_graph_over_items_trims_and_cuts_them() {
        let mut p = project(&[("g", 0.0), ("g", 4.0)]);
        let short = |length: f64| ItemLengths {
            new_graph_length: length,
            ..lengths()
        };
        // Lands across the join: both are trimmed.
        assert_eq!(p.place_resource("g", GRAPH, 3.0, &short(2.0)), Some(1));
        assert_eq!(spans(&p, "g"), [(0.0, 3.0), (3.0, 5.0), (5.0, 8.0)]);
        // The tail keeps its place in graph time: its start moved by what was covered.
        assert_eq!(items(&p, "g")[2].1, 1.0);
        // Inside one item: it is cut in two.
        p.place_resource("g", GRAPH, 6.0, &short(1.0));
        assert_eq!(
            spans(&p, "g"),
            [(0.0, 3.0), (3.0, 5.0), (5.0, 6.0), (6.0, 7.0), (7.0, 8.0)]
        );
    }

    #[test]
    fn placing_needs_a_track_of_the_resources_kind() {
        let mut p = project(&[]);
        let song = resource_of(&p, "a");
        assert!(p.place_resource("g", GRAPH, 1.0, &lengths()).is_some());
        assert!(
            p.place_resource("g", ResourceId(99), 0.0, &lengths())
                .is_none()
        );
        assert!(p.place_resource("nope", GRAPH, 0.0, &lengths()).is_none());
        assert!(p.place_resource("v", GRAPH, 0.0, &lengths()).is_none());
        assert!(p.place_resource("g", song, 0.0, &lengths()).is_none());
        assert!(p.place_resource("b", song, 0.0, &lengths()).is_some());
        // Until the renderer reads a file per item, a video or audio track plays one resource.
        let other = resource_of(&p, "b");
        assert!(p.place_resource("b", other, 3.0, &lengths()).is_none());
    }

    #[test]
    fn an_item_of_unknown_length_neither_trims_nor_is_trimmed() {
        let mut p = project(&[("a", 0.0)]);
        let song = resource_of(&p, "a");
        let unknown = ItemLengths::default();
        // The file not read yet: the new item can't say what it covers.
        assert_eq!(p.place_resource("a", song, 1.0, &unknown), Some(1));
        assert_eq!(items(&p, "a")[0], (0.0, 0.0, None, 1.0));
        // Nor can the old one: a known item landing on it leaves it be.
        let mut known = Item::whole(song, 0.0);
        known.end = Some(1.5);
        p.paste_items(&[("a".into(), known)], 0.5, &unknown);
        assert_eq!(items(&p, "a")[0], (0.0, 0.0, None, 1.0));
    }

    #[test]
    fn moving_media_onto_an_item_trims_it() {
        let mut p = project(&[("a", 0.0), ("a", 5.0)]);
        let moved = p.move_items(&[at("a", 1)], -4.0, 0, &lengths());
        assert_eq!(moved, [at("a", 1)]);
        assert_eq!(spans(&p, "a"), [(0.0, 1.0), (1.0, 3.0)]);
        // Moved inside a longer item, it cuts it in two.
        let mut p = project(&[("v", 0.0), ("v", 12.0)]);
        p.trim_item(&at("v", 1), Edge::End, 13.0, false, &lengths());
        let moved = p.move_items(&[at("v", 1)], -8.0, 0, &lengths());
        assert_eq!(moved, [at("v", 1)]);
        assert_eq!(spans(&p, "v"), [(0.0, 4.0), (4.0, 5.0), (5.0, 10.0)]);
        assert_eq!(items(&p, "v")[2].1, 5.0);
    }

    #[test]
    fn items_move_across_tracks_within_their_section() {
        let mut p = project(&[("g", 0.0), ("h", 6.0), ("a", 0.0)]);
        // A graph item one track down, onto what is there.
        let moved = p.move_items(&[at("g", 0)], 5.0, 1, &lengths());
        assert_eq!(moved, [at("h", 0)]);
        assert!(items(&p, "g").is_empty());
        assert_eq!(spans(&p, "h"), [(5.0, 9.0), (9.0, 10.0)]);
        // Audio between the audio tracks, back and forth.
        let moved = p.move_items(&[at("a", 0)], 0.0, 1, &lengths());
        assert_eq!(moved, [at("b", 0)]);
        let moved = p.move_items(&moved, 1.0, -1, &lengths());
        assert_eq!(moved, [at("a", 0)]);
        assert_eq!(spans(&p, "a"), [(1.0, 3.0)]);
    }

    #[test]
    fn a_move_across_sections_is_refused() {
        let mut p = project(&[("h", 0.0), ("v", 0.0), ("a", 0.0)]);
        let before = p.clone();
        // Down from the last graph track, up from the first audio one, off the bottom.
        for (item, rows) in [(at("h", 0), 1), (at("a", 0), -1), (at("a", 0), 2)] {
            assert_eq!(
                p.move_items(std::slice::from_ref(&item), 1.0, rows, &lengths()),
                [item]
            );
            assert_eq!(p, before);
        }
        // A group moves as one: one of its items leaving the section stops all of them.
        p.group_items(&[at("h", 0), at("a", 0)]);
        let grouped = p.clone();
        p.move_items(&[at("a", 0)], 1.0, 1, &lengths());
        assert_eq!(p, grouped);
        // Onto an audio track playing another file, for now.
        let mut p = project(&[("a", 0.0), ("b", 3.0)]);
        let before = p.clone();
        p.move_items(&[at("a", 0)], 0.0, 1, &lengths());
        assert_eq!(p, before);
    }

    #[test]
    fn trimming_moves_an_edge_over_the_resource_within_its_ends() {
        let mut p = project(&[("a", 1.0)]);
        let a = at("a", 0);
        p.trim_item(&a, Edge::Start, 1.5, false, &lengths());
        assert_eq!(items(&p, "a"), [(1.5, 0.5, None, 1.0)]);
        // Not before the resource's start.
        p.trim_item(&a, Edge::Start, 0.0, false, &lengths());
        assert_eq!(items(&p, "a"), [(1.0, 0.0, None, 1.0)]);
        p.trim_item(&a, Edge::End, 2.5, false, &lengths());
        assert_eq!(items(&p, "a"), [(1.0, 0.0, Some(1.5), 1.0)]);
        // Not past the resource's end, nor shorter than the minimum.
        p.trim_item(&a, Edge::End, 9.0, false, &lengths());
        assert_eq!(items(&p, "a"), [(1.0, 0.0, Some(2.0), 1.0)]);
        p.trim_item(&a, Edge::End, 0.0, false, &lengths());
        assert!(close(items(&p, "a")[0].2.unwrap(), MIN_ITEM_LENGTH));
    }

    #[test]
    fn a_trim_stops_at_the_neighbour() {
        let mut p = project(&[("v", 0.0), ("v", 12.0)]);
        // The second item from 4 s into the file, at 8 s: the file starts at 4 s.
        p.trim_item(&at("v", 1), Edge::Start, 16.0, false, &lengths());
        p.move_items(&[at("v", 1)], -8.0, 0, &lengths());
        assert_eq!(spans(&p, "v"), [(0.0, 8.0), (8.0, 14.0)]);
        // The file would let its start go back to 4; the item before stops it at 8.
        p.trim_item(&at("v", 1), Edge::Start, 2.0, false, &lengths());
        assert_eq!(spans(&p, "v")[1], (8.0, 14.0));
        // The file would let the first end at 10; the item after stops it at 8.
        p.trim_item(&at("v", 0), Edge::End, 5.0, false, &lengths());
        p.trim_item(&at("v", 0), Edge::End, 20.0, false, &lengths());
        assert_eq!(spans(&p, "v")[0], (0.0, 8.0));
        // A stretch stops there too.
        p.trim_item(&at("v", 0), Edge::End, 5.0, false, &lengths());
        p.trim_item(&at("v", 0), Edge::End, 30.0, true, &lengths());
        assert_eq!(spans(&p, "v")[0], (0.0, 8.0));
        assert!(close(items(&p, "v")[0].3, 5.0 / 8.0));
        // So does a graph item, which has no end of its own.
        let mut p = project(&[("g", 2.0), ("g", 7.0)]);
        p.trim_item(&at("g", 0), Edge::End, 20.0, false, &lengths());
        assert_eq!(spans(&p, "g")[0], (2.0, 7.0));
    }

    #[test]
    fn trimming_a_graph_items_left_edge_keeps_what_follows() {
        let mut p = project(&[("g", 2.0)]);
        p.trim_item(&at("g", 0), Edge::Start, 3.0, false, &lengths());
        assert_eq!(items(&p, "g"), [(3.0, 1.0, Some(4.0), 1.0)]);
        // The graph's own time can't go below zero: it has nothing before it.
        p.trim_item(&at("g", 0), Edge::Start, 0.0, false, &lengths());
        assert_eq!(items(&p, "g"), [(2.0, 0.0, Some(4.0), 1.0)]);
    }

    #[test]
    fn stretching_changes_the_rate_and_keeps_the_far_edge() {
        let mut p = project(&[("a", 1.0)]);
        let a = at("a", 0);
        // Dragging the end from 3 to 5: twice as long, half the speed.
        p.trim_item(&a, Edge::End, 5.0, true, &lengths());
        assert_eq!(items(&p, "a"), [(1.0, 0.0, Some(2.0), 0.5)]);
        // Dragging the start from 1 to 4: the end stays at 5, 1 s for 2 s of resource.
        p.trim_item(&a, Edge::Start, 4.0, true, &lengths());
        let (position, _, _, rate) = items(&p, "a")[0];
        assert!(close(position, 4.0) && close(rate, 2.0));
        // Within the rate range.
        p.trim_item(&a, Edge::End, 4.0 + 1e-6, true, &lengths());
        assert!(close(items(&p, "a")[0].3, *RATE_RANGE.end()));
    }

    #[test]
    fn graph_items_have_no_stretch() {
        let mut p = project(&[("g", 1.0)]);
        // Alt+drag on a graph item trims it, at the graph's own speed.
        p.trim_item(&at("g", 0), Edge::End, 3.0, true, &lengths());
        assert_eq!(items(&p, "g"), [(1.0, 0.0, Some(2.0), 1.0)]);
        // A file can't set one either.
        p.track_mut("g").unwrap().items[0].rate = 3.0;
        p.sanitize_items();
        assert_eq!(items(&p, "g")[0].3, 1.0);
    }

    #[test]
    fn grouped_items_move_together_until_ungrouped() {
        let mut p = project(&[("v", 0.0), ("a", 0.0), ("a", 5.0), ("b", 1.0)]);
        let pair = [at("v", 0), at("a", 0)];
        assert_eq!(p.group_items(&pair[..1]), None);
        let group = p.group_items(&pair).unwrap();
        assert_eq!(p.with_grouped(&pair[1..]), [at("a", 0), at("v", 0)]);
        p.move_items(&[at("a", 0)], 1.0, 0, &lengths());
        assert_eq!(items(&p, "v")[0].0, 1.0);
        assert_eq!(items(&p, "a")[1].0, 5.0);
        assert_eq!(items(&p, "b")[0].0, 1.0);
        // Grouping an item of a group with another takes in the whole group.
        let bigger = p.group_items(&[at("a", 0), at("b", 0)]).unwrap();
        assert_ne!(bigger, group);
        assert_eq!(p.with_grouped(&[at("b", 0)]).len(), 3);
        assert!(p.ungroup_items(&[at("v", 0)]));
        assert!(!p.ungroup_items(&[at("v", 0)]));
        p.move_items(&[at("a", 0)], 1.0, 0, &lengths());
        assert_eq!(items(&p, "v")[0].0, 1.0);
    }

    #[test]
    fn trimming_takes_the_matching_edges_of_grouped_items() {
        let mut p = project(&[("v", 0.0), ("a", 0.0), ("b", 0.5)]);
        p.group_items(&[at("v", 0), at("a", 0), at("b", 0)]);
        p.trim_item(&at("a", 0), Edge::Start, 0.25, false, &lengths());
        assert_eq!(items(&p, "v")[0].0, 0.25);
        assert_eq!(items(&p, "a")[0].0, 0.25);
        // b's start wasn't at the same time.
        assert_eq!(items(&p, "b")[0].0, 0.5);
    }

    #[test]
    fn splitting_cuts_grouped_items_and_keeps_the_first_halves_selected() {
        let mut p = project(&[("v", 0.0), ("a", 0.0), ("a", 4.0)]);
        p.group_items(&[at("v", 0), at("a", 0)]);
        let selection = p.split_items(&[at("a", 0), at("a", 1)], 1.5, &lengths());
        assert_eq!(
            items(&p, "a"),
            [
                (0.0, 0.0, Some(1.5), 1.0),
                (1.5, 1.5, None, 1.0),
                (4.0, 0.0, None, 1.0)
            ]
        );
        // Item 1 (at 4.0, not cut) moved to index 2.
        assert_eq!(selection, [at("a", 0), at("a", 2)]);
        // The grouped video was cut with it, and each side is a group of its own.
        assert_eq!(
            items(&p, "v"),
            [(0.0, 0.0, Some(1.5), 1.0), (1.5, 1.5, None, 1.0)]
        );
        assert_eq!(p.with_grouped(&[at("a", 1)]), [at("a", 1), at("v", 1)]);
        assert_eq!(p.with_grouped(&[at("v", 0)]), [at("a", 0), at("v", 0)]);
        // A split at an edge, or outside, does nothing.
        p.split_items(&[at("a", 1)], 1.5, &lengths());
        assert_eq!(items(&p, "a").len(), 3);
    }

    #[test]
    fn a_split_follows_the_rate_and_keeps_graph_time() {
        let mut p = project(&[("a", 1.0), ("g", 1.0)]);
        p.track_mut("a").unwrap().items[0].rate = 2.0;
        p.split_items(&[at("a", 0), at("g", 0)], 1.5, &lengths());
        assert_eq!(
            items(&p, "a"),
            [(1.0, 0.0, Some(1.0), 2.0), (1.5, 1.0, None, 2.0)]
        );
        assert_eq!(
            items(&p, "g"),
            [(1.0, 0.0, Some(0.5), 1.0), (1.5, 0.5, Some(4.0), 1.0)]
        );
    }

    #[test]
    fn deleting_takes_grouped_items_and_copies_paste_at_a_time() {
        let mut p = project(&[("v", 0.0), ("a", 0.0), ("a", 4.0), ("b", 1.0)]);
        p.group_items(&[at("v", 0), at("a", 1)]);
        let copied = p.copy_items(&[at("a", 1)]);
        // The video is in a's second item's group.
        assert_eq!(copied.len(), 2);
        let pasted = p.paste_items(&copied, 20.0, &lengths());
        assert_eq!(pasted, [at("a", 2), at("v", 1)]);
        // The earliest copied item (the video at 0) lands at 20; a's keeps its offset.
        assert_eq!(items(&p, "v")[1].0, 20.0);
        assert_eq!(items(&p, "a")[2].0, 24.0);
        // The pasted pair is a group of its own.
        assert_eq!(p.with_grouped(&[at("v", 1)]).len(), 2);
        assert_eq!(p.with_grouped(&[at("a", 1)]).len(), 2);
        assert_ne!(
            p.track("v").unwrap().items[0].group,
            p.track("v").unwrap().items[1].group
        );
        p.delete_items(&[at("a", 1)]);
        assert_eq!(items(&p, "a").len(), 2);
        assert_eq!(items(&p, "v"), [(20.0, 0.0, None, 1.0)]);
        assert_eq!(items(&p, "b").len(), 1);
        // A track that is gone takes nothing.
        let gone = Item::whole(GRAPH, 0.0);
        assert!(
            p.paste_items(&[("gone".into(), gone)], 0.0, &lengths())
                .is_empty()
        );
    }

    #[test]
    fn pasting_trims_what_it_lands_on() {
        let mut p = project(&[("v", 0.0), ("g", 0.0)]);
        // A copy keeps its trim.
        let mut copied = p.copy_items(&[at("g", 0)]);
        copied[0].1.start = 0.5;
        copied[0].1.end = Some(1.5);
        assert_eq!(p.paste_items(&copied, 2.0, &lengths()), [at("g", 1)]);
        assert_eq!(spans(&p, "g"), [(0.0, 2.0), (2.0, 3.0), (3.0, 4.0)]);
        assert_eq!(items(&p, "g")[1], (2.0, 0.5, Some(1.5), 1.0));
        // Media too.
        let copied = p.copy_items(&[at("v", 0)]);
        assert_eq!(p.paste_items(&copied, 4.0, &lengths()), [at("v", 1)]);
        assert_eq!(spans(&p, "v"), [(0.0, 4.0), (4.0, 14.0)]);
    }

    #[test]
    fn graph_items_group_copy_and_paste_with_media() {
        let mut p = project(&[("g", 0.0), ("v", 1.0), ("a", 1.0)]);
        p.group_items(&[at("g", 0), at("v", 0)]);
        assert_eq!(p.with_grouped(&[at("v", 0)]), [at("g", 0), at("v", 0)]);
        // They move, split and copy together.
        p.move_items(&[at("v", 0)], 1.0, 0, &lengths());
        assert_eq!((items(&p, "g")[0].0, items(&p, "v")[0].0), (1.0, 2.0));
        p.split_items(&[at("g", 0)], 3.0, &lengths());
        assert_eq!((items(&p, "g").len(), items(&p, "v").len()), (2, 2));
        let copied = p.copy_items(&[at("g", 1)]);
        assert_eq!(copied.len(), 2);
        let pasted = p.paste_items(&copied, 20.0, &lengths());
        assert_eq!(pasted, [at("g", 2), at("v", 2)]);
        assert_eq!(p.with_grouped(&pasted[..1]), pasted);
        // Deleting one takes the other.
        p.delete_items(&[at("v", 2)]);
        assert_eq!((items(&p, "g").len(), items(&p, "v").len()), (2, 2));
    }

    #[test]
    fn removing_a_resource_leaves_its_tracks() {
        let mut p = project(&[("g", 0.0), ("v", 0.0), ("a", 0.0)]);
        let other = p.add_graph("B", None);
        p.place_resource("g", other, 5.0, &lengths());
        p.group_items(&[at("g", 1), at("a", 0)]);
        assert_eq!(p.resource_users(other), ["g"]);
        assert_eq!(p.resource_item_count(GRAPH), 1);
        assert_eq!(p.remove_resource(other), ["g"]);
        assert_eq!(items(&p, "g").len(), 1);
        assert_eq!(p.track("g").unwrap().items[0].resource, GRAPH);
        // Its group is gone with it.
        assert_eq!(p.with_grouped(&[at("a", 0)]).len(), 1);
        let video = resource_of(&p, "v");
        assert_eq!(p.remove_resource(video), ["v"]);
        assert_eq!(p.tracks.len(), 5);
        assert!(items(&p, "v").is_empty());
        // A resource nothing places is on no track.
        let unplaced = p.add_graph("C", None);
        assert!(p.remove_resource(unplaced).is_empty());
    }

    #[test]
    fn tracks_round_trip_and_overlaps_are_repaired() {
        let mut p = project(&[("g", 1.0), ("a", 0.0)]);
        let json = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        // A file with overlapping graph items, out of order: put in order and trimmed.
        let mut early = p.track("g").unwrap().items[0].clone();
        early.position = 0.0;
        p.track_mut("g").unwrap().items.push(early);
        // A file's item without an out point has no length to trim it by: it stays.
        let song = resource_of(&p, "a");
        p.track_mut("a").unwrap().items.push(Item::whole(song, 1.0));
        p.sanitize_items();
        assert_eq!(spans(&p, "g"), [(0.0, 1.0), (1.0, 5.0)]);
        assert_eq!(items(&p, "a").len(), 2);
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
