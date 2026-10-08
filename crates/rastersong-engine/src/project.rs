//! Project files: the timeline, its media and the graph the user is working on.

use std::path::{Path, PathBuf};

use rastersong_graph::nodes::{AUDIO_OUTPUT, BUS_PARAM, DEFAULT_BUS};
use rastersong_graph::{GraphDesc, ParamValue, Tempo, audio_output_bus};
use rastersong_lang::tr_args;
use serde::{Deserialize, Serialize};

use crate::DEFAULT_AUDIO_TRACK;
use crate::timeline::{Bus, Item, Timebase, Timeline, TrackKind, TrackSpec};

/// The project file format version. Like the graph format it stays 0 until 1.0: files change
/// freely, with no migrations, and projects saved by another version are rejected.
pub const PROJECT_VERSION: u32 = 0;

/// Conventional extension for project files.
pub const PROJECT_EXTENSION: &str = "rastersong";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub version: u32,
    /// The project's size and frame rate. `None` until set: the first video track's are used,
    /// or [`Timebase::DEFAULT`] without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timebase: Option<Timebase>,
    /// Video tracks, top first. Media paths are saved relative to the project file when
    /// possible, so a project folder can be moved or shared; they are always absolute in memory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub video_tracks: Vec<ProjectTrack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_tracks: Vec<ProjectTrack>,
    /// The output buses, master first: audio tracks are summed into them in the track mix, and
    /// each Audio Output writes to one. Never empty; Main, stereo, by default.
    #[serde(default = "default_buses", skip_serializing_if = "is_default_buses")]
    pub buses: Vec<Bus>,
    pub graph: GraphDesc,
    /// The loop region on the timeline, if one has been made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_region: Option<LoopRegion>,
    /// The tempo beat and bar units follow.
    #[serde(default)]
    pub tempo: Tempo,
    /// Whether the timeline ruler shows time or bars and beats (tempo).
    #[serde(default)]
    pub timeline_mode: TimelineMode,
    /// Skips the whole graph in the preview: it shows and plays the track mix.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bypass_graph: bool,
    /// Sample rate of the sound a graph with an Audio Output renders, in the preview and export.
    #[serde(
        default = "default_audio_rate",
        skip_serializing_if = "is_default_audio_rate"
    )]
    pub audio_rate: u32,
    /// The most frames the engine pre-renders and discards after a seek so stateful nodes have
    /// history. It limits only that background pre-render, never what a node does.
    #[serde(
        default = "default_max_warmup_frames",
        skip_serializing_if = "is_default_max_warmup_frames"
    )]
    pub max_warmup_frames: u32,
    /// How many times a second the inspection popup asks for a new frame while the playhead
    /// moves. Each asks the engine to render one frame for the popup, so a project that is slow
    /// to render wants this low.
    #[serde(
        default = "default_inspect_rate",
        skip_serializing_if = "is_default_inspect_rate"
    )]
    pub inspect_rate: f64,
}

/// The default for [`Project::inspect_rate`], in updates a second.
pub const DEFAULT_INSPECT_RATE: f64 = 5.0;

/// The least and most [`Project::inspect_rate`] can be set to.
pub const INSPECT_RATE_RANGE: std::ops::RangeInclusive<f64> = 1.0..=30.0;

fn default_inspect_rate() -> f64 {
    DEFAULT_INSPECT_RATE
}

fn is_default_inspect_rate(rate: &f64) -> bool {
    *rate == DEFAULT_INSPECT_RATE
}

/// The default for [`Project::max_warmup_frames`]: about four seconds of 30 fps video.
pub const DEFAULT_MAX_WARMUP_FRAMES: u32 = 120;

/// The most [`Project::max_warmup_frames`] can be set to.
pub const MAX_WARMUP_FRAMES_LIMIT: u32 = 9999;

fn default_max_warmup_frames() -> u32 {
    DEFAULT_MAX_WARMUP_FRAMES
}

fn is_default_max_warmup_frames(frames: &u32) -> bool {
    *frames == DEFAULT_MAX_WARMUP_FRAMES
}

fn default_buses() -> Vec<Bus> {
    vec![Bus::main()]
}

fn is_default_buses(buses: &Vec<Bus>) -> bool {
    *buses == default_buses()
}

fn is_main_bus(bus: &str) -> bool {
    bus == DEFAULT_BUS
}

fn main_bus() -> String {
    DEFAULT_BUS.to_owned()
}

fn default_audio_rate() -> u32 {
    crate::DEFAULT_AUDIO_RATE
}

fn is_default_audio_rate(rate: &u32) -> bool {
    *rate == crate::DEFAULT_AUDIO_RATE
}

/// How the timeline ruler and grid are labelled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineMode {
    #[default]
    Time,
    Tempo,
}

/// A stretch of the timeline that playback repeats, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoopRegion {
    pub start: f64,
    pub end: f64,
    /// Whether playback loops. The region stays when looping is off.
    pub enabled: bool,
}

impl LoopRegion {
    /// The region in whole frames, `start..end`, if it covers at least one frame.
    pub fn frames(&self, frame_rate: f64) -> Option<std::ops::Range<usize>> {
        let start = (self.start * frame_rate).round().max(0.0) as usize;
        let end = (self.end * frame_rate).round().max(0.0) as usize;
        (end > start).then_some(start..end)
    }
}

/// A track on the timeline: items of one media file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrack {
    /// The name the graph's input nodes select the track by. Unique among all tracks.
    pub name: String,
    pub path: PathBuf,
    /// Where the file plays on the timeline.
    #[serde(default = "whole")]
    pub items: Vec<Item>,
    /// Level in the track mix, 0 to 1. Graphs read the track as it is.
    #[serde(default = "full_volume")]
    pub volume: f32,
    /// Left out of the track mix. Graphs still read the track.
    #[serde(default)]
    pub muted: bool,
    /// The output bus an audio track is summed into in the track mix.
    #[serde(default = "main_bus", skip_serializing_if = "is_main_bus")]
    pub bus: String,
}

fn full_volume() -> f32 {
    1.0
}

fn whole() -> Vec<Item> {
    vec![Item::whole(0.0)]
}

impl ProjectTrack {
    /// A track playing the whole file from the start of the timeline.
    pub fn new(name: String, path: PathBuf) -> Self {
        Self {
            name,
            path,
            items: whole(),
            volume: 1.0,
            muted: false,
            bus: main_bus(),
        }
    }

    /// Where time 0 of the file falls on the timeline, by the first item: negative when the
    /// item starts partway into the file. 0 for a track with no items.
    pub fn offset(&self) -> f64 {
        self.items
            .first()
            .map_or(0.0, |item| item.position - item.start / item.rate)
    }

    /// Moves the first item so time 0 of the file falls `offset` seconds into the timeline.
    /// Items can't start before the timeline does, so a negative offset starts the item partway
    /// into the file instead. Its out point stays where it was in the file.
    pub fn set_offset(&mut self, offset: f64) {
        if let Some(item) = self.items.first_mut() {
            item.position = offset.max(0.0);
            item.start = (-offset).max(0.0) * item.rate;
            if let Some(end) = &mut item.end {
                *end = end.max(item.start);
            }
        }
    }

    fn spec(&self, kind: TrackKind) -> TrackSpec {
        TrackSpec {
            name: self.name.clone(),
            kind,
            path: self.path.clone(),
            items: self.items.clone(),
            bus: self.bus.clone(),
            gain: if self.muted { 0.0 } else { self.volume },
        }
    }
}

impl Project {
    pub fn new(graph: GraphDesc) -> Self {
        Self {
            version: PROJECT_VERSION,
            timebase: None,
            video_tracks: Vec::new(),
            audio_tracks: Vec::new(),
            buses: default_buses(),
            graph,
            loop_region: None,
            tempo: Tempo::default(),
            timeline_mode: TimelineMode::default(),
            bypass_graph: false,
            audio_rate: crate::DEFAULT_AUDIO_RATE,
            max_warmup_frames: DEFAULT_MAX_WARMUP_FRAMES,
            inspect_rate: DEFAULT_INSPECT_RATE,
        }
    }

    /// What the engine renders: the timebase and every track.
    pub fn timeline(&self) -> Timeline {
        Timeline {
            timebase: self.timebase,
            tracks: self
                .video_tracks
                .iter()
                .map(|t| t.spec(TrackKind::Video))
                .chain(self.audio_tracks.iter().map(|t| t.spec(TrackKind::Audio)))
                .collect(),
            buses: self.buses.clone(),
        }
    }

    /// The master bus: the first, which the preview plays and the export writes.
    pub fn master_bus(&self) -> &str {
        self.buses.first().map_or(DEFAULT_BUS, |b| b.name.as_str())
    }

    pub fn has_bus(&self, name: &str) -> bool {
        self.buses.iter().any(|b| b.name == name)
    }

    /// The first `Bus 2`, `Bus 3`, … no bus has, for a new bus.
    pub fn unused_bus_name(&self) -> String {
        (2..)
            .map(|n| tr_args("project.bus.new_name", &[("n", &n.to_string())]))
            .find(|name| !self.has_bus(name))
            .unwrap()
    }

    /// What removing bus `name` affects: the audio tracks routed to it, and the ids of the
    /// graph's Audio Outputs that write to it.
    pub fn bus_users(&self, name: &str) -> (Vec<String>, Vec<String>) {
        let tracks = self
            .audio_tracks
            .iter()
            .filter(|t| t.bus == name)
            .map(|t| t.name.clone())
            .collect();
        let outputs = self
            .graph
            .nodes
            .iter()
            .filter(|n| n.kind == AUDIO_OUTPUT && audio_output_bus(n) == name)
            .map(|n| n.id.clone())
            .collect();
        (tracks, outputs)
    }

    /// Removes bus `name`, unless it is the last. Its tracks move to the master bus (the first
    /// left); Audio Outputs writing to it are left as they are, writing to a bus that isn't
    /// there, so they aren't rendered until pointed at another.
    pub fn remove_bus(&mut self, name: &str) -> bool {
        if self.buses.len() <= 1 || !self.has_bus(name) {
            return false;
        }
        self.buses.retain(|b| b.name != name);
        let master = self.master_bus().to_owned();
        for track in &mut self.audio_tracks {
            if track.bus == name {
                track.bus = master.clone();
            }
        }
        true
    }

    /// Renames bus `old` to `new`, with the tracks routed to it and the Audio Outputs writing
    /// to it. Refused when `new` is empty or another bus has it.
    pub fn rename_bus(&mut self, old: &str, new: &str) -> bool {
        let new = new.trim();
        if new.is_empty() || old == new || self.has_bus(new) {
            return false;
        }
        let Some(bus) = self.buses.iter_mut().find(|b| b.name == old) else {
            return false;
        };
        bus.name = new.to_owned();
        for track in &mut self.audio_tracks {
            if track.bus == old {
                track.bus = new.to_owned();
            }
        }
        for node in &mut self.graph.nodes {
            if node.kind == AUDIO_OUTPUT && audio_output_bus(node) == old {
                node.params
                    .insert(BUS_PARAM.to_owned(), ParamValue::Text(new.to_owned()));
            }
        }
        true
    }

    /// Every track, video first.
    pub fn tracks(&self) -> impl Iterator<Item = &ProjectTrack> {
        self.video_tracks.iter().chain(&self.audio_tracks)
    }

    fn tracks_mut(&mut self) -> impl Iterator<Item = &mut ProjectTrack> {
        self.video_tracks.iter_mut().chain(&mut self.audio_tracks)
    }

    /// Whether a track, of either kind, is called `name`.
    pub fn has_track(&self, name: &str) -> bool {
        self.tracks().any(|t| t.name == name)
    }

    /// The first `name`, `name_2`, `name_3`, … that no track has.
    fn unique_name(&self, name: &str) -> String {
        (1..)
            .map(|n| {
                if n == 1 {
                    name.to_owned()
                } else {
                    format!("{name}_{n}")
                }
            })
            .find(|name| !self.has_track(name))
            .unwrap()
    }

    /// A name no track has yet: `audio`, then `audio_2`, `audio_3`, …
    pub fn unused_track_name(&self) -> String {
        self.unique_name(DEFAULT_AUDIO_TRACK)
    }

    /// A name for an audio track of the file at `path`: its file name without the extension,
    /// made unique with `_2`, `_3`, … if another track already has it.
    pub fn track_name_for(&self, path: &Path) -> String {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_AUDIO_TRACK.to_owned());
        self.unique_name(&stem)
    }

    /// A name for a video track of the file at `path`: its file name, made unique the same
    /// way. The extension stays, so the video's own sound track (named without it) doesn't
    /// clash.
    pub fn video_track_name_for(&self, path: &Path) -> String {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| crate::VIDEO_SOURCE.to_owned());
        self.unique_name(&name)
    }

    /// The video: the first video track.
    pub fn video(&self) -> Option<&ProjectTrack> {
        self.video_tracks.first()
    }

    /// The video's file.
    pub fn video_path(&self) -> Option<&Path> {
        self.video().map(|t| t.path.as_path())
    }

    /// What the video is called: its track's name.
    pub fn video_display_name(&self) -> Option<String> {
        self.video().map(|t| t.name.clone())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        let json = std::fs::read_to_string(path).map_err(|e| error(&e))?;
        let value: serde_json::Value = serde_json::from_str(&json).map_err(|e| {
            error(&tr_args(
                "error.project.invalid",
                &[("error", &e.to_string())],
            ))
        })?;
        let invalid = |e: serde_json::Error| {
            error(&tr_args(
                "error.project.invalid",
                &[("error", &e.to_string())],
            ))
        };
        let version = value.get("version").and_then(serde_json::Value::as_u64);
        if version != Some(u64::from(PROJECT_VERSION)) {
            return Err(error(&tr_args(
                "error.project.version",
                &[
                    ("version", &format!("{version:?}")),
                    ("supported", &PROJECT_VERSION.to_string()),
                ],
            )));
        }
        let mut project: Self = serde_json::from_value(value).map_err(invalid)?;
        project.tempo = project.tempo.sanitized();
        project.max_warmup_frames = project.max_warmup_frames.min(MAX_WARMUP_FRAMES_LIMIT);
        project.inspect_rate = if project.inspect_rate.is_finite() {
            project
                .inspect_rate
                .clamp(*INSPECT_RATE_RANGE.start(), *INSPECT_RATE_RANGE.end())
        } else {
            DEFAULT_INSPECT_RATE
        };
        project.graph.upgrade();
        project.buses = sanitized_buses(std::mem::take(&mut project.buses));
        let dir = path.parent().unwrap_or(Path::new(""));
        for track in project.tracks_mut() {
            if track.path.is_relative() {
                track.path = dir.join(&track.path);
            }
            for item in &mut track.items {
                *item = item.sanitized();
            }
        }
        Ok(project)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let dir = path.parent().unwrap_or(Path::new(""));
        let mut saved = self.clone();
        for track in saved.tracks_mut() {
            if let Ok(relative) = track.path.strip_prefix(dir) {
                track.path = relative.to_path_buf();
            }
        }
        let json = serde_json::to_string_pretty(&saved).expect("projects always serialize");
        std::fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// `buses` with channel counts in range, nameless and repeated names dropped, and Main when
/// nothing is left.
fn sanitized_buses(buses: Vec<Bus>) -> Vec<Bus> {
    let mut kept: Vec<Bus> = Vec::new();
    for bus in buses {
        let bus = Bus {
            name: bus.name.trim().to_owned(),
            ..bus.sanitized()
        };
        if !bus.name.is_empty() && !kept.iter().any(|b| b.name == bus.name) {
            kept.push(bus);
        }
    }
    if kept.is_empty() {
        kept.push(Bus::main());
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::MAX_BUS_CHANNELS;

    fn graph() -> GraphDesc {
        GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [ { "id": "v", "type": "video_input", "position": [10, 20] } ] }"#,
        )
        .unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rastersong-{name}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("media")).unwrap();
        dir
    }

    #[test]
    fn tracks_of_both_kinds_share_one_set_of_names() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        assert_eq!(project.video_display_name(), None);
        let video = PathBuf::from("clips/take 3.mp4");
        let name = project.video_track_name_for(&video);
        assert_eq!(name, "take 3.mp4");
        project
            .video_tracks
            .push(ProjectTrack::new(name, video.clone()));
        assert_eq!(project.video_display_name().as_deref(), Some("take 3.mp4"));
        // The video's own sound is named without the extension, so it doesn't clash.
        assert_eq!(project.track_name_for(&video), "take 3");
        assert_eq!(project.video_track_name_for(&video), "take 3.mp4_2");
    }

    #[test]
    fn buses_route_tracks_and_audio_outputs_and_follow_renames_and_removals() {
        let mut project = Project::new(
            GraphDesc::from_json(
                r#"{ "version": 0, "nodes": [
                    { "id": "sound", "type": "audio_output" },
                    { "id": "stems", "type": "audio_output", "params": { "bus": "Stems" } } ] }"#,
            )
            .unwrap(),
        );
        assert_eq!(project.buses, [Bus::main()]);
        assert_eq!(project.unused_bus_name(), "Bus 2");
        project.buses.push(Bus {
            name: "Stems".into(),
            channels: 1,
        });
        project
            .audio_tracks
            .push(ProjectTrack::new("kick".into(), "kick.wav".into()));
        project.audio_tracks[0].bus = "Stems".into();
        project.audio_tracks[0].volume = 0.5;
        let timeline = project.timeline();
        assert_eq!(timeline.master(), Bus::main());
        assert_eq!(timeline.tracks_on("Stems").count(), 1);
        assert_eq!(timeline.tracks[0].gain, 0.5);
        assert_eq!(
            project.bus_users("Stems"),
            (vec!["kick".to_owned()], vec!["stems".to_owned()])
        );

        // Renaming follows the tracks and outputs; Main's output had no bus set and gets one.
        assert!(!project.rename_bus("Stems", "Main"));
        assert!(project.rename_bus("Stems", "Drums"));
        assert!(project.rename_bus("Main", "Music"));
        assert_eq!(project.audio_tracks[0].bus, "Drums");
        let bus_of = |p: &Project, id: &str| {
            audio_output_bus(p.graph.nodes.iter().find(|n| n.id == id).unwrap()).to_owned()
        };
        assert_eq!(bus_of(&project, "sound"), "Music");
        assert_eq!(bus_of(&project, "stems"), "Drums");

        // Removing moves its tracks to the master and leaves its outputs pointing nowhere. The
        // last bus stays.
        assert!(project.remove_bus("Drums"));
        assert_eq!(project.audio_tracks[0].bus, "Music");
        assert_eq!(bus_of(&project, "stems"), "Drums");
        assert!(!project.remove_bus("Music"));
    }

    #[test]
    fn buses_are_saved_only_when_changed_and_sanitized_on_load() {
        let dir = temp_dir("buses");
        let path = dir.join("buses.rastersong");
        let mut project = Project::new(graph());
        project
            .audio_tracks
            .push(ProjectTrack::new("song".into(), "song.wav".into()));
        project.save(&path).unwrap();
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(!json.contains("buses") && !json.contains("\"bus\""));
        project.buses.push(Bus {
            name: " Stems ".into(),
            channels: 99,
        });
        project.buses.push(Bus {
            name: "Main".into(),
            channels: 1,
        });
        project.audio_tracks[0].bus = "Stems".into();
        project.save(&path).unwrap();
        let loaded = Project::load(&path).unwrap();
        assert_eq!(
            loaded.buses,
            [
                Bus::main(),
                Bus {
                    name: "Stems".into(),
                    channels: MAX_BUS_CHANNELS
                }
            ]
        );
        assert_eq!(loaded.audio_tracks[0].bus, "Stems");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn offsets_move_the_first_item_and_never_before_the_start() {
        let mut track = ProjectTrack::new("a".into(), "a.wav".into());
        assert_eq!(track.offset(), 0.0);
        track.set_offset(1.5);
        assert_eq!((track.items[0].position, track.items[0].start), (1.5, 0.0));
        assert_eq!(track.offset(), 1.5);
        // Earlier than the timeline: the item starts partway into the file instead.
        track.set_offset(-2.0);
        assert_eq!((track.items[0].position, track.items[0].start), (0.0, 2.0));
        assert_eq!(track.offset(), -2.0);
    }

    #[test]
    fn the_timeline_lists_video_tracks_first() {
        let mut project = Project::new(graph());
        project
            .audio_tracks
            .push(ProjectTrack::new("song".into(), "song.wav".into()));
        project
            .video_tracks
            .push(ProjectTrack::new("clip.mp4".into(), "clip.mp4".into()));
        let timeline = project.timeline();
        assert_eq!(timeline.timebase, None);
        let kinds: Vec<_> = timeline
            .tracks
            .iter()
            .map(|t| (t.name.as_str(), t.kind))
            .collect();
        assert_eq!(
            kinds,
            [("clip.mp4", TrackKind::Video), ("song", TrackKind::Audio)]
        );
        assert_eq!(
            timeline.first_video().map(|t| t.path.clone()),
            Some(PathBuf::from("clip.mp4"))
        );
    }

    #[test]
    fn tracks_are_named_after_their_files() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        let name = project.track_name_for(Path::new("music/Drum Loop.wav"));
        assert_eq!(name, "Drum Loop");
        project.audio_tracks.push(ProjectTrack::new(
            name,
            PathBuf::from("music/Drum Loop.wav"),
        ));
        assert_eq!(
            project.track_name_for(Path::new("other/Drum Loop.mp3")),
            "Drum Loop_2"
        );
    }

    #[test]
    fn round_trips_with_paths_relative_to_the_project() {
        let dir = temp_dir("project");
        let path = dir.join("song.rastersong");

        let mut project = Project::new(graph());
        project.timebase = Some(Timebase {
            width: 640,
            height: 360,
            frame_rate: rastersong_media::Rational::new(30_000, 1001),
        });
        project
            .video_tracks
            .push(ProjectTrack::new("clip".into(), dir.join("media/clip.mp4")));
        // Absolute and outside the project folder on every platform.
        let elsewhere = std::env::current_dir().unwrap().join("elsewhere-song.wav");
        project.audio_tracks.push(ProjectTrack {
            items: vec![
                Item {
                    start: 1.25,
                    end: Some(3.0),
                    ..Item::whole(0.0)
                },
                Item {
                    rate: 0.5,
                    muted: true,
                    ..Item::whole(4.0)
                },
            ],
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
            json.contains(r#""path": "media"#),
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
    fn graph_bypass_round_trips_and_defaults_off() {
        let dir = temp_dir("project-bypass");
        let path = dir.join("bypass.rastersong");
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        project.save(&path).unwrap();
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("bypass_graph")
        );
        assert!(!Project::load(&path).unwrap().bypass_graph);
        project.bypass_graph = true;
        project.save(&path).unwrap();
        assert!(Project::load(&path).unwrap().bypass_graph);
    }

    #[test]
    fn tempo_round_trips_and_is_sanitized_on_load() {
        let dir = temp_dir("project-tempo");
        let path = dir.join("tempo.rastersong");
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        project.tempo = Tempo {
            bpm: 133.5,
            beats_per_bar: 3,
            offset_secs: 0.25,
        };
        project.timeline_mode = TimelineMode::Tempo;
        project.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap(), project);

        project.tempo.bpm = 0.0;
        project.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap().tempo.bpm, Tempo::MIN_BPM);
    }

    #[test]
    fn max_warmup_frames_round_trips_defaults_and_is_limited_on_load() {
        let dir = temp_dir("project-warmup");
        let path = dir.join("warmup.rastersong");
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        project.save(&path).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("warmup"));
        assert_eq!(
            Project::load(&path).unwrap().max_warmup_frames,
            DEFAULT_MAX_WARMUP_FRAMES
        );

        project.max_warmup_frames = 400;
        project.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap().max_warmup_frames, 400);

        std::fs::write(
            &path,
            r#"{ "version": 0, "max_warmup_frames": 1000000, "graph": { "version": 0, "nodes": [] } }"#,
        )
        .unwrap();
        assert_eq!(
            Project::load(&path).unwrap().max_warmup_frames,
            MAX_WARMUP_FRAMES_LIMIT
        );
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

#[cfg(test)]
mod inspect_rate_tests {
    use super::*;

    #[test]
    fn inspect_rate_round_trips_defaults_and_is_limited_on_load() {
        let dir = std::env::temp_dir().join(format!("rastersong-inspect-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("inspect.rastersong");
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        project.save(&path).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("inspect"));
        assert_eq!(
            Project::load(&path).unwrap().inspect_rate,
            DEFAULT_INSPECT_RATE
        );

        project.inspect_rate = 12.0;
        project.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap().inspect_rate, 12.0);

        std::fs::write(
            &path,
            r#"{ "version": 0, "inspect_rate": 5000, "graph": { "version": 0, "nodes": [] } }"#,
        )
        .unwrap();
        assert_eq!(Project::load(&path).unwrap().inspect_rate, 30.0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
