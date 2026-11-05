//! Project files: the timeline, its media and the graph the user is working on.

use std::path::{Path, PathBuf};

use rastersong_graph::nodes::{AUDIO_BUS_OUTPUT, BUS_PARAM, DEFAULT_BUS};
use rastersong_graph::{GraphDesc, ParamValue, Tempo, audio_output_bus};
use rastersong_lang::tr_args;
use serde::{Deserialize, Serialize};

use crate::timeline::{Bus, Item, Timebase, TrackKind};

mod editing;
mod graphs;
mod resources;
mod sections;
mod sources;

pub use editing::{Edge, ItemLengths, ItemRef, MIN_ITEM_LENGTH, RATE_RANGE, snap_offset};
pub use graphs::{DEFAULT_GRAPH_NAME, PASSTHROUGH_GRAPH};
pub use resources::{MediaFile, Resource, ResourceContent, ResourceId, resource_name_for};

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
    /// The media and graphs the project uses, in the order the Resources panel lists them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<Resource>,
    /// The tracks, a section of each kind in [`TrackKind`]'s order, each top first: the graph
    /// tracks (the topmost active graph item's outputs are the master), then the video tracks,
    /// then the audio tracks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<ProjectTrack>,
    /// The output buses, master first: audio tracks are summed into them in the mix, and each
    /// Audio Output writes to one. Never empty; Main, stereo, by default.
    #[serde(default = "default_buses", skip_serializing_if = "is_default_buses")]
    pub buses: Vec<Bus>,
    /// The loop region on the timeline, if one has been made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_region: Option<LoopRegion>,
    /// The tempo beat and bar units follow.
    #[serde(default)]
    pub tempo: Tempo,
    /// Whether the timeline ruler shows time or bars and beats (tempo).
    #[serde(default)]
    pub timeline_mode: TimelineMode,
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

/// A track: items of resources of its kind, in order and never overlapping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrack {
    /// What the timeline and graph sources call the track. Unique among all tracks.
    pub name: String,
    /// Its section: what it plays.
    pub kind: TrackKind,
    /// What plays where on the timeline, by position.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Item>,
    /// Level in the audio mix, 0 to 1, for kinds that [have one](TrackKind::has_level). Graphs
    /// read the track as it is.
    #[serde(default = "full_volume", skip_serializing_if = "is_full_volume")]
    pub volume: f32,
    /// Left out of the mix: a muted graph track is skipped, so what is below it shows through.
    /// Graphs still read a muted video or audio track.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
    /// The output bus an audio track is summed into in the mix.
    #[serde(default = "main_bus", skip_serializing_if = "is_main_bus")]
    pub bus: String,
    /// While any track of a kind is soloed, only soloed tracks of that kind are in the mix.
    /// Graphs still read every track.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub solo: bool,
    /// The track's height on the timeline, in points. `None` is the app's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
}

fn full_volume() -> f32 {
    1.0
}

fn is_full_volume(volume: &f32) -> bool {
    *volume == 1.0
}

impl ProjectTrack {
    /// An empty track of `kind`.
    pub fn new(name: String, kind: TrackKind) -> Self {
        Self {
            name,
            kind,
            items: Vec::new(),
            volume: 1.0,
            muted: false,
            bus: main_bus(),
            solo: false,
            height: None,
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
}

impl Project {
    /// A project holding one resource: `graph`, as [`Self::FIRST_GRAPH`] called
    /// [`DEFAULT_GRAPH_NAME`].
    pub fn new(graph: GraphDesc) -> Self {
        let mut project = Self::empty();
        let id = project.add_graph(DEFAULT_GRAPH_NAME, Some(graph));
        debug_assert_eq!(id, Self::FIRST_GRAPH);
        project
    }

    /// The id of the graph [`Self::new`] makes.
    pub const FIRST_GRAPH: ResourceId = ResourceId(1);

    /// A project with nothing in it.
    pub fn empty() -> Self {
        Self {
            version: PROJECT_VERSION,
            timebase: None,
            resources: Vec::new(),
            tracks: Vec::new(),
            buses: default_buses(),
            loop_region: None,
            tempo: Tempo::default(),
            timeline_mode: TimelineMode::default(),
            audio_rate: crate::DEFAULT_AUDIO_RATE,
            max_warmup_frames: DEFAULT_MAX_WARMUP_FRAMES,
            inspect_rate: DEFAULT_INSPECT_RATE,
        }
    }

    /// What `rastersong render <video> <audio> <graph>` renders: `graph` (as
    /// [`Self::FIRST_GRAPH`]) on a graph track for the first `length` seconds, over the file at
    /// `video` on the video track [`VIDEO_SOURCE`](crate::VIDEO_SOURCE) and the file at `audio` on
    /// the audio track [`DEFAULT_AUDIO_TRACK`](crate::DEFAULT_AUDIO_TRACK). The audio starts
    /// `audio_offset` seconds after the video; a negative offset starts it partway into the
    /// audio.
    pub fn from_files(
        video: impl Into<PathBuf>,
        audio: impl Into<PathBuf>,
        graph: GraphDesc,
        audio_offset: f64,
        length: f64,
    ) -> Self {
        let mut project = Self::new(graph);
        project.add_track(TrackKind::Video, crate::VIDEO_SOURCE, video);
        let song = project.add_track(TrackKind::Audio, crate::DEFAULT_AUDIO_TRACK, audio);
        song.items[0].position = audio_offset.max(0.0);
        song.items[0].start = (-audio_offset).max(0.0);
        let lengths = ItemLengths {
            new_graph_length: length,
            ..ItemLengths::default()
        };
        project.add_track_for(Self::FIRST_GRAPH, 0.0, &lengths);
        project
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

    /// What removing bus `name` affects: the audio tracks routed to it, and the Audio Bus
    /// Outputs that write to it, in every graph, as (graph name, node id).
    pub fn bus_users(&self, name: &str) -> (Vec<String>, Vec<(String, String)>) {
        let tracks = self
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio && t.bus == name)
            .map(|t| t.name.clone())
            .collect();
        let outputs = self
            .resources
            .iter()
            .filter_map(|r| Some((&r.name, r.graph()?)))
            .flat_map(|(graph, desc)| {
                desc.nodes
                    .iter()
                    .filter(|n| n.kind == AUDIO_BUS_OUTPUT && audio_output_bus(n) == name)
                    .map(|n| (graph.clone(), n.id.clone()))
            })
            .collect();
        (tracks, outputs)
    }

    /// Removes bus `name`, unless it is the last. Its tracks move to the master bus (the first
    /// left); Audio Bus Outputs writing to it are left as they are, writing to a bus that isn't
    /// there, so they aren't rendered until pointed at another.
    pub fn remove_bus(&mut self, name: &str) -> bool {
        if self.buses.len() <= 1 || !self.has_bus(name) {
            return false;
        }
        self.buses.retain(|b| b.name != name);
        Bus::mark_master(&mut self.buses);
        let master = self.master_bus().to_owned();
        for track in &mut self.tracks {
            if track.bus == name {
                track.bus = master.clone();
            }
        }
        true
    }

    /// Renames bus `old` to `new`, with the tracks routed to it and the Audio Bus Outputs
    /// writing to it. Refused when `new` is empty or another bus has it.
    pub fn rename_bus(&mut self, old: &str, new: &str) -> bool {
        let new = new.trim();
        if new.is_empty() || old == new || self.has_bus(new) {
            return false;
        }
        let Some(bus) = self.buses.iter_mut().find(|b| b.name == old) else {
            return false;
        };
        bus.name = new.to_owned();
        for track in &mut self.tracks {
            if track.bus == old {
                track.bus = new.to_owned();
            }
        }
        for node in self.graphs_mut().flat_map(|g| &mut g.nodes) {
            if node.kind == AUDIO_BUS_OUTPUT && audio_output_bus(node) == old {
                node.params
                    .insert(BUS_PARAM.to_owned(), ParamValue::Text(new.to_owned()));
            }
        }
        true
    }

    /// Renames track `old` to `new`, and the graph sources that read it. Refused (false, nothing
    /// changed) when there is no such track, or `new` is empty or another track's. Renaming a
    /// track to its own name succeeds.
    pub fn rename_track(&mut self, old: &str, new: &str) -> bool {
        let new = new.trim();
        if !self.has_track(old) || new.is_empty() {
            return false;
        }
        if new == old {
            return true;
        }
        if self.has_track(new) {
            return false;
        }
        for track in self.tracks_mut().filter(|t| t.name == old) {
            track.name = new.to_owned();
        }
        self.rename_sources(old, new);
        true
    }

    /// The track called `name`.
    pub fn track(&self, name: &str) -> Option<&ProjectTrack> {
        self.tracks().find(|t| t.name == name)
    }

    /// Every track, top first.
    pub fn tracks(&self) -> impl Iterator<Item = &ProjectTrack> {
        self.tracks.iter()
    }

    fn tracks_mut(&mut self) -> impl Iterator<Item = &mut ProjectTrack> {
        self.tracks.iter_mut()
    }

    /// Whether any track is called `name`.
    pub fn has_track(&self, name: &str) -> bool {
        self.track(name).is_some()
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

    /// The video: the first video track that plays something, top down.
    pub fn video(&self) -> Option<&ProjectTrack> {
        self.tracks_of(TrackKind::Video)
            .find(|t| !t.items.is_empty())
    }

    /// The video's file.
    pub fn video_path(&self) -> Option<&Path> {
        self.video().and_then(|t| self.track_path(t))
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
        for graph in project.graphs_mut() {
            graph.upgrade();
        }
        project.buses = sanitized_buses(std::mem::take(&mut project.buses));
        let dir = path.parent().unwrap_or(Path::new(""));
        for file in project.resources.iter_mut().filter_map(Resource::file_mut) {
            if file.path.is_relative() {
                file.path = dir.join(&file.path);
            }
        }
        // Every item must play a resource the project has, of its track's kind.
        let kind_of = |id: ResourceId| project.resource(id).map(Resource::kind);
        let unknown = project
            .tracks
            .iter()
            .find(|t| t.items.iter().any(|i| kind_of(i.resource) != Some(t.kind)));
        if let Some(track) = unknown {
            return Err(error(&tr_args(
                "error.project.unknown_resource",
                &[("track", &track.name)],
            )));
        }
        project.sanitize_sections();
        project.sanitize_items();
        project.tidy_groups();
        Ok(project)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let dir = path.parent().unwrap_or(Path::new(""));
        let mut saved = self.clone();
        for file in saved.resources.iter_mut().filter_map(Resource::file_mut) {
            if let Ok(relative) = file.path.strip_prefix(dir) {
                file.path = relative.to_path_buf();
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
    Bus::mark_master(&mut kept);
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ComposedTrack;
    use crate::timeline::{LabelColor, MAX_BUS_CHANNELS};

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

    fn three_tracks() -> Project {
        let mut project = Project::new(graph());
        project.add_track(TrackKind::Video, "v", "v.mp4");
        for name in ["a", "b"] {
            project.add_track(TrackKind::Audio, name, format!("{name}.wav"));
        }
        project
    }

    #[test]
    fn soloing_leaves_the_other_tracks_of_its_kind_out_of_the_mix() {
        let mut project = three_tracks();
        project.tracks[2].solo = true;
        let gains = |p: &Project| -> Vec<f32> {
            p.composition()
                .tracks
                .iter()
                .map(ComposedTrack::mix_gain)
                .collect()
        };
        // The video isn't affected by an audio solo.
        assert_eq!(gains(&project), [1.0, 0.0, 1.0]);
        // A muted soloed track is still muted; the video solos among video tracks.
        project.tracks[2].muted = true;
        project.tracks[0].solo = true;
        assert_eq!(gains(&project), [1.0, 0.0, 0.0]);
        // Muting a video track changes the picture; an audio level doesn't.
        let before = project.composition();
        project.tracks[1].volume = 0.5;
        assert!(project.composition().renders_like(&before));
        project.tracks[0].muted = true;
        assert!(!project.composition().renders_like(&before));
    }

    #[test]
    fn tracks_of_both_kinds_share_one_set_of_names() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        assert_eq!(project.video_display_name(), None);
        let video = PathBuf::from("clips/take 3.mp4");
        // The video's own sound is named without the extension, so it doesn't clash.
        assert_eq!(resource_name_for(&video, TrackKind::Video), "take 3.mp4");
        assert_eq!(resource_name_for(&video, TrackKind::Audio), "take 3");
        let id = project.add_resource(TrackKind::Video, "take 3.mp4", &video, None);
        assert_eq!(
            project
                .add_track_for(id, 0.0, &ItemLengths::default())
                .as_deref(),
            Some("take 3.mp4")
        );
        assert_eq!(project.video_display_name().as_deref(), Some("take 3.mp4"));
        assert_eq!(
            project
                .add_track_for(id, 2.0, &ItemLengths::default())
                .as_deref(),
            Some("take 3.mp4_2")
        );
        assert_eq!(project.tracks[1].items[0].position, 2.0);
        assert_eq!(project.video_path(), Some(video.as_path()));
    }

    #[test]
    fn buses_route_tracks_and_audio_outputs_and_follow_renames_and_removals() {
        let mut project = Project::new(
            GraphDesc::from_json(
                r#"{ "version": 0, "nodes": [
                    { "id": "sound", "type": "audio_output" },
                    { "id": "stems", "type": "audio_bus_output", "params": { "bus": "Stems" } } ] }"#,
            )
            .unwrap(),
        );
        assert_eq!(project.buses, [Bus::main()]);
        assert_eq!(project.unused_bus_name(), "Bus 2");
        project.buses.push(Bus::new("Stems", 1));
        project.add_track(TrackKind::Audio, "kick", "kick.wav");
        project.tracks[0].bus = "Stems".into();
        project.tracks[0].volume = 0.5;
        let composition = project.composition();
        assert_eq!(composition.master(), Bus::main());
        assert_eq!(composition.tracks_on("Stems").count(), 1);
        assert_eq!(composition.tracks[0].gain, 0.5);
        assert_eq!(
            project.bus_users("Stems"),
            (
                vec!["kick".to_owned()],
                vec![("Graph".to_owned(), "stems".to_owned())]
            )
        );

        // Renaming follows the tracks and bus outputs; the Audio Output has no bus to follow.
        assert!(!project.rename_bus("Stems", "Main"));
        assert!(project.rename_bus("Stems", "Drums"));
        assert!(project.rename_bus("Main", "Music"));
        assert_eq!(project.tracks[0].bus, "Drums");
        let bus_of = |p: &Project, id: &str| {
            let graph = p.graph(Project::FIRST_GRAPH).unwrap();
            audio_output_bus(graph.nodes.iter().find(|n| n.id == id).unwrap()).to_owned()
        };
        assert_eq!(bus_of(&project, "sound"), "");
        assert_eq!(bus_of(&project, "stems"), "Drums");

        // Removing moves its tracks to the master and leaves its outputs pointing nowhere. The
        // last bus stays.
        assert!(project.remove_bus("Drums"));
        assert_eq!(project.tracks[0].bus, "Music");
        assert_eq!(bus_of(&project, "stems"), "Drums");
        assert!(!project.remove_bus("Music"));
    }

    #[test]
    fn renaming_a_bus_follows_audio_bus_outputs_in_every_graph() {
        let mut project = Project::new(graph());
        project.buses.push(Bus::new("Stems", 2));
        let stems = GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [
                { "id": "out", "type": "audio_bus_output", "params": { "bus": "Stems" } } ] }"#,
        )
        .unwrap();
        let other = project.add_graph("Other", Some(stems));
        assert_eq!(
            project.bus_users("Stems").1,
            [("Other".to_owned(), "out".to_owned())]
        );
        assert!(project.rename_bus("Stems", "Drums"));
        let node = &project.graph(other).unwrap().nodes[0];
        assert_eq!(audio_output_bus(node), "Drums");
    }

    #[test]
    fn buses_are_saved_only_when_changed_and_sanitized_on_load() {
        let dir = temp_dir("buses");
        let path = dir.join("buses.rastersong");
        let mut project = Project::new(graph());
        project.add_track(TrackKind::Audio, "song", "song.wav");
        project.save(&path).unwrap();
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(!json.contains("buses") && !json.contains("\"bus\""));
        project.buses.push(Bus::new(" Stems ", 99));
        project.buses.push(Bus::new("Main", 1));
        project.tracks[0].bus = "Stems".into();
        project.save(&path).unwrap();
        let loaded = Project::load(&path).unwrap();
        assert_eq!(
            loaded.buses,
            [Bus::main(), Bus::new("Stems", MAX_BUS_CHANNELS)]
        );
        assert_eq!(loaded.tracks[0].bus, "Stems");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn renaming_a_track_follows_through_to_sources() {
        let mut project = three_tracks();
        project.add_empty_track(TrackKind::Graph, "Graphs");
        let mut kick = rastersong_graph::NodeDesc::new("k".into(), crate::AUDIO_INPUT);
        kick.params.insert(
            crate::SOURCE_PARAM.into(),
            ParamValue::Text("track:a".into()),
        );
        project
            .graph_mut(Project::FIRST_GRAPH)
            .unwrap()
            .nodes
            .push(kick);
        // Taken, empty and unknown names are refused; its own name is fine.
        assert!(!project.rename_track("a", "b"));
        assert!(!project.rename_track("a", "  "));
        assert!(!project.rename_track("nope", "c"));
        assert!(project.rename_track("a", "a"));
        assert!(project.rename_track("a", " kick "));
        assert!(project.has_track("kick") && !project.has_track("a"));
        assert_eq!(
            crate::input_ports(project.graph(Project::FIRST_GRAPH).unwrap())
                .last()
                .unwrap()
                .source,
            crate::Source::Track("kick".into())
        );
        // Graph tracks share the names.
        assert!(!project.rename_track("kick", "Graphs"));
        // Video tracks are renamed the same way (below the graph track).
        assert!(project.rename_track("v", "clip"));
        assert_eq!(project.tracks[1].name, "clip");
    }

    #[test]
    fn offsets_move_the_first_item_and_never_before_the_start() {
        let mut track = ProjectTrack::new("a".into(), TrackKind::Audio);
        track.items.push(Item::whole(ResourceId(1), 0.0));
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
    fn the_composition_lists_video_tracks_before_audio_tracks() {
        let mut project = Project::new(graph());
        project.add_track(TrackKind::Audio, "song", "song.wav");
        project.add_track(TrackKind::Video, "clip.mp4", "clip.mp4");
        let composition = project.composition();
        assert_eq!(composition.timebase, None);
        let kinds: Vec<_> = composition
            .tracks
            .iter()
            .map(|t| (t.name.as_str(), t.kind))
            .collect();
        // Sections: the video track goes above the audio track added before it.
        assert_eq!(
            kinds,
            [("clip.mp4", TrackKind::Video), ("song", TrackKind::Audio)]
        );
        let first_video = composition.tracks_of(TrackKind::Video).next().unwrap();
        assert_eq!(
            composition
                .track_resource(first_video)
                .and_then(|r| r.file())
                .map(|f| f.path.clone()),
            Some(PathBuf::from("clip.mp4"))
        );
    }

    #[test]
    fn resources_are_named_uniquely_and_found_again_by_stream() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        let loop_a =
            project.add_resource(TrackKind::Audio, "Drum Loop", "music/Drum Loop.wav", None);
        let loop_b =
            project.add_resource(TrackKind::Audio, "Drum Loop", "other/Drum Loop.mp3", None);
        assert_ne!(loop_a, loop_b);
        assert_eq!(project.resource(loop_b).unwrap().name, "Drum Loop_2");
        // The same stream again is the same resource; another stream of the file is another.
        assert_eq!(
            project.add_resource(TrackKind::Audio, "x", "music/Drum Loop.wav", None),
            loop_a
        );
        let second = project.add_resource(TrackKind::Audio, "x", "music/Drum Loop.wav", Some(2));
        assert_ne!(second, loop_a);
        // Renames stay unique; removing a resource removes its items, not its tracks.
        assert!(project.rename_resource(loop_b, "Drum Loop"));
        assert_eq!(project.resource(loop_b).unwrap().name, "Drum Loop_2");
        assert!(!project.rename_resource(loop_b, "  "));
        project.add_track_for(second, 0.0, &ItemLengths::default());
        assert_eq!(project.resource_users(second), ["x"]);
        assert_eq!(project.remove_resource(second), ["x"]);
        assert!(project.tracks[0].items.is_empty());
        assert!(project.resource(second).is_none());
    }

    #[test]
    fn resource_colors_are_set_cleared_and_saved() {
        let mut project = Project::new(graph());
        let id = project.add_resource(TrackKind::Audio, "loop", "loop.wav", None);
        assert!(project.set_resource_color(id, Some(LabelColor::Teal)));
        assert!(!project.set_resource_color(id, Some(LabelColor::Teal)));
        assert!(!project.set_resource_color(ResourceId(99), Some(LabelColor::Red)));
        let json = serde_json::to_string(&project).unwrap();
        assert!(json.contains(r#""color":"teal""#));
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.resource(id).unwrap().color, Some(LabelColor::Teal));
        assert!(project.set_resource_color(id, None));
        assert!(!serde_json::to_string(&project).unwrap().contains("color"));
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
        project.add_track(TrackKind::Video, "clip", dir.join("media/clip.mp4"));
        // Absolute and outside the project folder on every platform.
        let elsewhere = std::env::current_dir().unwrap().join("elsewhere-song.wav");
        let elsewhere = project.add_resource(TrackKind::Audio, "elsewhere", elsewhere, None);
        project.tracks.push(ProjectTrack {
            items: vec![
                Item {
                    start: 1.25,
                    end: Some(3.0),
                    ..Item::whole(elsewhere, 0.0)
                },
                Item {
                    rate: 0.5,
                    muted: true,
                    ..Item::whole(elsewhere, 4.0)
                },
            ],
            volume: 0.5,
            muted: true,
            ..ProjectTrack::new("audio".into(), TrackKind::Audio)
        });
        project.add_track(TrackKind::Audio, "drums", dir.join("media/drums.wav"));
        // A chosen stream is kept.
        let voice = project.add_resource(
            TrackKind::Audio,
            "voice",
            dir.join("media/clip.mp4"),
            Some(2),
        );
        project.add_track_for(voice, 1.0, &ItemLengths::default());
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

        // An item must play a resource the project has.
        project.tracks[1].items[0].resource = ResourceId(999);
        project.save(&path).unwrap();
        let error = Project::load(&path).unwrap_err();
        assert!(error.contains("\"audio\""), "{error}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tempo_round_trips_and_is_sanitized_on_load() {
        let dir = temp_dir("project-tempo");
        let path = dir.join("tempo.rastersong");
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        project.tempo = Tempo {
            bpm: 133.5,
            beats_per_bar: 6,
            beat_unit: 8,
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

        std::fs::write(&path, r#"{ "version": 0, "max_warmup_frames": 1000000 }"#).unwrap();
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
}

#[cfg(test)]
mod inspect_rate_tests {
    use super::*;
    use crate::timeline::LabelColor;

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

        std::fs::write(&path, r#"{ "version": 0, "inspect_rate": 5000 }"#).unwrap();
        assert_eq!(Project::load(&path).unwrap().inspect_rate, 30.0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn graph_resources_are_added_duplicated_renamed_and_removed() {
        let starter = GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap();
        let mut project = Project::new(starter.clone());
        assert_eq!(
            project.graph_ids().collect::<Vec<_>>(),
            [Project::FIRST_GRAPH]
        );
        let new = project.add_graph("", None);
        // Names stay unique, a blank name is the default, and the template is the passthrough.
        let new_graph = project.resource(new).unwrap();
        assert_eq!(new_graph.name, "Graph_2");
        assert_eq!(new_graph.kind(), TrackKind::Graph);
        assert_eq!(project.graph(new).unwrap().nodes.len(), 4);
        assert_eq!(project.graph(Project::FIRST_GRAPH), Some(&starter));

        // A copy is a graph of its own with the original's colour; a stream of a file can't be
        // copied.
        project.set_resource_color(new, Some(LabelColor::Pink));
        let copy = project.duplicate_resource(new).unwrap();
        assert_eq!(project.resource(copy).unwrap().name, "Graph_2 copy");
        assert_eq!(
            project.resource(copy).unwrap().color,
            Some(LabelColor::Pink)
        );
        project.graph_mut(copy).unwrap().nodes.clear();
        assert_eq!(project.graph(new).unwrap().nodes.len(), 4);
        let song = project.add_resource(TrackKind::Audio, "song", "song.wav", None);
        assert_eq!(project.duplicate_resource(song), None);
        project.remove_resource(song);

        assert!(project.rename_resource(copy, "Copy"));
        assert!(project.rename_resource(Project::FIRST_GRAPH, "Graph_2"));
        assert_eq!(
            project.resource(Project::FIRST_GRAPH).unwrap().name,
            "Graph_2_2"
        );
        project.remove_resource(copy);
        assert_eq!(project.graph_ids().count(), 2);

        // They survive saving, kind and all.
        let dir = std::env::temp_dir().join("rastersong-graph-resources");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.rastersong");
        project.save(&path).unwrap();
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(json.contains(r#""kind": "graph""#), "{json}");
        assert_eq!(Project::load(&path).unwrap(), project);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn graphs_and_media_share_one_name_namespace() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        let song = project.add_resource(TrackKind::Audio, "Graph", "song.wav", None);
        assert_eq!(project.resource(song).unwrap().name, "Graph_2");
        let graph = project.add_graph("song", None);
        assert!(project.rename_resource(song, "song"));
        assert_eq!(project.resource(song).unwrap().name, "song_2");
        assert_eq!(project.resource(graph).unwrap().name, "song");
    }

    #[test]
    fn graph_resources_take_label_colours() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        assert!(project.set_resource_color(Project::FIRST_GRAPH, Some(LabelColor::Green)));
        let json = serde_json::to_string(&project).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.resource(Project::FIRST_GRAPH).unwrap().color,
            Some(LabelColor::Green)
        );
    }

    #[test]
    fn a_graph_item_of_an_unknown_graph_or_of_media_is_rejected_on_load() {
        let dir = std::env::temp_dir().join(format!("rastersong-unknown-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.rastersong");
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        let song = project.add_resource(TrackKind::Audio, "song", "song.wav", None);
        let track = project.add_empty_track(TrackKind::Graph, "FX");
        project
            .place_resource(&track, Project::FIRST_GRAPH, 0.0, &ItemLengths::default())
            .unwrap();
        project.save(&path).unwrap();
        assert!(Project::load(&path).is_ok());
        for wrong in [ResourceId(99), song] {
            project.tracks[0].items[0].resource = wrong;
            project.save(&path).unwrap();
            let error = Project::load(&path).unwrap_err();
            assert!(error.contains("\"FX\""), "{error}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_empty_track_takes_the_first_resource_of_its_kind_dropped_on_it() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        let clip = project.add_resource(TrackKind::Video, "clip", "clip.mp4", None);
        let other = project.add_resource(TrackKind::Video, "other", "other.mp4", None);
        let song = project.add_resource(TrackKind::Audio, "song", "song.wav", None);
        let name = project.add_empty_track(TrackKind::Video, "Video");
        let second = project.add_empty_track(TrackKind::Audio, "Audio");
        assert_eq!(
            project.add_empty_track(TrackKind::Audio, "Audio"),
            "Audio_2"
        );
        // Empty tracks play nothing.
        assert!(project.composition().is_empty());
        assert!(project.video().is_none());

        // Only its own kind fills an empty track.
        assert!(
            project
                .place_resource(&name, song, 0.0, &ItemLengths::default())
                .is_none()
        );
        assert!(
            project
                .place_resource(&name, clip, 1.0, &ItemLengths::default())
                .is_some()
        );
        assert_eq!(project.tracks[0].items[0].position, 1.0);
        assert_eq!(project.video().unwrap().name, name);
        assert!(
            project
                .place_resource(&second, song, 0.0, &ItemLengths::default())
                .is_some()
        );
        // The same resource adds an item; another resource is refused.
        assert!(
            project
                .place_resource(&name, clip, 5.0, &ItemLengths::default())
                .is_some()
        );
        assert_eq!(project.tracks[0].items.len(), 2);
        assert!(
            project
                .place_resource(&name, other, 0.0, &ItemLengths::default())
                .is_none()
        );
        assert!(!project.composition().is_empty());
    }

    #[test]
    fn several_resources_dropped_together_each_get_a_track_in_order() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        let stems: Vec<_> = ["drums", "bass", "keys"]
            .iter()
            .map(|n| project.add_resource(TrackKind::Audio, n, format!("{n}.wav"), None))
            .collect();
        let existing = project.add_resource(TrackKind::Audio, "old", "old.wav", None);
        project.place_linked(existing, None, 0.0, false, &ItemLengths::default());
        let placed = project.place_many_at(&stems, 0, 1.5, true, &ItemLengths::default());
        let names: Vec<&str> = project.tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["drums", "bass", "keys", "old"]);
        assert_eq!(placed.len(), 3);
        assert!(
            project.tracks[..3]
                .iter()
                .all(|t| t.items[0].position == 1.5)
        );
        // Each is on its own: they are not grouped with one another.
        assert_eq!(project.with_grouped(&placed[..1]).len(), 1);
        // Asked for twice, a resource is placed once.
        let again =
            project.place_many_at(&[stems[0], stems[0]], 4, 0.0, true, &ItemLengths::default());
        assert_eq!(again.len(), 1);
    }

    #[test]
    fn dropping_on_a_line_between_tracks_makes_new_tracks_there() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        let picture = project.add_resource(TrackKind::Video, "take.mp4", "take.mp4", None);
        let sound = project.add_resource(TrackKind::Audio, "take", "take.mp4", Some(1));
        let song = project.add_resource(TrackKind::Audio, "song", "song.wav", None);
        let names =
            |p: &Project| -> Vec<String> { p.tracks.iter().map(|t| t.name.clone()).collect() };
        project.place_linked(picture, None, 0.0, true, &ItemLengths::default());
        project.place_linked(song, None, 0.0, false, &ItemLengths::default());
        assert_eq!(names(&project), ["take.mp4", "take", "song"]);

        // Above the first track, even though the resource already has a track.
        project.place_linked_at(picture, 0, 1.0, false, &ItemLengths::default());
        assert_eq!(names(&project), ["take.mp4_2", "take.mp4", "take", "song"]);
        // Linked at the line under the videos: the picture goes there, the sound to the top of
        // the audio tracks (the same index), grouped.
        let placed = project.place_linked_at(picture, 2, 2.0, true, &ItemLengths::default());
        assert_eq!(
            names(&project),
            [
                "take.mp4_2",
                "take.mp4",
                "take.mp4_3",
                "take_2",
                "take",
                "song"
            ]
        );
        assert_eq!(project.with_grouped(&placed[..1]).len(), 2);
        // A line outside the resource's section: its own section's bottom.
        project.place_linked_at(song, 0, 0.0, false, &ItemLengths::default());
        assert_eq!(project.tracks.last().unwrap().name, "song_2");
        let _ = sound;
    }

    #[test]
    fn dropping_a_resource_places_its_whole_file_grouped_unless_asked_for_one() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        let picture = project.add_resource(TrackKind::Video, "take.mp4", "take.mp4", None);
        let sound = project.add_resource(TrackKind::Audio, "take", "take.mp4", Some(1));
        let commentary = project.add_resource(TrackKind::Audio, "voice", "take.mp4", Some(2));
        let song = project.add_resource(TrackKind::Audio, "song", "song.wav", None);
        assert_eq!(project.siblings(sound), [sound, picture, commentary]);
        assert_eq!(project.siblings(song), [song]);
        let empty = project.add_empty_track(TrackKind::Audio, "Audio");

        // Over the empty audio track: the first sound goes there, the rest on new tracks.
        let placed =
            project.place_linked(picture, Some(&empty), 2.0, true, &ItemLengths::default());
        let tracks: Vec<&str> = placed.iter().map(|at| at.track.as_str()).collect();
        assert_eq!(tracks, ["take.mp4", "Audio", "voice"]);
        let names: Vec<&str> = project.tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["take.mp4", "Audio", "voice"]);
        assert_eq!(project.with_grouped(&placed[..1]).len(), 3);
        assert!(
            placed
                .iter()
                .all(|at| { project.track(&at.track).unwrap().items[at.item].position == 2.0 })
        );

        // Again: each goes on the track already playing it, as a new group.
        let again = project.place_linked(sound, None, 10.0, true, &ItemLengths::default());
        assert_eq!(again.len(), 3);
        assert_eq!(project.tracks.len(), 3);
        assert_eq!(project.with_grouped(&again[..1]).len(), 3);

        // Only the one dragged.
        let one = project.place_linked(sound, None, 20.0, false, &ItemLengths::default());
        assert_eq!(one, [ItemRef::new("Audio", 2)]);
        assert_eq!(project.with_grouped(&one).len(), 1);
    }

    #[test]
    fn two_video_tracks_may_play_one_resource() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        let clip = project.add_resource(TrackKind::Video, "clip", "clip.mp4", None);
        let a = project
            .add_track_for(clip, 0.0, &ItemLengths::default())
            .unwrap();
        let b = project
            .add_track_for(clip, 0.0, &ItemLengths::default())
            .unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("clip", "clip_2"));
        let composition = project.composition();
        let paths: Vec<_> = composition
            .tracks
            .iter()
            .map(|t| composition.track_resource(t).and_then(|r| r.file()))
            .collect();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], paths[1]);
        // Removing the resource takes its items from both tracks; the tracks stay.
        assert_eq!(project.remove_resource(clip).len(), 2);
        assert_eq!(project.tracks.len(), 2);
        assert!(project.tracks.iter().all(|t| t.items.is_empty()));
    }
}
