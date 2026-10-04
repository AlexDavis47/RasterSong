//! Project files: the media, timeline and graph the user is working on.

use std::path::{Path, PathBuf};

use rastersong_graph::GraphDesc;
use serde::{Deserialize, Serialize};

use crate::{AudioTrackSpec, DEFAULT_AUDIO_TRACK};

pub const PROJECT_VERSION: u32 = 2;

/// Conventional extension for project files.
pub const PROJECT_EXTENSION: &str = "rastersong";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub version: u32,
    /// Media paths are saved relative to the project file when possible, so a project folder can
    /// be moved or shared; they are always absolute in memory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_tracks: Vec<ProjectTrack>,
    pub graph: GraphDesc,
}

/// An audio track on the timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrack {
    /// The name audio input nodes select the track by.
    pub name: String,
    pub path: PathBuf,
    /// Seconds the track starts after the video (before it, if negative).
    #[serde(default)]
    pub offset: f64,
    /// Playback volume, 0 to 1. Doesn't affect rendering.
    #[serde(default = "full_volume")]
    pub volume: f32,
    /// Silent in playback. Doesn't affect rendering.
    #[serde(default)]
    pub muted: bool,
}

fn full_volume() -> f32 {
    1.0
}

impl ProjectTrack {
    pub fn new(name: String, path: PathBuf) -> Self {
        Self {
            name,
            path,
            offset: 0.0,
            volume: 1.0,
            muted: false,
        }
    }
}

/// The version 1 format: a single audio file.
#[derive(Deserialize)]
struct ProjectV1 {
    video: Option<PathBuf>,
    audio: Option<PathBuf>,
    #[serde(default)]
    audio_offset: f64,
    graph: GraphDesc,
}

impl Project {
    pub fn new(graph: GraphDesc) -> Self {
        Self {
            version: PROJECT_VERSION,
            video: None,
            audio_tracks: Vec::new(),
            graph,
        }
    }

    /// What the engine renders with.
    pub fn track_specs(&self) -> Vec<AudioTrackSpec> {
        self.audio_tracks
            .iter()
            .map(|t| AudioTrackSpec {
                name: t.name.clone(),
                path: t.path.clone(),
                offset: t.offset,
            })
            .collect()
    }

    /// A name no track has yet: `audio`, then `audio_2`, `audio_3`, …
    pub fn unused_track_name(&self) -> String {
        (1..)
            .map(|n| {
                if n == 1 {
                    DEFAULT_AUDIO_TRACK.to_owned()
                } else {
                    format!("{DEFAULT_AUDIO_TRACK}_{n}")
                }
            })
            .find(|name| !self.audio_tracks.iter().any(|t| &t.name == name))
            .unwrap()
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        let json = std::fs::read_to_string(path).map_err(|e| error(&e))?;
        let value: serde_json::Value = serde_json::from_str(&json)
            .map_err(|e| error(&format!("invalid project file: {e}")))?;
        let invalid = |e: serde_json::Error| error(&format!("invalid project file: {e}"));
        let mut project = match value.get("version").and_then(serde_json::Value::as_u64) {
            Some(1) => {
                let v1: ProjectV1 = serde_json::from_value(value).map_err(invalid)?;
                Self {
                    version: PROJECT_VERSION,
                    video: v1.video,
                    audio_tracks: v1
                        .audio
                        .map(|path| ProjectTrack {
                            offset: v1.audio_offset,
                            ..ProjectTrack::new(DEFAULT_AUDIO_TRACK.to_owned(), path)
                        })
                        .into_iter()
                        .collect(),
                    graph: v1.graph,
                }
            }
            Some(v) if v == u64::from(PROJECT_VERSION) => {
                serde_json::from_value(value).map_err(invalid)?
            }
            other => {
                return Err(error(&format!(
                    "unsupported project version {other:?} (expected {PROJECT_VERSION})"
                )));
            }
        };
        let dir = path.parent().unwrap_or(Path::new(""));
        let media = project
            .video
            .iter_mut()
            .chain(project.audio_tracks.iter_mut().map(|t| &mut t.path));
        for media in media {
            if media.is_relative() {
                *media = dir.join(&*media);
            }
        }
        Ok(project)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let dir = path.parent().unwrap_or(Path::new(""));
        let mut saved = self.clone();
        let media = saved
            .video
            .iter_mut()
            .chain(saved.audio_tracks.iter_mut().map(|t| &mut t.path));
        for media in media {
            if let Ok(relative) = media.strip_prefix(dir) {
                *media = relative.to_path_buf();
            }
        }
        let json = serde_json::to_string_pretty(&saved).expect("projects always serialize");
        std::fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph() -> GraphDesc {
        GraphDesc::from_json(
            r#"{ "version": 1, "nodes": [ { "id": "v", "type": "video_input", "position": [10, 20] } ] }"#,
        )
        .unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rastersong-{name}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("media")).unwrap();
        dir
    }

    #[test]
    fn round_trips_with_paths_relative_to_the_project() {
        let dir = temp_dir("project");
        let path = dir.join("song.rastersong");

        let mut project = Project::new(graph());
        project.video = Some(dir.join("media/clip.mp4"));
        // Absolute and outside the project folder on every platform.
        let elsewhere = std::env::current_dir().unwrap().join("elsewhere-song.wav");
        project.audio_tracks.push(ProjectTrack {
            offset: -1.25,
            volume: 0.5,
            muted: true,
            ..ProjectTrack::new("audio".into(), elsewhere)
        });
        project.audio_tracks.push(ProjectTrack::new(
            "drums".into(),
            dir.join("media/drums.wav"),
        ));
        project.save(&path).unwrap();

        let json = std::fs::read_to_string(&path).unwrap();
        assert!(
            json.contains(r#""video": "media"#),
            "inside the folder: relative\n{json}"
        );
        assert!(
            json.contains("elsewhere-song"),
            "outside the folder: unchanged"
        );

        assert_eq!(Project::load(&path).unwrap(), project);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn migrates_version_1() {
        let dir = temp_dir("project-v1");
        let path = dir.join("old.rastersong");
        std::fs::write(
            &path,
            r#"{ "version": 1, "video": "media/clip.mp4", "audio": "media/song.wav", "audio_offset": 2.5,
                 "graph": { "version": 1, "nodes": [] } }"#,
        )
        .unwrap();
        let project = Project::load(&path).unwrap();
        assert_eq!(project.version, PROJECT_VERSION);
        assert_eq!(project.audio_tracks.len(), 1);
        let track = &project.audio_tracks[0];
        assert_eq!(
            (track.name.as_str(), track.offset, track.volume),
            ("audio", 2.5, 1.0)
        );
        assert_eq!(track.path, dir.join("media/song.wav"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_unknown_versions() {
        let dir = temp_dir("project-v99");
        let path = dir.join("future.rastersong");
        let mut project = Project::new(graph());
        project.version = 99;
        std::fs::write(&path, serde_json::to_string(&project).unwrap()).unwrap();
        assert!(Project::load(&path).unwrap_err().contains("version"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn names_new_tracks_uniquely() {
        let mut project = Project::new(graph());
        assert_eq!(project.unused_track_name(), "audio");
        project
            .audio_tracks
            .push(ProjectTrack::new("audio".into(), "a.wav".into()));
        assert_eq!(project.unused_track_name(), "audio_2");
    }
}
