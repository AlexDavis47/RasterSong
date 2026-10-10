//! Resources: the media a project uses, each one stream of a linked file. Tracks play them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{ItemRef, Project, ProjectTrack};
use crate::timeline::{Item, TrackKind};

/// Identifies a resource within its project: what tracks point at. Unique among the project's
/// resources (and those its tracks name), not across time: a new resource can take a removed
/// one's id once nothing points at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceId(pub u32);

/// What a resource holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// One video stream.
    Video,
    /// One audio stream, any channel count.
    Audio,
}

impl ResourceKind {
    /// The kind of track that plays it.
    pub fn track_kind(self) -> TrackKind {
        match self {
            Self::Video => TrackKind::Video,
            Self::Audio => TrackKind::Audio,
        }
    }
}

/// One stream of a linked file, listed in the Resources panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub id: ResourceId,
    /// What the panel shows, and what tracks made from it are named after.
    pub name: String,
    pub kind: ResourceKind,
    /// The linked file. Saved relative to the project file when it is inside the project's
    /// folder, so the folder can be moved or shared; always absolute in memory.
    pub path: PathBuf,
    /// The file's stream, by its index in the file. `None` is the file's best stream of the
    /// resource's kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<usize>,
}

impl Project {
    /// The resource with `id`.
    pub fn resource(&self, id: ResourceId) -> Option<&Resource> {
        self.resources.iter().find(|r| r.id == id)
    }

    pub fn resource_mut(&mut self, id: ResourceId) -> Option<&mut Resource> {
        self.resources.iter_mut().find(|r| r.id == id)
    }

    /// The resource `track` plays.
    pub fn track_resource(&self, track: &ProjectTrack) -> Option<&Resource> {
        self.resource(track.resource?)
    }

    /// The file `track` plays.
    pub fn track_path(&self, track: &ProjectTrack) -> Option<&Path> {
        self.track_resource(track).map(|r| r.path.as_path())
    }

    /// Adds a resource for `stream` of the file at `path` (its best stream of `kind` for
    /// `None`), named `name` (made unique among resources), and returns its id. A resource of
    /// the same stream already in the project is returned instead of adding another.
    pub fn add_resource(
        &mut self,
        kind: ResourceKind,
        name: &str,
        path: impl Into<PathBuf>,
        stream: Option<usize>,
    ) -> ResourceId {
        let path = path.into();
        if let Some(known) = self
            .resources
            .iter()
            .find(|r| r.kind == kind && r.path == path && r.stream == stream)
        {
            return known.id;
        }
        let id = ResourceId(self.next_resource_id());
        let name = self.unique_resource_name(name);
        self.resources.push(Resource {
            id,
            name,
            kind,
            path,
            stream,
        });
        id
    }

    fn next_resource_id(&self) -> u32 {
        self.resources
            .iter()
            .map(|r| r.id.0 + 1)
            .chain(self.tracks().filter_map(|t| t.resource).map(|id| id.0 + 1))
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

    /// The names of the tracks that play `id`.
    pub fn resource_users(&self, id: ResourceId) -> Vec<String> {
        self.tracks()
            .filter(|t| t.resource == Some(id))
            .map(|t| t.name.clone())
            .collect()
    }

    /// Adds a track playing the whole resource from `position` seconds, named after it (made
    /// unique among tracks), at the bottom of its section, and returns its name. `None` if there
    /// is no such resource.
    pub fn add_track_for(&mut self, id: ResourceId, position: f64) -> Option<String> {
        self.add_track_for_at(id, position, None)
    }

    /// As [`Self::add_track_for`], but before the track now at index `slot` when that is in the
    /// new track's section (or just past its last).
    pub fn add_track_for_at(
        &mut self,
        id: ResourceId,
        position: f64,
        slot: Option<usize>,
    ) -> Option<String> {
        let resource = self.resource(id)?;
        let kind = resource.kind.track_kind();
        let name = self.unique_name(&resource.name);
        let mut track = ProjectTrack::new(name.clone(), kind, id);
        track.items[0].position = position.max(0.0);
        self.insert_new_track_at(track, slot);
        Some(name)
    }

    /// Points every resource that reads the file at `old` (the streams of one file) at `new`.
    /// Returns how many moved.
    pub fn relocate_resource(&mut self, id: ResourceId, new: impl Into<PathBuf>) -> usize {
        let Some(old) = self.resource(id).map(|r| r.path.clone()) else {
            return 0;
        };
        let new = new.into();
        let mut moved = 0;
        for resource in self.resources.iter_mut().filter(|r| r.path == old) {
            resource.path = new.clone();
            moved += 1;
        }
        moved
    }

    /// Adds an empty track of `kind`, named `name`, `name_2`, … (unique among tracks), at the
    /// bottom of its section, and returns its name. It takes the first resource of its kind
    /// dropped on it.
    pub fn add_empty_track(&mut self, kind: TrackKind, name: &str) -> String {
        let name = self.unique_name(name);
        self.insert_new_track(ProjectTrack::empty(name.clone(), kind));
        name
    }

    /// Whether [`Self::place_resource`] would put resource `id` on track `track`: an empty track
    /// of its kind, or a track already playing it.
    pub fn can_place_resource(&self, track: &str, id: ResourceId) -> bool {
        let Some(resource) = self.resource(id) else {
            return false;
        };
        self.track(track).is_some_and(|t| {
            t.kind == resource.kind.track_kind() && t.resource.is_none_or(|r| r == id)
        })
    }

    /// Puts the whole resource `id` on track `track` from `position` seconds. An empty track
    /// takes the resource; a track holds items of one resource only, so any other resource is
    /// refused (false), and so is a resource of the other kind.
    pub fn place_resource(&mut self, track: &str, id: ResourceId, position: f64) -> bool {
        if !self.can_place_resource(track, id) {
            return false;
        }
        let Some(track) = self.tracks.iter_mut().find(|t| t.name == track) else {
            return false;
        };
        track.resource = Some(id);
        let mut item = Item::whole(0.0);
        item.position = position.max(0.0);
        track.items.push(item);
        true
    }

    /// Resource `id` and the other resources from the same file (its other streams), `id`
    /// first, then in the Resources panel's order.
    pub fn siblings(&self, id: ResourceId) -> Vec<ResourceId> {
        let Some(path) = self.resource(id).map(|r| &r.path) else {
            return Vec::new();
        };
        std::iter::once(id)
            .chain(
                self.resources
                    .iter()
                    .filter(|r| r.id != id && r.path == *path)
                    .map(|r| r.id),
            )
            .collect()
    }

    /// Where [`Self::place_linked`] puts each resource: the track it lands on, or `None` for a
    /// new track at the bottom of its section. Each resource goes on track `hovered` when that
    /// is in its section and can take it (the first resource of that kind only), else on the
    /// first track of its section that can and that no other resource here takes.
    pub fn plan_linked(
        &self,
        id: ResourceId,
        hovered: Option<&str>,
        linked: bool,
    ) -> Vec<(ResourceId, Option<String>)> {
        let resources = if linked {
            self.siblings(id)
        } else {
            self.resource(id).map(|r| r.id).into_iter().collect()
        };
        let mut taken: Vec<String> = Vec::new();
        let mut plan = Vec::new();
        for res in resources {
            let fits =
                |name: &str| !taken.iter().any(|t| t == name) && self.can_place_resource(name, res);
            let target = hovered.filter(|&h| fits(h)).map(str::to_owned).or_else(|| {
                self.tracks
                    .iter()
                    .find(|t| fits(&t.name))
                    .map(|t| t.name.clone())
            });
            if let Some(name) = &target {
                taken.push(name.clone());
            }
            plan.push((res, target));
        }
        plan
    }

    /// Places resource `id` from `position` seconds as [`Self::plan_linked`] says, with every
    /// other resource of its file when `linked` (so dropping a video places its sound too), and
    /// groups what it placed. Returns the items placed, `id`'s first.
    pub fn place_linked(
        &mut self,
        id: ResourceId,
        hovered: Option<&str>,
        position: f64,
        linked: bool,
    ) -> Vec<ItemRef> {
        let mut placed = Vec::new();
        for (res, target) in self.plan_linked(id, hovered, linked) {
            let name = match target {
                Some(name) if self.place_resource(&name, res, position) => name,
                _ => match self.add_track_for(res, position) {
                    Some(name) => name,
                    None => continue,
                },
            };
            let item = self.track(&name).map_or(0, |t| t.items.len() - 1);
            placed.push(ItemRef::new(name, item));
        }
        self.group_items(&placed);
        placed
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
    ) -> Vec<ItemRef> {
        let resources = if linked {
            self.siblings(id)
        } else {
            self.resource(id).map(|r| r.id).into_iter().collect()
        };
        let mut slot = slot;
        let mut placed = Vec::new();
        for res in resources {
            let Some(name) = self.add_track_for_at(res, position, Some(slot)) else {
                continue;
            };
            // Everything from the new track on shifts down one.
            if self.track_index(&name).is_some_and(|at| at <= slot) {
                slot += 1;
            }
            placed.push(ItemRef::new(name, 0));
        }
        self.group_items(&placed);
        placed
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
        let resource_kind = match kind {
            TrackKind::Video => ResourceKind::Video,
            TrackKind::Audio => ResourceKind::Audio,
        };
        let resource_name = resource_name_for(&path, resource_kind);
        let id = self.add_resource(resource_kind, &resource_name, path, None);
        let at = self.insert_new_track(ProjectTrack::new(name.into(), kind, id));
        &mut self.tracks[at]
    }

    /// Removes a resource and every track that plays it, returning the removed tracks' names.
    pub fn remove_resource(&mut self, id: ResourceId) -> Vec<String> {
        let users = self.resource_users(id);
        while let Some(i) = self.tracks.iter().position(|t| t.resource == Some(id)) {
            self.remove_track(i);
        }
        self.resources.retain(|r| r.id != id);
        users
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
}

/// A name for a resource of the file at `path`: the file name for video, and the file name
/// without its extension for audio, so a video's own sound doesn't share its video's name.
pub fn resource_name_for(path: &Path, kind: ResourceKind) -> String {
    let name = match kind {
        ResourceKind::Video => path.file_name(),
        ResourceKind::Audio => path.file_stem(),
    };
    name.map(|s| s.to_string_lossy().trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| match kind {
            ResourceKind::Video => crate::VIDEO_SOURCE.to_owned(),
            ResourceKind::Audio => crate::DEFAULT_AUDIO_TRACK.to_owned(),
        })
}
