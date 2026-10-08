//! Project files: the timeline, its media and the graph the user is working on.

use std::path::Path;

use rastersong_graph::nodes::{AUDIO_OUTPUT, BUS_PARAM, DEFAULT_BUS};
use rastersong_graph::{GraphDesc, ParamValue, Tempo, audio_output_bus};
use rastersong_lang::tr_args;
use serde::{Deserialize, Serialize};

use crate::timeline::{Bus, Fx, Item, Timebase, TrackKind};

mod editing;
mod fx;
mod graphs;
mod resources;
mod tree;

pub use editing::{Edge, ItemRef, MIN_ITEM_LENGTH, RATE_RANGE, snap_offset};
pub use fx::FxTarget;
pub use graphs::{GraphEntry, PASSTHROUGH_GRAPH, StoredGraph};
pub use resources::{Resource, ResourceId, ResourceKind, resource_name_for};
pub use tree::drop_depths;

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
    /// The media the project uses, in the order the Resources panel lists them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<Resource>,
    /// The track tree, top first, as the timeline lists it: each folder is followed by the
    /// tracks inside it, one level deeper (see [`ProjectTrack::depth`]). Tracks of any kind go
    /// anywhere.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<ProjectTrack>,
    /// The output buses, master first: top-level tracks are summed into them in the track mix,
    /// and each Audio Output writes to one. Never empty; Main, stereo, by default.
    #[serde(default = "default_buses", skip_serializing_if = "is_default_buses")]
    pub buses: Vec<Bus>,
    /// The open graph: what the editor shows and the engine renders.
    pub graph: GraphDesc,
    /// The open graph's id and name among the project's graphs.
    #[serde(default = "graphs::first_graph_id")]
    pub graph_id: u32,
    #[serde(default = "graphs::default_graph_name")]
    pub graph_name: String,
    /// The project's other graphs, kept until one is opened.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graphs: Vec<StoredGraph>,
    /// The master's FX chain, run on everything the top-level tracks send. See
    /// [`Self::routing`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub master_fx: Vec<Fx>,
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

/// A track on the timeline: items of one resource, or a folder of other tracks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrack {
    /// What the timeline and receives call the track. Unique among all tracks.
    pub name: String,
    /// How deep in the tree the track sits: 0 at the top, one more inside each folder. A track
    /// is inside the nearest folder above it that is one level shallower.
    #[serde(default, skip_serializing_if = "is_zero_depth")]
    pub depth: u32,
    /// A folder holds the tracks below it that are deeper, and mixes them like a bus. It has no
    /// resource and no items of its own.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub folder: bool,
    /// A collapsed folder hides its tracks on the timeline.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub collapsed: bool,
    /// A folder where new tracks of this kind go (the template's Video and Audio folders).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_tracks: Option<TrackKind>,
    /// What the track plays: `None` for an empty track, which takes the first resource dropped
    /// on it, or a folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceId>,
    /// Where the resource plays on the timeline.
    #[serde(default = "whole")]
    pub items: Vec<Item>,
    /// The track's FX chain, run on its items' picture and sound (a folder's, on its mix).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fx: Vec<Fx>,
    /// Level in the track mix, 0 to 1; a folder's scales everything in it. Graphs read the
    /// track as it is.
    #[serde(default = "full_volume")]
    pub volume: f32,
    /// Left out of the track mix, and for a folder everything in it. Graphs still read the
    /// track.
    #[serde(default)]
    pub muted: bool,
    /// Whether the track is mixed into its folder, or at the top into its bus. Off, it reaches
    /// no output and is only there for graphs to read.
    #[serde(default = "sends_default", skip_serializing_if = "is_true")]
    pub master_send: bool,
    /// The output bus a top-level track (and everything in it) is summed into in the track mix.
    /// Tracks inside a folder go where their folder goes.
    #[serde(default = "main_bus", skip_serializing_if = "is_main_bus")]
    pub bus: String,
    /// While any track of a kind is soloed, only soloed tracks of that kind are in the track
    /// mix; soloing a folder solos everything in it. Graphs still read every track.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub solo: bool,
    /// The track's height on the timeline, in points. `None` is the app's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    /// The tracks with the same link group, video or audio, move their items together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<u32>,
}

fn full_volume() -> f32 {
    1.0
}

fn sends_default() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

fn is_zero_depth(depth: &u32) -> bool {
    *depth == 0
}

fn whole() -> Vec<Item> {
    vec![Item::whole(0.0)]
}

impl ProjectTrack {
    /// A track playing the whole resource from the start of the timeline.
    pub fn new(name: String, resource: ResourceId) -> Self {
        Self {
            name,
            depth: 0,
            folder: false,
            collapsed: false,
            new_tracks: None,
            resource: Some(resource),
            items: whole(),
            fx: Vec::new(),
            volume: 1.0,
            muted: false,
            master_send: true,
            bus: main_bus(),
            solo: false,
            height: None,
            link: None,
        }
    }

    /// A track with no resource and no items yet.
    pub fn empty(name: String) -> Self {
        Self {
            resource: None,
            items: Vec::new(),
            ..Self::new(name, ResourceId(0))
        }
    }

    /// An empty folder.
    pub fn new_folder(name: String) -> Self {
        Self {
            folder: true,
            ..Self::empty(name)
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
    pub fn new(graph: GraphDesc) -> Self {
        Self {
            version: PROJECT_VERSION,
            timebase: None,
            resources: Vec::new(),
            tracks: Vec::new(),
            buses: default_buses(),
            graph,
            graph_id: graphs::first_graph_id(),
            graph_name: graphs::default_graph_name(),
            graphs: Vec::new(),
            master_fx: Vec::new(),
            loop_region: None,
            tempo: Tempo::default(),
            timeline_mode: TimelineMode::default(),
            bypass_graph: false,
            audio_rate: crate::DEFAULT_AUDIO_RATE,
            max_warmup_frames: DEFAULT_MAX_WARMUP_FRAMES,
            inspect_rate: DEFAULT_INSPECT_RATE,
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

    /// What removing bus `name` affects: the top-level tracks routed to it, and the ids of the
    /// graph's Audio Outputs that write to it.
    pub fn bus_users(&self, name: &str) -> (Vec<String>, Vec<String>) {
        let tracks = self
            .tracks
            .iter()
            .filter(|t| t.depth == 0 && t.bus == name)
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
        for track in &mut self.tracks {
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
        for track in &mut self.tracks {
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

    /// The names of the other tracks linked to track `name`.
    pub fn linked_to(&self, name: &str) -> Vec<String> {
        let Some(group) = self.track(name).and_then(|t| t.link) else {
            return Vec::new();
        };
        self.tracks()
            .filter(|t| t.link == Some(group) && t.name != name)
            .map(|t| t.name.clone())
            .collect()
    }

    /// Links tracks `a` and `b`, and with them every track already linked to either.
    pub fn link_tracks(&mut self, a: &str, b: &str) {
        if a == b || !self.has_track(a) || !self.has_track(b) {
            return;
        }
        let groups = [a, b].map(|n| self.track(n).and_then(|t| t.link));
        let group = groups.into_iter().flatten().min().unwrap_or_else(|| {
            self.tracks()
                .filter_map(|t| t.link)
                .max()
                .map_or(1, |g| g + 1)
        });
        for track in self.tracks_mut() {
            if track.name == a
                || track.name == b
                || (track.link.is_some() && groups.contains(&track.link))
            {
                track.link = Some(group);
            }
        }
    }

    /// Takes track `name` out of its link; a track left linked to nothing is unlinked too.
    pub fn unlink_track(&mut self, name: &str) {
        let Some(group) = self.track(name).and_then(|t| t.link) else {
            return;
        };
        for track in self.tracks_mut() {
            if track.name == name {
                track.link = None;
            }
        }
        if self.tracks().filter(|t| t.link == Some(group)).count() == 1 {
            for track in self.tracks_mut() {
                if track.link == Some(group) {
                    track.link = None;
                }
            }
        }
    }

    /// Renames track `old` to `new`, and what refers to it by name (FX receives). Refused
    /// (false, nothing changed) when there is no such track, or `new` is empty or another
    /// track's. Renaming a track to its own name succeeds.
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
        self.rename_receives(old, new);
        true
    }

    /// The track (or folder) called `name`.
    pub fn track(&self, name: &str) -> Option<&ProjectTrack> {
        self.tracks().find(|t| t.name == name)
    }

    /// Every track and folder, top first.
    pub fn tracks(&self) -> impl Iterator<Item = &ProjectTrack> {
        self.tracks.iter()
    }

    fn tracks_mut(&mut self) -> impl Iterator<Item = &mut ProjectTrack> {
        self.tracks.iter_mut()
    }

    /// Whether a track or folder is called `name`.
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

    /// The video: the first video track, top down.
    pub fn video(&self) -> Option<&ProjectTrack> {
        self.tracks_of(TrackKind::Video).next()
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
        project.graph.upgrade();
        project.buses = sanitized_buses(std::mem::take(&mut project.buses));
        let dir = path.parent().unwrap_or(Path::new(""));
        for resource in &mut project.resources {
            if resource.path.is_relative() {
                resource.path = dir.join(&resource.path);
            }
        }
        let resources = &project.resources;
        let unknown = project.tracks.iter().find(|t| {
            t.resource
                .is_some_and(|id| !resources.iter().any(|r| r.id == id))
        });
        if let Some(track) = unknown {
            return Err(error(&tr_args(
                "error.project.track_resource",
                &[("track", &track.name)],
            )));
        }
        project.sanitize_tree();
        for track in project.tracks_mut() {
            for item in &mut track.items {
                *item = item.clone().sanitized();
            }
        }
        Ok(project)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let dir = path.parent().unwrap_or(Path::new(""));
        let mut saved = self.clone();
        for resource in &mut saved.resources {
            if let Ok(relative) = resource.path.strip_prefix(dir) {
                resource.path = relative.to_path_buf();
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
    use std::path::PathBuf;

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
        let gains: Vec<f32> = project.timeline().tracks.iter().map(|t| t.gain).collect();
        // The video isn't affected by an audio solo.
        assert_eq!(gains, [1.0, 0.0, 1.0]);
        // A muted soloed track is still muted; the video solos among video tracks.
        project.tracks[2].muted = true;
        project.tracks[0].solo = true;
        let gains: Vec<f32> = project.timeline().tracks.iter().map(|t| t.gain).collect();
        assert_eq!(gains, [1.0, 0.0, 0.0]);
        // Muting a video track changes the picture; an audio level doesn't.
        let before = project.timeline();
        project.tracks[1].volume = 0.5;
        assert!(project.timeline().renders_like(&before));
        project.tracks[0].muted = true;
        assert!(!project.timeline().renders_like(&before));
    }

    #[test]
    fn linking_joins_groups_and_unlinking_the_second_to_last_ends_it() {
        let mut project = three_tracks();
        project.link_tracks("v", "a");
        assert_eq!(project.linked_to("v"), ["a"]);
        project.link_tracks("b", "a");
        assert_eq!(project.linked_to("a"), ["v", "b"]);
        project.unlink_track("v");
        assert_eq!(project.linked_to("a"), ["b"]);
        project.unlink_track("b");
        assert!(project.tracks().all(|t| t.link.is_none()));
        // Two separate links merge when linked.
        project.link_tracks("v", "a");
        project.add_track(TrackKind::Audio, "c", "c.wav");
        project.link_tracks("b", "c");
        project.link_tracks("a", "c");
        assert_eq!(project.linked_to("v"), ["a", "b", "c"]);
    }

    #[test]
    fn moving_an_item_moves_the_overlapping_items_of_linked_tracks() {
        let mut project = three_tracks();
        // a: items at 0..2 and 5..7; b: one at 1..3; v: 0..10. Each file is 2 s (v 10 s).
        project.tracks[1].items = vec![Item::whole(0.0), Item::whole(5.0)];
        project.tracks[2].items = vec![Item::whole(1.0)];
        let duration = |t: &ProjectTrack| Some(if t.name == "v" { 10.0 } else { 2.0 });
        let positions = |p: &Project| -> Vec<Vec<f64>> {
            p.tracks()
                .map(|t| t.items.iter().map(|i| i.position).collect())
                .collect()
        };
        // Unlinked, only the item dragged moves.
        assert_eq!(project.move_item("a", 1, 1.0, duration), 1.0);
        assert_eq!(positions(&project), [vec![0.0], vec![0.0, 6.0], vec![1.0]]);
        // Linked, b's item overlaps a's first; a's second item and the video don't (v isn't
        // linked).
        project.link_tracks("a", "b");
        assert_eq!(project.move_item("a", 0, 0.5, duration), 0.5);
        assert_eq!(positions(&project), [vec![0.0], vec![0.5, 6.0], vec![1.5]]);
        // Moving left stops when the first of them reaches the start.
        assert_eq!(project.move_item("b", 0, -3.0, duration), -0.5);
        assert_eq!(positions(&project), [vec![0.0], vec![0.0, 6.0], vec![1.0]]);
        assert_eq!(project.move_item("nope", 0, 1.0, duration), 0.0);
    }

    #[test]
    fn tracks_of_both_kinds_share_one_set_of_names() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        assert_eq!(project.video_display_name(), None);
        let video = PathBuf::from("clips/take 3.mp4");
        // The video's own sound is named without the extension, so it doesn't clash.
        assert_eq!(resource_name_for(&video, ResourceKind::Video), "take 3.mp4");
        assert_eq!(resource_name_for(&video, ResourceKind::Audio), "take 3");
        let id = project.add_resource(ResourceKind::Video, "take 3.mp4", &video, None);
        assert_eq!(
            project.add_track_for(id, 0.0).as_deref(),
            Some("take 3.mp4")
        );
        assert_eq!(project.video_display_name().as_deref(), Some("take 3.mp4"));
        assert_eq!(
            project.add_track_for(id, 2.0).as_deref(),
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
        project.add_track(TrackKind::Audio, "kick", "kick.wav");
        project.tracks[0].bus = "Stems".into();
        project.tracks[0].volume = 0.5;
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
        assert_eq!(project.tracks[0].bus, "Drums");
        let bus_of = |p: &Project, id: &str| {
            audio_output_bus(p.graph.nodes.iter().find(|n| n.id == id).unwrap()).to_owned()
        };
        assert_eq!(bus_of(&project, "sound"), "Music");
        assert_eq!(bus_of(&project, "stems"), "Drums");

        // Removing moves its tracks to the master and leaves its outputs pointing nowhere. The
        // last bus stays.
        assert!(project.remove_bus("Drums"));
        assert_eq!(project.tracks[0].bus, "Music");
        assert_eq!(bus_of(&project, "stems"), "Drums");
        assert!(!project.remove_bus("Music"));
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
        project.buses.push(Bus {
            name: " Stems ".into(),
            channels: 99,
        });
        project.buses.push(Bus {
            name: "Main".into(),
            channels: 1,
        });
        project.tracks[0].bus = "Stems".into();
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
        assert_eq!(loaded.tracks[0].bus, "Stems");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn renaming_a_track_follows_through_to_receives() {
        let mut project = three_tracks();
        let graph = project.graph_id;
        project.add_fx(&FxTarget::Master, graph).unwrap();
        project.set_fx_receive(&FxTarget::Master, 0, "Kick", Some("a"));
        // Taken, empty and unknown names are refused; its own name is fine.
        assert!(!project.rename_track("a", "b"));
        assert!(!project.rename_track("a", "  "));
        assert!(!project.rename_track("nope", "c"));
        assert!(project.rename_track("a", "a"));
        assert!(project.rename_track("a", " kick "));
        assert!(project.has_track("kick") && !project.has_track("a"));
        assert_eq!(project.master_fx[0].receives["Kick"], "kick");
        // Video tracks are renamed the same way.
        assert!(project.rename_track("v", "clip"));
        assert_eq!(project.tracks[0].name, "clip");
    }

    #[test]
    fn offsets_move_the_first_item_and_never_before_the_start() {
        let mut track = ProjectTrack::new("a".into(), ResourceId(1));
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
    fn the_timeline_lists_tracks_in_their_order() {
        let mut project = Project::new(graph());
        project.add_track(TrackKind::Audio, "song", "song.wav");
        project.add_track(TrackKind::Video, "clip.mp4", "clip.mp4");
        let timeline = project.timeline();
        assert_eq!(timeline.timebase, None);
        let kinds: Vec<_> = timeline
            .tracks
            .iter()
            .map(|t| (t.name.as_str(), t.kind))
            .collect();
        // One tree: an audio track can sit above a video track.
        assert_eq!(
            kinds,
            [("song", TrackKind::Audio), ("clip.mp4", TrackKind::Video)]
        );
        assert_eq!(
            timeline.first_video().map(|t| t.path.clone()),
            Some(PathBuf::from("clip.mp4"))
        );
    }

    #[test]
    fn resources_are_named_uniquely_and_found_again_by_stream() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        let loop_a = project.add_resource(
            ResourceKind::Audio,
            "Drum Loop",
            "music/Drum Loop.wav",
            None,
        );
        let loop_b = project.add_resource(
            ResourceKind::Audio,
            "Drum Loop",
            "other/Drum Loop.mp3",
            None,
        );
        assert_ne!(loop_a, loop_b);
        assert_eq!(project.resource(loop_b).unwrap().name, "Drum Loop_2");
        // The same stream again is the same resource; another stream of the file is another.
        assert_eq!(
            project.add_resource(ResourceKind::Audio, "x", "music/Drum Loop.wav", None),
            loop_a
        );
        let second = project.add_resource(ResourceKind::Audio, "x", "music/Drum Loop.wav", Some(2));
        assert_ne!(second, loop_a);
        // Renames stay unique; removing a resource removes its tracks.
        assert!(project.rename_resource(loop_b, "Drum Loop"));
        assert_eq!(project.resource(loop_b).unwrap().name, "Drum Loop_2");
        assert!(!project.rename_resource(loop_b, "  "));
        project.add_track_for(second, 0.0);
        assert_eq!(project.resource_users(second), ["x"]);
        assert_eq!(project.remove_resource(second), ["x"]);
        assert!(project.tracks.is_empty());
        assert!(project.resource(second).is_none());
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
        let elsewhere = project.add_resource(ResourceKind::Audio, "elsewhere", elsewhere, None);
        project.tracks.push(ProjectTrack {
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
        project.add_track(TrackKind::Audio, "drums", dir.join("media/drums.wav"));
        // A chosen stream is kept.
        let voice = project.add_resource(
            ResourceKind::Audio,
            "voice",
            dir.join("media/clip.mp4"),
            Some(2),
        );
        project.add_track_for(voice, 1.0);
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

        // A track must play a resource the project has.
        project.tracks[1].resource = Some(ResourceId(999));
        project.save(&path).unwrap();
        let error = Project::load(&path).unwrap_err();
        assert!(error.contains("\"audio\""), "{error}");
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

    #[test]
    fn graphs_are_kept_and_swapped_in_when_opened() {
        let starter = GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap();
        let mut project = Project::new(starter.clone());
        assert_eq!(project.graph_entries().len(), 1);
        let new = project.add_graph("Graph", None);
        // Names stay unique, and the template is the passthrough.
        assert_eq!(project.graph_entries()[1].name, "Graph_2");
        assert_eq!(project.graphs[0].graph.nodes.len(), 4);

        assert!(project.open_graph(new));
        assert_eq!(project.graph.nodes.len(), 4);
        assert_eq!(
            (project.graph_id, project.graph_name.as_str()),
            (new, "Graph_2")
        );
        // The graph that was open is stored in its place, as it was.
        assert_eq!(project.graphs[0].graph, starter);
        // The open graph can't be opened again or removed; a stored one can.
        assert!(!project.open_graph(new));
        assert!(!project.remove_graph(new));
        let copy = project.duplicate_graph(new).unwrap();
        assert!(project.rename_graph(copy, "Copy"));
        assert!(project.remove_graph(copy));
        assert!(project.rename_graph(1, "Graph_2"));
        assert_eq!(
            project
                .graph_entries()
                .iter()
                .filter(|g| g.name == "Graph_2")
                .count(),
            1
        );

        // They survive saving.
        let dir = std::env::temp_dir().join("rastersong-graph-resources");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.rastersong");
        project.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap(), project);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_empty_track_takes_the_first_resource_dropped_on_it() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        let clip = project.add_resource(ResourceKind::Video, "clip", "clip.mp4", None);
        let other = project.add_resource(ResourceKind::Video, "other", "other.mp4", None);
        let song = project.add_resource(ResourceKind::Audio, "song", "song.wav", None);
        let name = project.add_empty_track();
        let second = project.add_empty_track();
        assert_eq!(second, "Track_2");
        // Empty tracks play nothing: the engine isn't told about them.
        assert!(project.timeline().tracks.is_empty());
        assert!(project.video().is_none());

        // Either kind fills an empty track.
        assert!(project.place_resource(&name, clip, 1.0));
        assert_eq!(project.tracks[0].items[0].position, 1.0);
        assert_eq!(project.video().unwrap().name, name);
        assert!(project.place_resource(&second, song, 0.0));
        assert_eq!(
            project.track_kind(&project.tracks[1]),
            Some(TrackKind::Audio)
        );
        // The same resource adds an item; another resource is refused, and so is a folder.
        assert!(project.place_resource(&name, clip, 5.0));
        assert_eq!(project.tracks[0].items.len(), 2);
        assert!(!project.place_resource(&name, other, 0.0));
        let folder = project.add_folder("Folder");
        assert!(!project.place_resource(&folder, other, 0.0));
        assert_eq!(project.timeline().tracks.len(), 2);
    }

    #[test]
    fn two_video_tracks_may_play_one_resource() {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        let clip = project.add_resource(ResourceKind::Video, "clip", "clip.mp4", None);
        let a = project.add_track_for(clip, 0.0).unwrap();
        let b = project.add_track_for(clip, 0.0).unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("clip", "clip_2"));
        let timeline = project.timeline();
        assert_eq!(timeline.tracks.len(), 2);
        assert_eq!(timeline.tracks[0].path, timeline.tracks[1].path);
        // Removing the resource takes both tracks.
        assert_eq!(project.remove_resource(clip).len(), 2);
    }
}
