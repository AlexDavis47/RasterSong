//! Resources: the media a project uses, each one stream of a linked file. Tracks play them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Project, ProjectTrack};
use crate::timeline::TrackKind;

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
        self.resource(track.resource)
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
            .chain(self.tracks().map(|t| t.resource.0 + 1))
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
            .filter(|t| t.resource == id)
            .map(|t| t.name.clone())
            .collect()
    }

    /// Adds a track playing the whole resource from `position` seconds, named after it (made
    /// unique among tracks), below the other tracks of its kind, and returns its name. `None`
    /// if there is no such resource.
    pub fn add_track_for(&mut self, id: ResourceId, position: f64) -> Option<String> {
        let resource = self.resource(id)?;
        let kind = resource.kind.track_kind();
        let name = self.unique_name(&resource.name);
        let mut track = ProjectTrack::new(name.clone(), id);
        track.items[0].position = position.max(0.0);
        match kind {
            TrackKind::Video => self.video_tracks.push(track),
            TrackKind::Audio => self.audio_tracks.push(track),
        }
        Some(name)
    }

    /// Adds a track called `name` playing the whole file at `path` (its best stream of `kind`),
    /// through a resource for that stream, and returns it.
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
        let tracks = match kind {
            TrackKind::Video => &mut self.video_tracks,
            TrackKind::Audio => &mut self.audio_tracks,
        };
        tracks.push(ProjectTrack::new(name.into(), id));
        tracks.last_mut().unwrap()
    }

    /// Removes a resource and every track that plays it, returning the removed tracks' names.
    pub fn remove_resource(&mut self, id: ResourceId) -> Vec<String> {
        let users = self.resource_users(id);
        self.video_tracks.retain(|t| t.resource != id);
        self.audio_tracks.retain(|t| t.resource != id);
        self.resources.retain(|r| r.id != id);
        users
    }

    /// Renames a resource, keeping the name unique among resources. Its tracks keep their
    /// names, since graphs select tracks by name. False if there is no such resource or the
    /// name is blank.
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
