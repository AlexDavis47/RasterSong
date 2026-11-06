//! Resources: what a project's tracks play, listed in the Resources panel. A media resource is
//! one stream of a linked file; a graph resource is a graph, kept in the project.

use std::path::{Path, PathBuf};

use rastersong_graph::GraphDesc;
use rastersong_media::AudioOptions;
use serde::{Deserialize, Serialize};

use super::{ItemLengths, ItemRef, Project, ProjectTrack};
use crate::timeline::{Item, LabelColor, TrackKind};

/// Identifies a resource within its project: what items point at. Unique among the project's
/// resources (and those its items name), not across time: a new resource can take a removed
/// one's id once nothing points at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceId(pub u32);

/// A linked file's stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaFile {
    /// The linked file. Saved relative to the project file when it is inside the project's
    /// folder, so the folder can be moved or shared; always absolute in memory.
    pub path: PathBuf,
    /// The file's stream, by its index in the file. `None` is the file's best stream of the
    /// resource's kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<usize>,
}

/// What a resource is, saved with its kind as `"kind"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceContent {
    Video(MediaFile),
    Audio(MediaFile),
    Graph { graph: GraphDesc },
}

/// One thing the project's tracks can play, listed in the Resources panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub id: ResourceId,
    /// What the panel shows, and what tracks made from it are named after. Unique among
    /// resources of every kind.
    pub name: String,
    /// The label colour the panel and the timeline draw it and its items in, to tell resources
    /// apart. `None` is its track kind's own colour. It changes nothing in the render.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<LabelColor>,
    pub content: ResourceContent,
}

impl MediaFile {
    /// How its audio is decoded: its stream, at its own rate and channels.
    pub fn audio_options(&self) -> AudioOptions {
        AudioOptions {
            stream: self.stream,
            ..AudioOptions::default()
        }
    }
}

impl ResourceContent {
    /// What it is, and so which tracks can play it.
    pub fn kind(&self) -> TrackKind {
        match self {
            Self::Video(_) => TrackKind::Video,
            Self::Audio(_) => TrackKind::Audio,
            Self::Graph { .. } => TrackKind::Graph,
        }
    }

    /// The linked file's stream, for a media resource.
    pub fn file(&self) -> Option<&MediaFile> {
        match self {
            Self::Video(file) | Self::Audio(file) => Some(file),
            Self::Graph { .. } => None,
        }
    }

    /// The graph, for a graph resource.
    pub fn graph(&self) -> Option<&GraphDesc> {
        match self {
            Self::Graph { graph } => Some(graph),
            _ => None,
        }
    }
}

impl Resource {
    /// What it is, and so which tracks can play it.
    pub fn kind(&self) -> TrackKind {
        self.content.kind()
    }

    /// The linked file's stream, for a media resource.
    pub fn file(&self) -> Option<&MediaFile> {
        self.content.file()
    }

    pub fn file_mut(&mut self) -> Option<&mut MediaFile> {
        match &mut self.content {
            ResourceContent::Video(file) | ResourceContent::Audio(file) => Some(file),
            ResourceContent::Graph { .. } => None,
        }
    }

    /// The linked file, for a media resource.
    pub fn path(&self) -> Option<&Path> {
        self.file().map(|f| f.path.as_path())
    }

    /// The linked file's stream, for a media resource (`None` there is its best stream).
    pub fn stream(&self) -> Option<usize> {
        self.file().and_then(|f| f.stream)
    }

    /// The graph, for a graph resource.
    pub fn graph(&self) -> Option<&GraphDesc> {
        self.content.graph()
    }

    pub fn graph_mut(&mut self) -> Option<&mut GraphDesc> {
        match &mut self.content {
            ResourceContent::Graph { graph } => Some(graph),
            _ => None,
        }
    }
}

impl Project {
    /// The resource with `id`.
    pub fn resource(&self, id: ResourceId) -> Option<&Resource> {
        self.resources.iter().find(|r| r.id == id)
    }

    pub fn resource_mut(&mut self, id: ResourceId) -> Option<&mut Resource> {
        self.resources.iter_mut().find(|r| r.id == id)
    }

    /// Adds a resource for `stream` of the file at `path` (its best stream of `kind` for
    /// `None`), named `name` (made unique among resources), and returns its id. A resource of
    /// the same stream already in the project is returned instead of adding another.
    pub fn add_resource(
        &mut self,
        kind: TrackKind,
        name: &str,
        path: impl Into<PathBuf>,
        stream: Option<usize>,
    ) -> ResourceId {
        let file = MediaFile {
            path: path.into(),
            stream,
        };
        if let Some(known) = self
            .resources
            .iter()
            .find(|r| r.kind() == kind && r.file() == Some(&file))
        {
            return known.id;
        }
        let content = match kind {
            TrackKind::Video => ResourceContent::Video(file),
            TrackKind::Audio => ResourceContent::Audio(file),
            TrackKind::Graph => panic!("a graph resource has no file"),
        };
        self.push_resource(name, content)
    }

    /// Adds a resource holding `content`, named `name` (made unique among resources), and
    /// returns its id.
    pub(super) fn push_resource(&mut self, name: &str, content: ResourceContent) -> ResourceId {
        let id = ResourceId(self.next_resource_id());
        let name = self.unique_resource_name(name);
        self.resources.push(Resource {
            id,
            name,
            color: None,
            content,
        });
        id
    }

    /// One past the highest id a resource has or an item names.
    fn next_resource_id(&self) -> u32 {
        let named = self.tracks().flat_map(|t| &t.items).map(|i| i.resource);
        self.resources
            .iter()
            .map(|r| r.id)
            .chain(named)
            .map(|id| id.0 + 1)
            .max()
            .unwrap_or(1)
    }

    /// The first `name`, `name_2`, `name_3`, … that no resource has.
    pub fn unique_resource_name(&self, name: &str) -> String {
        let name = name.trim();
        let name = if name.is_empty() { "resource" } else { name };
        (1..)
            .map(|n| {
                if n == 1 {
                    name.to_owned()
                } else {
                    format!("{name}_{n}")
                }
            })
            .find(|candidate| !self.resources.iter().any(|r| r.name == *candidate))
            .unwrap()
    }

    /// The names of the tracks with an item of `id`.
    pub fn resource_users(&self, id: ResourceId) -> Vec<String> {
        self.tracks()
            .filter(|t| t.items.iter().any(|i| i.resource == id))
            .map(|t| t.name.clone())
            .collect()
    }

    /// How many items play `id`, on every track.
    pub fn resource_item_count(&self, id: ResourceId) -> usize {
        self.tracks()
            .flat_map(|t| &t.items)
            .filter(|i| i.resource == id)
            .count()
    }

    /// Adds a track with one item of resource `id` from `position` seconds (see
    /// [`Self::place_resource`]), named after it (made unique among tracks), at the bottom of its
    /// section, and returns its name. `None` if there is no such resource.
    pub fn add_track_for(
        &mut self,
        id: ResourceId,
        position: f64,
        lengths: &ItemLengths,
    ) -> Option<String> {
        self.add_track_for_at(id, position, None, lengths)
    }

    /// As [`Self::add_track_for`], but before the track now at index `slot` when that is in the
    /// new track's section (or just past its last).
    pub fn add_track_for_at(
        &mut self,
        id: ResourceId,
        position: f64,
        slot: Option<usize>,
        lengths: &ItemLengths,
    ) -> Option<String> {
        let resource = self.resource(id)?;
        let (kind, name) = (resource.kind(), resource.name.clone());
        let name = self.add_empty_track_at(kind, &name, slot);
        self.place_resource(&name, id, position, lengths);
        Some(name)
    }

    /// Points every resource that reads the file at `old` (the streams of one file) at `new`.
    /// Returns how many moved.
    pub fn relocate_resource(&mut self, id: ResourceId, new: impl Into<PathBuf>) -> usize {
        let Some(old) = self
            .resource(id)
            .and_then(Resource::path)
            .map(Path::to_owned)
        else {
            return 0;
        };
        let new = new.into();
        let mut moved = 0;
        for file in self.resources.iter_mut().filter_map(Resource::file_mut) {
            if file.path == old {
                file.path = new.clone();
                moved += 1;
            }
        }
        moved
    }

    /// Adds an empty track of `kind`, named `name`, `name_2`, … (unique among tracks), at the
    /// bottom of its section, and returns its name.
    pub fn add_empty_track(&mut self, kind: TrackKind, name: &str) -> String {
        self.add_empty_track_at(kind, name, None)
    }

    /// As [`Self::add_empty_track`], but before the track now at index `slot` when that is in
    /// its section (or just past its last).
    pub fn add_empty_track_at(
        &mut self,
        kind: TrackKind,
        name: &str,
        slot: Option<usize>,
    ) -> String {
        let name = self.unique_name(name);
        self.insert_new_track_at(ProjectTrack::new(name.clone(), kind), slot);
        name
    }

    /// Whether [`Self::place_resource`] would put resource `id` on track `track`: a track of its
    /// kind, whatever else it plays.
    pub fn can_place_resource(&self, track: &str, id: ResourceId) -> bool {
        let (Some(resource), Some(index)) = (self.resource(id), self.track_index(track)) else {
            return false;
        };
        self.tracks[index].kind == resource.kind()
    }

    /// Resource `id` and the other resources from the same file (its other streams), `id`
    /// first, then in the Resources panel's order.
    pub fn siblings(&self, id: ResourceId) -> Vec<ResourceId> {
        let Some(resource) = self.resource(id) else {
            return Vec::new();
        };
        let path = resource.path();
        std::iter::once(id)
            .chain(
                self.resources
                    .iter()
                    .filter(|r| r.id != id && path.is_some() && r.path() == path)
                    .map(|r| r.id),
            )
            .collect()
    }

    /// The resources of a drop of `ids` that each start a group: with `linked` a resource from
    /// the file of an earlier one (its sound, dragged with its picture) comes with that one
    /// instead. Each comes with the resources it places: every resource of its file when
    /// `linked`, else itself.
    fn drop_groups(&self, ids: &[ResourceId], linked: bool) -> Vec<Vec<ResourceId>> {
        let mut covered: Vec<ResourceId> = Vec::new();
        let mut groups = Vec::new();
        for &id in ids {
            if covered.contains(&id) || self.resource(id).is_none() {
                continue;
            }
            let group = if linked { self.siblings(id) } else { vec![id] };
            covered.extend(&group);
            groups.push(group);
        }
        groups
    }

    /// Where [`Self::place_drop`] puts each resource of a drop of `ids` (several dragged
    /// together, stems say), one list per file: the track it lands on, or `None` for a new
    /// track at the bottom of its section. Each resource goes on track `hovered` when that is in
    /// its section and can take it (the first resource of that kind only), else on the first
    /// track of its section that plays nothing else (empty, or only that resource) and that no
    /// other resource of the drop takes.
    pub fn plan_drop(
        &self,
        ids: &[ResourceId],
        hovered: Option<&str>,
        linked: bool,
    ) -> Vec<Vec<(ResourceId, Option<String>)>> {
        let mut taken: Vec<String> = Vec::new();
        let mut hovered = hovered;
        self.drop_groups(ids, linked)
            .into_iter()
            .map(|group| {
                let mut plan = Vec::new();
                for res in group {
                    let fits = |name: &str| {
                        !taken.iter().any(|t| t == name) && self.can_place_resource(name, res)
                    };
                    let free = |t: &ProjectTrack| t.items.iter().all(|i| i.resource == res);
                    let target = hovered.filter(|&h| fits(h)).map(str::to_owned).or_else(|| {
                        self.tracks
                            .iter()
                            .find(|t| fits(&t.name) && free(t))
                            .map(|t| t.name.clone())
                    });
                    if let Some(name) = &target {
                        taken.push(name.clone());
                        if Some(name.as_str()) == hovered {
                            hovered = None;
                        }
                    }
                    plan.push((res, target));
                }
                plan
            })
            .collect()
    }

    /// Places a drop of `ids` from `position` seconds as [`Self::plan_drop`] says, grouping
    /// each file's items. Returns the items placed, the first resource's first.
    pub fn place_drop(
        &mut self,
        ids: &[ResourceId],
        hovered: Option<&str>,
        position: f64,
        linked: bool,
        lengths: &ItemLengths,
    ) -> Vec<ItemRef> {
        let mut all = Vec::new();
        for group in self.plan_drop(ids, hovered, linked) {
            let mut placed = Vec::new();
            for (res, target) in group {
                let landed = target.and_then(|name| {
                    let item = self.place_resource(&name, res, position, lengths)?;
                    Some(ItemRef::new(name, item))
                });
                let landed = landed.or_else(|| {
                    let name = self.add_track_for(res, position, lengths)?;
                    Some(ItemRef::new(name, 0))
                });
                placed.extend(landed);
            }
            self.group_items(&placed);
            all.extend(placed);
        }
        all
    }

    /// [`Self::place_drop`] of resource `id` alone.
    pub fn place_linked(
        &mut self,
        id: ResourceId,
        hovered: Option<&str>,
        position: f64,
        linked: bool,
        lengths: &ItemLengths,
    ) -> Vec<ItemRef> {
        self.place_drop(&[id], hovered, position, linked, lengths)
    }

    /// Places resource `id` from `position` seconds on new tracks inserted before the track now
    /// at index `slot`, with every other resource of its file when `linked`, and groups what it
    /// placed. A resource whose section doesn't hold `slot` gets its new track at the bottom of
    /// its own section. Returns the items placed, `id`'s first.
    pub fn place_linked_at(
        &mut self,
        id: ResourceId,
        slot: usize,
        position: f64,
        linked: bool,
        lengths: &ItemLengths,
    ) -> Vec<ItemRef> {
        self.place_many_at(&[id], slot, position, linked, lengths)
    }

    /// As [`Self::place_linked_at`] for several resources dragged together (stems, say): each
    /// goes on its own new track, one after the other from `slot`, and each resource's file is
    /// grouped by itself. A resource already placed as another's sibling is placed once.
    pub fn place_many_at(
        &mut self,
        ids: &[ResourceId],
        slot: usize,
        position: f64,
        linked: bool,
        lengths: &ItemLengths,
    ) -> Vec<ItemRef> {
        let mut slot = slot;
        let mut all = Vec::new();
        for group in self.drop_groups(ids, linked) {
            let mut placed = Vec::new();
            for res in group {
                let Some(name) = self.add_track_for_at(res, position, Some(slot), lengths) else {
                    continue;
                };
                // Everything from the new track on shifts down one.
                if self.track_index(&name).is_some_and(|at| at <= slot) {
                    slot += 1;
                }
                placed.push(ItemRef::new(name, 0));
            }
            self.group_items(&placed);
            all.extend(placed);
        }
        all
    }

    /// Adds a track called `name` at the bottom of its section, playing the whole file at `path`
    /// (its best stream of `kind`) through a resource for that stream, and returns it.
    pub fn add_track(
        &mut self,
        kind: TrackKind,
        name: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> &mut ProjectTrack {
        let path = path.into();
        let resource_name = resource_name_for(&path, kind);
        let id = self.add_resource(kind, &resource_name, path, None);
        let mut track = ProjectTrack::new(name.into(), kind);
        track.items.push(Item::whole(id, 0.0));
        let at = self.insert_new_track(track);
        &mut self.tracks[at]
    }

    /// Removes a resource and its items, from every track. The tracks stay. Returns the names of
    /// the tracks it was on.
    pub fn remove_resource(&mut self, id: ResourceId) -> Vec<String> {
        let users = self.resource_users(id);
        for track in &mut self.tracks {
            track.items.retain(|i| i.resource != id);
        }
        self.tidy_groups();
        self.resources.retain(|r| r.id != id);
        users
    }

    /// Adds a copy of resource `id` named after it, with its colour, and returns the copy's
    /// id. Only kinds that [duplicate](TrackKind::duplicates) can be copied.
    pub fn duplicate_resource(&mut self, id: ResourceId) -> Option<ResourceId> {
        let resource = self.resource(id).filter(|r| r.kind().duplicates())?;
        let name = format!("{} copy", resource.name);
        let (content, color) = (resource.content.clone(), resource.color);
        let copy = self.push_resource(&name, content);
        self.set_resource_color(copy, color);
        Some(copy)
    }

    /// Renames a resource, keeping the name unique among resources. Its tracks keep their
    /// names. False if there is no such resource or the name is blank.
    pub fn rename_resource(&mut self, id: ResourceId, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || self.resource(id).is_none() {
            return false;
        }
        // Its own name doesn't count as taken.
        let old = std::mem::take(&mut self.resource_mut(id).unwrap().name);
        let name = if old == name {
            old
        } else {
            self.unique_resource_name(name)
        };
        self.resource_mut(id).unwrap().name = name;
        true
    }

    /// Gives resource `id` the label colour `color` (`None` for its track kind's own). Returns
    /// whether it changed.
    pub fn set_resource_color(&mut self, id: ResourceId, color: Option<LabelColor>) -> bool {
        match self.resource_mut(id) {
            Some(resource) if resource.color != color => {
                resource.color = color;
                true
            }
            _ => false,
        }
    }
}

/// A name for a resource of the file at `path`: the file name for video, and the file name
/// without its extension for audio, so a video's own sound doesn't share its video's name.
pub fn resource_name_for(path: &Path, kind: TrackKind) -> String {
    let name = match kind {
        TrackKind::Audio => path.file_stem(),
        _ => path.file_name(),
    };
    name.map(|s| s.to_string_lossy().trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| match kind {
            TrackKind::Audio => crate::DEFAULT_AUDIO_TRACK.to_owned(),
            _ => crate::VIDEO_SOURCE.to_owned(),
        })
}
