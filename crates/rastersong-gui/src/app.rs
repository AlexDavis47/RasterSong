//! The application window. All rendering goes through the engine; this only holds UI state.
//!
//! Layout: the Resources panel and the timeline along the bottom; above them the preview (with
//! its playback controls), the node graph and the inspector, side by side.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Ui, UiBuilder};
use rastersong_engine::LoadedTrack;
use rastersong_engine::playback::{MixTrack, Mixer};
use rastersong_engine::{
    AudioSink, BackendInfo, CompileOptions, Engine, EngineConfig, EngineStatus, Frame, Graph,
    GraphDesc, Item, ItemRef, MediaBackend, NodeStats, PROJECT_EXTENSION, PlaybackClock,
    PreviewScale, Project, ProjectTrack, Registry, ResourceId, ResourceKind, Thumbnails, Timeline,
    TimelineMode, TrackKind, TrackSpec, VIDEO_SOURCE, VideoInfo, VideoKey, resource_name_for,
};
use rastersong_lang::{tr, tr_args};

mod settings_window;

use crate::audio_out::AudioOut;

/// The sample rates offered for the project's rendered sound.
const AUDIO_RATES: [u32; 4] = [44_100, 48_000, 88_200, 96_000];

/// One track as playback mixes it: its name, items and gain.
/// A track as playback mixes it: name, items, level, and whether it is routed to the master bus.
type MixEntry = (String, Vec<Item>, f32, bool);
use crate::editor::{CanvasContext, GraphEditor, InspectorContext, without_layout};
use crate::history::History;
use crate::layer_inspector::{ItemEdit, graph_item_inspector};
use crate::preview::{Feed, PreviewView, clamp_split, split_rects, split_sides};
use crate::resources::{
    DraggedGraph, DraggedResource, PendingImport, ResourceAction, import_dialog, remove_dialog,
    resource_kind, resource_name, resources_panel,
};
use crate::settings::Settings;
use crate::theme::{Theme, ThemeChoice, apply_style};
use crate::timeline::{
    DropGhost, GhostTarget, GraphItemView, LANE_HEIGHT, LayerAction, LayerView, Thumbnail,
    TimelineModel, TimelineView, TrackAction, TrackThumbnails, TrackView, timecode, timeline,
};
use crate::track_ops::move_track;

/// The timeline row of the first audio track, if there is one.
fn first_audio_row(project: &Project) -> Option<usize> {
    (!project.audio_tracks.is_empty()).then_some(project.video_tracks.len())
}

/// The graph a new project starts with: the basic workflow from docs/concepts.md.
pub const STARTER_GRAPH: &str = include_str!("../../../examples/graphs/am_bands.json");

/// What the source engine renders: the video, untouched.
const SOURCE_GRAPH: &str = r#"{ "version": 0,
    "nodes": [ { "id": "video", "type": "video_input" }, { "id": "out", "type": "output" } ],
    "connections": [ { "from": "video", "to": "out" } ] }"#;

/// How long a graph item is when dropped on an empty project.
const DEFAULT_GRAPH_ITEM_SECS: f64 = 5.0;

/// What copying timeline items puts on the system clipboard. The items themselves stay in the app.
pub const ITEM_CLIPBOARD_MARKER: &str = "RasterSong timeline items";

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "avi", "webm", "m4v", "ts", "mts", "m2ts", "mpg",
];
const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "mp3", "flac", "ogg", "m4a", "aac", "opus", "aiff", "mp4", "mkv", "mov",
];
const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "avi", "webm", "m4v", "ts", "mts", "m2ts", "mpg", "wav", "mp3", "flac",
    "ogg", "m4a", "aac", "opus", "aiff",
];

/// Hearing a connection, which plays along with the transport.
struct Listening {
    target: rastersong_engine::ListenTarget,
}

/// Something that would throw away unsaved changes, waiting for the user to decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Close,
    NewProject,
    OpenProject,
}

/// The thumbnails of one video stream and their textures.
struct VideoThumbnails {
    service: Thumbnails,
    textures: BTreeMap<usize, (egui::TextureHandle, Thumbnail)>,
}

pub struct App {
    engine: Engine,
    /// For quick checks on media files (e.g. whether a video has sound).
    backend: Arc<dyn MediaBackend>,
    /// Thumbnails of each video stream the timeline's video tracks play: tracks of one
    /// resource share an entry.
    thumbnails: HashMap<VideoKey, VideoThumbnails>,
    /// The first video track's stream, which a changed timeline view starts over for.
    thumbnail_video: Option<VideoKey>,
    timeline_view: TimelineView,
    /// Where the timeline and the inspector were last drawn, for tests.
    timeline_area: egui::Rect,
    inspector_rect: egui::Rect,
    preview_rect: egui::Rect,
    audio: AudioOut,
    backend_info: Option<BackendInfo>,
    project: Project,
    project_path: Option<PathBuf>,
    /// The project as last saved or opened, to tell whether there are unsaved changes.
    saved: Project,
    history: History,
    /// An action waiting on "save changes?".
    confirm: Option<Pending>,
    /// The user chose to close without saving.
    allow_close: bool,
    editor: GraphEditor,
    /// The graph (without layout) the engine is rendering.
    sent_graph: GraphDesc,
    /// The graph and compile options last inspected, and what inspecting them found: every
    /// node's layouts, tags and warnings, including nodes that don't feed the output.
    inspected_for: Option<(GraphDesc, CompileOptions)>,
    inspected: Vec<NodeStats>,
    /// Mix last given to the audio output: (track, offset, gain) per track.
    /// What the playback mix was last built from: the render's audio, and the tracks' names,
    /// offsets and gains.
    sent_mix: Option<(AudioSink, Vec<MixEntry>)>,
    /// The connection being listened to with the Listen tool, and from when.
    listening: Option<Listening>,
    /// Scrolled distance not yet worth a step of the inspection view.
    inspect_scroll: f32,
    clock: PlaybackClock,
    /// Frame count and rate the clock was made for.
    clock_shape: Option<(usize, f64)>,
    settings: Settings,
    /// The theme last handed to egui.
    applied_theme: Option<ThemeChoice>,
    /// The selected row of the timeline: the video tracks, then the audio tracks.
    selected_track: Option<usize>,
    /// The selected timeline items.
    selected_items: Vec<ItemRef>,
    /// The selected graph item: its layer and its index among the layer's items. Exclusive with
    /// [`Self::selected_items`].
    selected_graph_item: Option<(usize, usize)>,
    /// The graph node that was active last frame, to tell when another is picked.
    seen_active_node: Option<u64>,
    /// Timeline items copied, with the names of their tracks.
    item_clipboard: Vec<(String, Item)>,
    /// When (egui time) the preview started waiting on a frame to render, if it is.
    waiting_since: Option<f64>,
    preview: Option<(egui::TextureHandle, Arc<Frame>)>,
    /// Renders the original video, the preview's second feed. It only has a video while the
    /// preview shows it, so it costs nothing otherwise.
    source_engine: Engine,
    /// The video the source engine was given.
    source_video_sent: Option<Timeline>,
    /// Resources whose files aren't there, checked about once a second.
    missing: HashSet<ResourceId>,
    missing_checked: Option<Instant>,
    source_preview: Option<(egui::TextureHandle, Arc<Frame>)>,
    preview_view: PreviewView,
    /// The feed the preview shows; with the split on, the one on the right.
    preview_feed: Feed,
    preview_split: bool,
    /// The split's position across the preview, 0..1.
    split_position: f32,
    error: Option<String>,
    show_about: bool,
    show_settings: bool,
    settings_tab: settings_window::SettingsTab,
    /// The output bus waiting on the "remove bus?" warning.
    bus_removal: Option<String>,
    /// Files with several streams, waiting on the import dialog in turn.
    pending_imports: VecDeque<PendingImport>,
    /// The resource waiting on the "remove resource?" warning, because tracks play it.
    resource_removal: Option<ResourceId>,
    initialized: bool,
    title: String,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("project", &self.project_path)
            .finish_non_exhaustive()
    }
}

impl App {
    pub fn new(
        backend: Arc<dyn MediaBackend>,
        mut project: Project,
        backend_info: Option<BackendInfo>,
        audio: AudioOut,
    ) -> Self {
        let engine = Engine::new(backend.clone(), EngineConfig::default());
        let source_engine = Engine::new(backend.clone(), EngineConfig::default());
        let editor = graph_editor(&project);
        // The editor fills in positions, full port names and a missing output; that isn't an
        // unsaved change.
        project.graph = editor.to_desc();
        let mut app = Self {
            engine,
            backend,
            thumbnails: HashMap::new(),
            thumbnail_video: None,
            timeline_view: TimelineView::default(),
            timeline_area: egui::Rect::NOTHING,
            inspector_rect: egui::Rect::NOTHING,
            preview_rect: egui::Rect::NOTHING,
            audio,
            backend_info,
            saved: project.clone(),
            history: History::new(project.clone()),
            confirm: None,
            allow_close: false,
            sent_graph: GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap(),
            inspected_for: None,
            inspected: Vec::new(),
            sent_mix: None,
            listening: None,
            inspect_scroll: 0.0,
            waiting_since: None,
            selected_track: first_audio_row(&project),
            selected_items: Vec::new(),
            selected_graph_item: None,
            seen_active_node: None,
            item_clipboard: Vec::new(),
            project,
            project_path: None,
            editor,
            clock: PlaybackClock::new(30.0, 0),
            clock_shape: None,
            settings: Settings::default(),
            applied_theme: None,
            preview: None,
            source_engine,
            source_video_sent: None,
            missing: HashSet::new(),
            missing_checked: None,
            source_preview: None,
            preview_view: PreviewView::default(),
            preview_feed: Feed::default(),
            preview_split: false,
            split_position: 0.5,
            error: None,
            show_about: false,
            show_settings: false,
            settings_tab: settings_window::SettingsTab::default(),
            bus_removal: None,
            pending_imports: VecDeque::new(),
            resource_removal: None,
            initialized: false,
            title: String::new(),
        };
        app.send_project();
        app.source_engine
            .set_graph(GraphDesc::from_json(SOURCE_GRAPH).expect("the source graph is valid"));
        app
    }

    pub fn with_starter_project(
        backend: Arc<dyn MediaBackend>,
        backend_info: Option<BackendInfo>,
        audio: AudioOut,
    ) -> Self {
        let graph = GraphDesc::from_json(STARTER_GRAPH).expect("the starter graph is valid");
        Self::new(backend, Project::new(graph), backend_info, audio)
    }

    pub fn clock(&self) -> &PlaybackClock {
        &self.clock
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn editor(&self) -> &GraphEditor {
        &self.editor
    }

    /// The selected timeline items.
    pub fn selected_items(&self) -> &[ItemRef] {
        &self.selected_items
    }

    pub fn timeline_view(&self) -> TimelineView {
        self.timeline_view
    }

    /// Where the timeline (ruler, headers and lanes) was last drawn.
    pub fn timeline_area(&self) -> egui::Rect {
        self.timeline_area
    }

    /// Where the inspector panel was last drawn.
    pub fn inspector_rect(&self) -> egui::Rect {
        self.inspector_rect
    }

    /// Where the preview's frame was last painted.
    pub fn preview_rect(&self) -> egui::Rect {
        self.preview_rect
    }

    /// With the split on, where the divider sits (a fraction of the preview's width) and the
    /// feeds shown (left, right); `None` with it off.
    pub fn preview_split(&self) -> Option<(f32, (Feed, Feed))> {
        self.preview_split
            .then(|| (self.split_position, split_sides(self.preview_feed)))
    }

    /// Each video track's file length in seconds, once it is known.
    pub fn video_durations(&self) -> Vec<Option<f64>> {
        let tracks = &self.project.video_tracks;
        tracks.iter().map(|t| self.track_duration(t)).collect()
    }

    /// How many video thumbnails are ready to draw.
    pub fn thumbnail_count(&self) -> usize {
        self.thumbnails.values().map(|t| t.textures.len()).sum()
    }

    /// Whether the sound of a connection is being listened to (Shift held over it).
    pub fn is_listening(&self) -> bool {
        self.listening.is_some()
    }

    /// The user's settings, saved between sessions.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
        self.settings.apply_language();
        self.engine.set_preview_scale(self.settings.preview_scale());
        self.engine.set_config(self.settings.engine_config());
    }

    /// Hands the whole project to the engine.
    fn send_project(&mut self) {
        self.engine.set_preview_scale(self.settings.preview_scale());
        self.refresh_missing(true);
        self.engine.set_timeline(self.timeline_of(&self.project));
        self.engine.set_layers(self.project.layer_set());
        self.sent_graph = without_layout(&self.project.graph);
        self.engine.set_graph(self.sent_graph.clone());
        self.engine.set_tempo(self.project.tempo);
        self.engine.set_bypass_all(self.project.plays_track_mix());
        self.sent_mix = None;
    }

    fn load_project(&mut self, mut project: Project, path: Option<PathBuf>) {
        self.editor = graph_editor(&project);
        if !self.editor.warnings.is_empty() {
            self.error = Some(self.editor.warnings.join("\n"));
        }
        // The editor fills in positions and full port names; that isn't an unsaved change.
        project.graph = self.editor.to_desc();
        self.saved = project.clone();
        self.history = History::new(project.clone());
        self.selected_track = first_audio_row(&project);
        self.selected_graph_item = None;
        self.project = project;
        self.project_path = path;
        self.clock = PlaybackClock::new(30.0, 0);
        self.clock_shape = None;
        self.preview = None;
        self.send_project();
    }

    pub fn open_project(&mut self, path: &Path) {
        match Project::load(path) {
            Ok(project) => self.load_project(project, Some(path.to_owned())),
            Err(e) => self.error = Some(e),
        }
    }

    /// Opens a video: its best video stream replaces the video track, as a resource named
    /// after the file. If it has sound of its own, that's added as an audio track too (unless a
    /// track already plays it).
    pub fn open_video(&mut self, path: PathBuf) {
        let has_audio = self.backend.has_audio(&path);
        self.project.video_tracks.clear();
        let name = resource_name_for(&path, ResourceKind::Video);
        let id = self
            .project
            .add_resource(ResourceKind::Video, &name, &path, None);
        self.project.add_track_for(id, 0.0);
        self.engine.set_timeline(self.timeline_of(&self.project));
        self.clock.seek(0);
        let known = self.project.audio_tracks.iter().any(|t| {
            self.project
                .track_resource(t)
                .is_some_and(|r| r.path == path && r.stream.is_none())
        });
        if has_audio && !known {
            self.add_audio_tracks([path]);
        }
    }

    /// Adds an audio track for each file's best audio stream, through a resource named after
    /// the file.
    pub fn add_audio_tracks(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            let name = resource_name_for(&path, ResourceKind::Audio);
            let id = self
                .project
                .add_resource(ResourceKind::Audio, &name, &path, None);
            self.add_resource_track(id, 0.0);
        }
    }

    /// Imports files into the Resources panel. A file with one video or audio stream becomes a
    /// resource at once; a file with several waits on the import dialog to choose them.
    pub fn import_files(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut problems = Vec::new();
        for path in paths {
            match self.backend.streams(&path) {
                Ok(streams) if streams.is_empty() => problems.push(tr_args(
                    "error.import.no_streams",
                    &[("path", &path.display().to_string())],
                )),
                Ok(streams) if streams.len() == 1 => {
                    self.add_stream_resource(&path, &streams[0]);
                }
                Ok(streams) => self
                    .pending_imports
                    .push_back(PendingImport::new(path, streams)),
                Err(e) => problems.push(e.to_string()),
            }
        }
        if !problems.is_empty() {
            self.error = Some(problems.join("\n"));
        }
    }

    fn add_stream_resource(&mut self, path: &Path, stream: &rastersong_engine::StreamInfo) {
        self.project.add_resource(
            resource_kind(stream),
            &resource_name(path, stream),
            path,
            Some(stream.index),
        );
    }

    /// The file waiting on the import dialog, if any.
    pub fn pending_import(&self) -> Option<&PendingImport> {
        self.pending_imports.front()
    }

    /// Answers the import dialog for the file waiting on it: `chosen` says, stream by stream,
    /// which become resources. `None` cancels the file's import.
    pub fn finish_import(&mut self, chosen: Option<&[bool]>) {
        let Some(pending) = self.pending_imports.pop_front() else {
            return;
        };
        let Some(chosen) = chosen else { return };
        for ((stream, _), &chosen) in pending.streams.iter().zip(chosen) {
            if chosen {
                self.add_stream_resource(&pending.path, stream);
            }
        }
    }

    /// Adds a track playing resource `id` from `position` seconds, named after it. Returns the
    /// track's name.
    pub fn add_resource_track(&mut self, id: ResourceId, position: f64) -> Option<String> {
        let kind = self.project.resource(id)?.kind;
        let name = self.project.add_track_for(id, position)?;
        if kind == ResourceKind::Audio {
            self.selected_track =
                Some(self.project.video_tracks.len() + self.project.audio_tracks.len() - 1);
        }
        Some(name)
    }

    /// Adds an empty track of `kind` to drag a resource onto, and selects it. Returns its name.
    pub fn add_empty_track(&mut self, kind: TrackKind) -> String {
        let name = self.project.add_empty_track(kind);
        let videos = self.project.video_tracks.len();
        self.selected_track = Some(match kind {
            TrackKind::Video => videos - 1,
            TrackKind::Audio => videos + self.project.audio_tracks.len() - 1,
        });
        name
    }

    /// The track in timeline row `row`, if it would take resource `id` dropped on it: an empty
    /// track of its kind, or a track already playing it. The drop preview and the drop both ask
    /// this.
    fn takes_resource(&self, id: ResourceId, row: usize) -> Option<&ProjectTrack> {
        let track = match self.track_row(row)? {
            (TrackKind::Video, i) => self.project.video_tracks.get(i)?,
            (TrackKind::Audio, i) => self.project.audio_tracks.get(i)?,
        };
        self.project
            .can_place_resource(&track.name, id)
            .then_some(track)
    }

    /// Drops resource `id` on the track in timeline row `row` at `position` seconds: an empty
    /// track takes it, a track of the same resource gets another item. Any other track can't
    /// take it, so a new track plays it instead. Returns the name of the track it landed on.
    pub fn drop_resource(
        &mut self,
        id: ResourceId,
        row: Option<usize>,
        position: f64,
    ) -> Option<String> {
        let target = row
            .and_then(|row| self.takes_resource(id, row))
            .map(|t| t.name.clone());
        let Some(name) = target else {
            return self.add_resource_track(id, position);
        };
        self.project.place_resource(&name, id, position);
        Some(name)
    }

    /// Adds a passthrough graph and opens it. Returns its id.
    pub fn new_graph(&mut self) -> u32 {
        self.project.graph = self.editor.to_desc();
        let id = self.project.add_graph("Graph", None);
        self.open_graph(id);
        id
    }

    /// Opens a graph from the Resources panel in the editor. It is the graph rendered, too:
    /// there is one at a time until graph layers can place several.
    pub fn open_graph(&mut self, id: u32) {
        self.project.graph = self.editor.to_desc();
        if !self.project.open_graph(id) {
            return;
        }
        self.editor = graph_editor(&self.project);
        if !self.editor.warnings.is_empty() {
            self.error = Some(self.editor.warnings.join("\n"));
        }
        self.project.graph = self.editor.to_desc();
    }

    /// Points the resource's file (and the other streams of it) at `path`.
    pub fn relocate_resource(&mut self, id: ResourceId, path: PathBuf) {
        if self.project.relocate_resource(id, path) > 0 {
            self.refresh_missing(true);
        }
    }

    /// What the engine plays: the tracks whose files are there.
    fn timeline_of(&self, project: &Project) -> Timeline {
        project.timeline_with(|r| !self.missing.contains(&r.id))
    }

    /// Looks for resources whose files are gone, at most once a second unless `now`.
    fn refresh_missing(&mut self, now: bool) {
        if !now
            && self
                .missing_checked
                .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
        {
            return;
        }
        self.missing_checked = Some(Instant::now());
        self.missing = self
            .project
            .resources
            .iter()
            .filter(|r| !self.backend.exists(&r.path))
            .map(|r| r.id)
            .collect();
    }

    /// The ids of resources whose files are missing.
    pub fn missing_resources(&self) -> &HashSet<ResourceId> {
        &self.missing
    }

    /// Removes a resource, the tracks that play it, and their links.
    pub fn remove_resource(&mut self, id: ResourceId) {
        let removed = self.project.remove_resource(id);
        for name in &removed {
            self.project.unlink_track(name);
        }
        self.selected_items.retain(|r| !removed.contains(&r.track));
        let rows = self.project.video_tracks.len() + self.project.audio_tracks.len();
        self.selected_track = self
            .selected_track
            .filter(|&s| s < rows)
            .or(first_audio_row(&self.project));
    }

    fn resource_action(&mut self, action: ResourceAction) {
        match action {
            ResourceAction::Import => self.pick_media(),
            ResourceAction::AddToTimeline(id) => {
                let at = self.clock.frame() as f64 / self.clock_rate();
                self.add_resource_track(id, at);
            }
            ResourceAction::NewGraph => {
                self.new_graph();
            }
            ResourceAction::OpenGraph(id) => self.open_graph(id),
            ResourceAction::RenameGraph(id, name) => {
                self.project.rename_graph(id, &name);
            }
            ResourceAction::DuplicateGraph(id) => {
                // The open graph is copied as it is in the editor now.
                self.project.graph = self.editor.to_desc();
                self.project.duplicate_graph(id);
            }
            ResourceAction::RemoveGraph(id) => {
                self.project.remove_graph(id);
            }
            ResourceAction::Relocate(id) => {
                let Some(old) = self.project.resource(id).map(|r| r.path.clone()) else {
                    return;
                };
                let mut dialog = rfd::FileDialog::new();
                if let Some(folder) = old.parent().filter(|p| p.exists()) {
                    dialog = dialog.set_directory(folder);
                }
                if let Some(file) = old.file_name() {
                    dialog = dialog.set_file_name(file.to_string_lossy());
                }
                if let Some(path) = dialog.pick_file() {
                    self.relocate_resource(id, path);
                }
            }
            ResourceAction::Rename(id, name) => {
                self.project.rename_resource(id, &name);
            }
            ResourceAction::Remove(id) => {
                if self.project.resource_users(id).is_empty() {
                    self.remove_resource(id);
                } else {
                    self.resource_removal = Some(id);
                }
            }
        }
    }

    /// Frames per second of the playhead's clock, for turning its frame into seconds.
    fn clock_rate(&self) -> f64 {
        self.engine
            .info()
            .map_or(30.0, |info| info.frame_rate.as_f64())
            .max(1e-6)
    }

    /// The import and "remove resource?" dialogs, while they wait.
    fn resource_dialogs(&mut self, ui: &Ui) {
        if let Some(pending) = self.pending_imports.front_mut() {
            match import_dialog(ui.ctx(), pending) {
                Some(true) => {
                    let chosen: Vec<bool> = pending.streams.iter().map(|(_, c)| *c).collect();
                    self.finish_import(Some(&chosen));
                }
                Some(false) => self.finish_import(None),
                None => {}
            }
        }
        if let Some(id) = self.resource_removal {
            let Some(resource) = self.project.resource(id) else {
                self.resource_removal = None;
                return;
            };
            let name = resource.name.clone();
            let users = self.project.resource_users(id);
            match remove_dialog(ui.ctx(), &name, &users) {
                Some(true) => {
                    self.resource_removal = None;
                    self.remove_resource(id);
                }
                Some(false) => self.resource_removal = None,
                None => {}
            }
        }
    }

    fn save(&mut self, choose_path: bool) {
        let path = match (&self.project_path, choose_path) {
            (Some(path), false) => Some(path.clone()),
            _ => rfd::FileDialog::new()
                .add_filter(tr("dialog.filter.project"), &[PROJECT_EXTENSION])
                .set_file_name(format!("untitled.{PROJECT_EXTENSION}"))
                .save_file(),
        };
        let Some(path) = path else { return };
        match self.project.save(&path) {
            Ok(()) => {
                self.saved = self.project.clone();
                self.project_path = Some(path);
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Asks to save unsaved changes before `action`, or does it straight away if there are none.
    fn request(&mut self, ui: &Ui, action: Pending) {
        if self.is_dirty() {
            self.confirm = Some(action);
        } else {
            self.perform(ui, action);
        }
    }

    fn perform(&mut self, ui: &Ui, action: Pending) {
        match action {
            Pending::Close => {
                self.allow_close = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Pending::NewProject => {
                let graph = GraphDesc::from_json(STARTER_GRAPH).unwrap();
                self.load_project(Project::new(graph), None);
            }
            Pending::OpenProject => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter(tr("dialog.filter.project"), &[PROJECT_EXTENSION])
                    .pick_file()
                {
                    self.open_project(&path);
                }
            }
        }
    }

    /// The "save changes?" dialog, while an action waits on it.
    fn confirm_dialog(&mut self, ui: &Ui) {
        let Some(action) = self.confirm else { return };
        let name = self.project_name();
        let mut choice = None;
        egui::Modal::new(egui::Id::new("save-changes")).show(ui.ctx(), |ui| {
            ui.set_width(340.0);
            ui.heading(tr_args("dialog.save_changes.title", &[("name", &name)]));
            ui.add_space(4.0);
            ui.label(tr("dialog.save_changes.body"));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button(tr("dialog.save")).clicked() {
                    choice = Some(true);
                }
                if ui.button(tr("dialog.dont_save")).clicked() {
                    choice = Some(false);
                }
                if ui.button(tr("dialog.cancel")).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    self.confirm = None;
                }
            });
        });
        match choice {
            Some(true) => {
                self.confirm = None;
                self.save(false);
                // Saving can be cancelled (or fail); then the action doesn't happen.
                if !self.is_dirty() {
                    self.perform(ui, action);
                }
            }
            Some(false) => {
                self.confirm = None;
                self.perform(ui, action);
            }
            None => {}
        }
    }

    /// Whether an action is waiting on "save changes?".
    pub fn is_confirming(&self) -> bool {
        self.confirm.is_some()
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo(&mut self) {
        if let Some(project) = self.history.undo().cloned() {
            self.restore(project);
        }
    }

    pub fn redo(&mut self) {
        if let Some(project) = self.history.redo().cloned() {
            self.restore(project);
        }
    }

    /// Puts the project back to an earlier (or later) state from the history.
    fn restore(&mut self, project: Project) {
        self.engine.set_timeline(self.timeline_of(&project));
        self.engine.set_layers(project.layer_set());
        self.editor.restore(&project.graph);
        let rows = project.video_tracks.len() + project.audio_tracks.len();
        self.selected_track = self
            .selected_track
            .filter(|&i| i < rows)
            .or(first_audio_row(&project));
        self.project = project;
    }

    /// Records the project in the undo history once the user has finished a gesture: no button
    /// held and no text being typed, so a drag or a typed name is one step.
    fn record_history(&mut self, ui: &Ui) {
        let busy = ui.input(|i| i.pointer.any_down()) || ui.ctx().egui_wants_keyboard_input();
        if !busy {
            self.history.record(&self.project);
        }
    }

    /// Whether the project has changed since it was last saved or opened.
    pub fn is_dirty(&self) -> bool {
        self.project != self.saved
    }

    /// The root of the UI.
    pub fn ui(&mut self, ui: &mut Ui) {
        if !self.initialized {
            let ctx = ui.ctx().clone();
            self.engine.on_update({
                let ctx = ctx.clone();
                move || ctx.request_repaint()
            });
            self.source_engine.on_update({
                let ctx = ctx.clone();
                move || ctx.request_repaint()
            });
            apply_style(ui.ctx());
            self.initialized = true;
        }
        if self.applied_theme != Some(self.settings.theme) {
            ui.ctx().set_theme(self.settings.theme.preference());
            self.applied_theme = Some(self.settings.theme);
        }
        if ui.input(|i| i.viewport().close_requested()) && !self.allow_close && self.is_dirty() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Close);
        }
        self.shortcuts(ui);
        self.update_inspection();

        let fill = ui.visuals().panel_fill;
        let panel = move |margin: i8| {
            egui::Frame::new()
                .fill(fill)
                .inner_margin(Margin::same(margin))
        };
        egui::Panel::top("menu")
            .frame(panel(4))
            .show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("timeline")
            .resizable(true)
            .default_size(240.0)
            .min_size(130.0)
            .frame(panel(8))
            .show(ui, |ui| {
                egui::Panel::left("resources")
                    .resizable(true)
                    .default_size(200.0)
                    .min_size(140.0)
                    .frame(egui::Frame::new().inner_margin(Margin {
                        right: 8,
                        ..Margin::ZERO
                    }))
                    .show(ui, |ui| {
                        for action in resources_panel(ui, &self.project, &self.missing) {
                            self.resource_action(action);
                        }
                    });
                self.timeline(ui);
            });
        egui::Panel::left("preview")
            .resizable(true)
            .default_size(ui.available_width() * 0.33)
            .min_size(280.0)
            .frame(panel(8))
            .show(ui, |ui| self.preview_column(ui));
        // Picking a different node in the graph brings its inspector back over a graph item's.
        let active = self.editor.active().map(|n| n.key);
        if active != self.seen_active_node {
            if active.is_some() {
                self.selected_graph_item = None;
            }
            self.seen_active_node = active;
        }
        let mut item_edits = Vec::new();
        self.inspector_rect = egui::Panel::right("inspector")
            .resizable(true)
            .default_size(340.0)
            .min_size(300.0)
            .frame(panel(10))
            .show(ui, |ui| {
                if let Some((layer, item)) = self.selected_graph_item {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        item_edits = graph_item_inspector(ui, &self.project, layer, item);
                    });
                    return;
                }
                let buses: Vec<String> =
                    self.project.buses.iter().map(|b| b.name.clone()).collect();
                let frame = self.engine.frame(self.clock.frame());
                let params = frame.as_ref().map_or(&[][..], |f| &f.params[..]);
                let meters = frame.as_ref().map_or(&[][..], |f| &f.meters[..]);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.editor.show_inspector(
                        ui,
                        &InspectorContext {
                            buses: &buses,
                            params,
                            meters,
                        },
                    );
                });
            })
            .response
            .rect;
        for edit in item_edits {
            self.graph_item_edit(edit);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(Margin::ZERO))
            .show(ui, |ui| self.graph(ui));

        self.sync();
        self.record_history(ui);
        self.tick(ui);
        self.import_dropped_files(ui);
        self.windows(ui);
        self.resource_dialogs(ui);
        self.confirm_dialog(ui);
        self.update_title(ui);
        // Last, once every drop target has had its chance at a dragged payload.
        crate::widgets::paint_bubble(ui.ctx());
    }

    fn graph(&mut self, ui: &mut Ui) {
        self.editor.keep_connections = self.settings.keep_connections;
        self.editor.max_warmup_frames = self.project.max_warmup_frames;
        let frame = self.engine.frame(self.clock.frame());
        let failure = match self.engine.status() {
            // The editor shows the open graph: a node of another graph isn't one it can point at.
            EngineStatus::Failed(mut failure) => {
                if failure.graph.is_some_and(|g| g != self.project.graph_id) {
                    failure.node = None;
                }
                Some(failure)
            }
            _ => None,
        };
        let levels = frame.as_ref().map_or(&[][..], |f| &f.levels[..]);
        let params = frame.as_ref().map_or(&[][..], |f| &f.params[..]);
        let costs = frame.as_ref().map_or(&[][..], |f| &f.costs[..]);
        self.update_inspect_view(ui);
        let engine = &self.engine;
        let tap = |request: &rastersong_engine::TapRequest| engine.tap(request);
        let canvas = self.editor.show(
            ui,
            &CanvasContext {
                levels,
                params,
                failure: failure.as_ref(),
                wire_style: self.settings.wire_style,
                show_stats: self.settings.node_stats,
                costs,
                show_performance: self.settings.show_performance,
                inspect: Some(crate::editor::InspectContext {
                    frame: self.clock.frame(),
                    tap: &tap,
                    refresh: 1.0 / self.project.inspect_rate.max(0.1),
                    views: self.settings.views,
                }),
            },
        );
        self.update_listening(ui);
        self.bypass_all_button(ui, canvas.rect);
    }

    /// Holding Alt over a connection turns the wheel into a way to change the view.
    fn update_inspect_view(&mut self, ui: &Ui) {
        let inspecting = self.editor.hovered_output().is_some() && crate::editor::changing_view(ui);
        self.editor.scroll_reserved = inspecting;
        if !inspecting {
            self.inspect_scroll = 0.0;
            return;
        }
        let wheel: Vec<_> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::MouseWheel { unit, delta, .. } => Some((*unit, delta.y)),
                    _ => None,
                })
                .collect()
        });
        let steps = crate::editor::scroll_steps(wheel, &mut self.inspect_scroll);
        if steps != 0 {
            let audio = self.editor.hovered_is_audio();
            self.settings.views.step(audio, steps);
        }
    }

    /// Starts, moves and stops the sound of the connection under the pointer while Shift is
    /// held.
    fn update_listening(&mut self, ui: &Ui) {
        let fps = self.clock_shape.map_or(30.0, |s| s.1);
        let wanted = crate::editor::listening(ui)
            .then(|| self.editor.hovered_output())
            .flatten()
            .map(|(node, output)| rastersong_engine::ListenTarget {
                node: node.to_owned(),
                output,
            });
        // The sound follows the playhead; the audio thread is silent while it is stopped.
        let now = self.clock.position() / fps;
        match (self.listening.as_ref().map(|l| &l.target), wanted) {
            (_, Some(target)) if self.listening.as_ref().is_none_or(|l| l.target != target) => {
                self.audio
                    .listen(Mixer::rendered(self.engine.listened_audio(), 1.0));
                self.listening = Some(Listening { target });
            }
            (Some(_), None) => {
                self.audio.stop_listening();
                self.engine.listen(None, self.clock.frame());
                self.listening = None;
            }
            _ => {}
        }
        if let Some(l) = &self.listening {
            self.engine
                .listen(Some(l.target.clone()), (now.max(0.0) * fps) as usize);
            if self.clock.is_playing() {
                ui.ctx().request_repaint();
            }
        }
    }

    /// The toggle in the canvas's corner that skips the whole graph.
    fn bypass_all_button(&mut self, ui: &mut Ui, canvas: egui::Rect) {
        let id = ui.id().with("bypass-all");
        egui::Area::new(id)
            .order(egui::Order::Foreground)
            .fixed_pos(canvas.right_top() + egui::vec2(-8.0, 8.0))
            .pivot(egui::Align2::RIGHT_TOP)
            .show(ui.ctx(), |ui| {
                let on = self.project.bypass_graph;
                let response = ui
                    .selectable_label(on, tr("editor.bypass_graph"))
                    .on_hover_text(tr("editor.bypass_graph.help"));
                if response.clicked() {
                    self.project.bypass_graph = !on;
                }
            });
    }

    /// Whether the preview needs the original video.
    fn wants_source(&self) -> bool {
        self.preview_split || self.preview_feed == Feed::Unprocessed
    }

    /// Gives the source engine the video while the preview shows it, and takes it away after.
    fn sync_source_engine(&mut self) {
        // The video alone, as the track the source graph reads.
        let video = self
            .wants_source()
            .then(|| self.project.video())
            .flatten()
            .map(|video| Timeline {
                timebase: self.project.timebase,
                tracks: vec![TrackSpec {
                    items: video.items.clone(),
                    stream: self.project.track_resource(video).and_then(|r| r.stream),
                    ..TrackSpec::new(
                        VIDEO_SOURCE,
                        TrackKind::Video,
                        self.project.track_path(video).unwrap_or(Path::new("")),
                    )
                }],
                ..Timeline::default()
            });
        if self.source_video_sent != video {
            self.source_engine
                .set_timeline(video.clone().unwrap_or_default());
            self.source_video_sent = video;
            if self.source_video_sent.is_none() {
                self.source_preview = None;
            }
        }
        self.source_engine
            .set_preview_scale(self.settings.preview_scale());
    }

    /// Gives the editor every node's layouts, tags and warnings (for wire colours, ports and the
    /// inspector), with latency and warmup from the engine's compiled graph. The graph is
    /// inspected again only when it or what it's compiled against changes.
    fn update_inspection(&mut self) {
        let stats = self.engine.node_stats();
        let Some(options) = self.engine.compile_options() else {
            self.editor.set_compiled(stats);
            return;
        };
        let current = self
            .inspected_for
            .as_ref()
            .is_some_and(|(graph, o)| *graph == self.sent_graph && *o == options);
        if !current {
            self.inspected = Graph::inspect(&self.sent_graph, Registry::shared(), &options);
            self.inspected_for = Some((self.sent_graph.clone(), options));
        }
        let merged = self
            .inspected
            .iter()
            .map(|node| {
                let mut node = node.clone();
                if let Some(compiled) = stats.iter().find(|s| s.node == node.node) {
                    node.latency_frames = compiled.latency_frames;
                    node.warmup_frames = compiled.warmup_frames;
                }
                node
            })
            .collect();
        self.editor.set_compiled(merged);
    }

    /// Sends edits to the engine and the audio output.
    fn sync(&mut self) {
        let graph = self.editor.to_desc();
        let semantic = without_layout(&graph);
        if semantic != self.sent_graph {
            self.engine.set_graph(semantic.clone());
            self.sent_graph = semantic;
        }
        self.project.graph = graph;
        self.refresh_missing(false);
        self.engine.set_timeline(self.timeline_of(&self.project));
        self.engine.set_layers(self.project.layer_set());
        self.engine.set_tempo(self.project.tempo);
        self.engine.set_bypass_all(self.project.plays_track_mix());
        self.engine.set_audio_rate(self.project.audio_rate);
        self.engine
            .set_max_warmup_frames(self.project.max_warmup_frames);
        self.sync_source_engine();

        // Rebuild the playback mix when tracks, offsets, levels or routing change, once decoded,
        // or when the graph starts or stops rendering its own sound. The preview plays the master
        // bus: its track mix is the tracks routed to it.
        let master = self.project.master_bus().to_owned();
        let soloing = self.project.soloing(TrackKind::Audio);
        let mix: Vec<MixEntry> = self
            .project
            .audio_tracks
            .iter()
            .map(|t| {
                (
                    t.name.clone(),
                    t.items.clone(),
                    t.mix_gain(TrackKind::Audio, soloing),
                    t.bus == master,
                )
            })
            .collect();
        let loaded = self.engine.loaded_tracks();
        let decoded = mix
            .iter()
            .all(|(name, ..)| loaded.iter().any(|l| &l.name == name));
        let sink = self.engine.audio_sink();
        let wanted = (sink, mix);
        if decoded && self.sent_mix.as_ref() != Some(&wanted) {
            let (sink, mix) = &wanted;
            let mixer = match sink {
                // The graph's sound replaces the tracks; their volumes and mutes don't apply.
                AudioSink::Rendered { .. } => Mixer::rendered(self.engine.rendered_audio(), 1.0),
                AudioSink::TrackMix | AudioSink::Passthrough(_) => {
                    Mixer::new(Self::mix_tracks(sink, mix, &loaded))
                }
            };
            self.audio.set_mixer(mixer);
            self.sent_mix = Some(wanted);
        }
    }

    /// The tracks playback mixes: the master bus's track mix, or just the track an audio output
    /// passes through.
    fn mix_tracks(sink: &AudioSink, mix: &[MixEntry], loaded: &[LoadedTrack]) -> Vec<MixTrack> {
        mix.iter()
            .filter(|(name, _, _, on_master)| match sink {
                AudioSink::Passthrough(track) => name == track,
                _ => *on_master,
            })
            .filter_map(|(name, items, gain, _)| {
                let clip = loaded.iter().find(|l| &l.name == name)?.clip.clone();
                Some(MixTrack {
                    clip,
                    items: items.clone(),
                    gain: *gain,
                })
            })
            .collect()
    }

    /// Advances playback and tells the engine and audio output where the playhead is.
    fn tick(&mut self, ui: &Ui) {
        if let Some(info) = self.engine.info() {
            let shape = (info.frames, info.frame_rate.as_f64());
            if self.clock_shape != Some(shape) {
                let frame = self.clock.frame();
                self.clock = PlaybackClock::new(shape.1, shape.0);
                self.clock.seek(frame);
                self.clock_shape = Some(shape);
            }
        }
        let fps = self.clock_shape.map_or(30.0, |s| s.1);
        let looping = self
            .project
            .loop_region
            .filter(|l| l.enabled)
            .and_then(|l| l.frames(fps));
        self.clock.set_loop(looping.clone());
        self.source_engine.set_loop(looping.clone());
        self.engine.set_loop(looping);
        let dt = f64::from(ui.input(|i| i.stable_dt)).min(0.1);
        let buffered = self.buffered_ahead();
        self.clock.advance(dt, buffered);
        self.engine.set_playhead(self.clock.frame());
        self.source_engine.set_playhead(self.clock.frame());
        self.audio.update(
            self.clock.position() / fps,
            self.clock.speed(),
            self.clock.is_playing(),
            self.settings.volume,
            // The tempo bar only shows in tempo mode, so the metronome only sounds there.
            (self.settings.metronome && self.project.timeline_mode == TimelineMode::Tempo)
                .then_some(self.project.tempo),
        );
        if self.clock.is_playing() {
            ui.ctx().request_repaint();
        }
    }

    /// Frames rendered from the playhead on, in playback order: when looping, on past the loop's
    /// end from its start.
    fn buffered_ahead(&self) -> usize {
        let frame = self.clock.frame();
        let buffered = self.engine.buffered_from(frame);
        match self.clock.active_loop() {
            Some(lp) if frame + buffered >= lp.end => {
                let wrapped = self.engine.buffered_from(lp.start).min(lp.len());
                lp.end.saturating_sub(frame) + wrapped
            }
            _ => buffered,
        }
    }

    /// Turns looping on or off, if there's a loop region.
    fn toggle_loop(&mut self) {
        if let Some(region) = &mut self.project.loop_region {
            region.enabled = !region.enabled;
        }
    }

    fn shortcuts(&mut self, ui: &Ui) {
        // Don't steal keys from text fields.
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (redo, undo) = ui.input_mut(|i| {
            (
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                ) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z),
            )
        });
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
            self.show_settings = true;
        }
        let (space, left, right, home, save, repeat) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::Home),
                i.modifiers.command && i.key_pressed(egui::Key::S),
                i.modifiers.is_none() && i.key_pressed(egui::Key::R),
            )
        });
        if repeat {
            self.toggle_loop();
        }
        if space {
            self.clock.toggle();
        }
        if left {
            self.clock.pause();
            self.clock.seek(self.clock.frame().saturating_sub(1));
        }
        if right {
            self.clock.pause();
            self.clock.seek(self.clock.frame() + 1);
        }
        if home {
            self.clock.seek(0);
        }
        if save {
            self.save(false);
        }
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tr("menu.file"), |ui| {
                if ui.button(tr("menu.file.new")).clicked() {
                    ui.close();
                    self.request(ui, Pending::NewProject);
                }
                if ui.button(tr("menu.file.open")).clicked() {
                    ui.close();
                    self.request(ui, Pending::OpenProject);
                }
                if ui.button(tr("menu.file.save")).clicked() {
                    ui.close();
                    self.save(false);
                }
                if ui.button(tr("menu.file.save_as")).clicked() {
                    ui.close();
                    self.save(true);
                }
                ui.separator();
                if ui.button(tr("menu.file.import_media")).clicked() {
                    ui.close();
                    self.pick_media();
                }
                if ui.button(tr("menu.file.open_video")).clicked() {
                    ui.close();
                    self.pick_video();
                }
                if ui.button(tr("menu.file.add_audio")).clicked() {
                    ui.close();
                    self.pick_audio_tracks();
                }
                ui.separator();
                if ui
                    .add(egui::Button::new(tr("menu.file.settings")).shortcut_text("Ctrl+,"))
                    .clicked()
                {
                    ui.close();
                    self.show_settings = true;
                }
                ui.separator();
                if ui.button(tr("menu.file.import_graph")).clicked() {
                    ui.close();
                    self.import_graph();
                }
                if ui.button(tr("menu.file.export_graph")).clicked() {
                    ui.close();
                    self.export_graph();
                }
            });
            ui.menu_button(tr("menu.edit"), |ui| self.edit_menu(ui));
            ui.menu_button(tr("menu.view"), |ui| {
                ui.checkbox(&mut self.settings.node_stats, tr("menu.view.node_stats"));
                ui.checkbox(
                    &mut self.settings.show_performance,
                    tr("menu.view.performance"),
                );
            });
            ui.menu_button(tr("menu.help"), |ui| {
                if ui.button(tr("menu.help.about")).clicked() {
                    self.show_about = true;
                    ui.close();
                }
            });
        });
    }

    fn edit_menu(&mut self, ui: &mut Ui) {
        let item = |ui: &mut Ui, enabled: bool, label: &str, keys: &str| {
            ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys))
                .clicked()
        };
        if item(ui, self.can_undo(), tr("menu.edit.undo"), "Ctrl+Z") {
            self.undo();
        }
        if item(ui, self.can_redo(), tr("menu.edit.redo"), "Ctrl+Shift+Z") {
            self.redo();
        }
        ui.separator();
        let selected = !self.editor.selected().is_empty();
        if item(ui, selected, tr("menu.edit.cut"), "Ctrl+X")
            && let Some(text) = self.editor.copy_selection()
        {
            ui.ctx().copy_text(text);
            self.editor.delete_selection();
        }
        if item(ui, selected, tr("menu.edit.copy"), "Ctrl+C")
            && let Some(text) = self.editor.copy_selection()
        {
            ui.ctx().copy_text(text);
        }
        let clipboard = self.editor.clipboard().map(str::to_owned);
        if item(ui, clipboard.is_some(), tr("menu.edit.paste"), "Ctrl+V")
            && let Some(text) = clipboard
        {
            let at = self.editor.view_center();
            self.editor.paste(&text, at, self.settings.keep_connections);
        }
        if item(ui, selected, tr("menu.edit.duplicate"), "Ctrl+D") {
            self.editor.duplicate_selection(false);
        }
        if item(ui, selected, tr("menu.edit.delete"), "Del") {
            self.editor.delete_selection();
        }
        ui.separator();
        if item(
            ui,
            self.editor.node_count() > 0,
            tr("menu.edit.select_all"),
            "Ctrl+A",
        ) {
            self.editor.select_all();
        }
    }

    fn pick_video(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(tr("dialog.filter.video"), VIDEO_EXTENSIONS)
            .pick_file()
        {
            self.open_video(path);
        }
    }

    fn pick_media(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter(tr("dialog.filter.media"), MEDIA_EXTENSIONS)
            .pick_files()
        {
            self.import_files(paths);
        }
    }

    /// Imports files dropped onto the window.
    fn import_dropped_files(&mut self, ui: &Ui) {
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !dropped.is_empty() {
            self.import_files(dropped);
        }
    }

    fn pick_audio_tracks(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter(tr("dialog.filter.audio"), AUDIO_EXTENSIONS)
            .pick_files()
        {
            self.add_audio_tracks(paths);
        }
    }

    fn import_graph(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(tr("dialog.filter.graph"), &["json"])
            .pick_file()
        else {
            return;
        };
        let graph = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|json| GraphDesc::from_json(&json).map_err(|e| e.to_string()));
        match graph {
            Ok(graph) => {
                self.editor.load(&graph);
                if !self.editor.warnings.is_empty() {
                    self.error = Some(self.editor.warnings.join("\n"));
                }
            }
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }

    fn export_graph(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(tr("dialog.filter.graph"), &["json"])
            .set_file_name("graph.json")
            .save_file()
        else {
            return;
        };
        if let Err(e) = std::fs::write(&path, self.project.graph.to_json()) {
            self.error = Some(format!("{}: {e}", path.display()));
        }
    }

    fn preview_column(&mut self, ui: &mut Ui) {
        let controls_height = 96.0;
        let size = egui::vec2(
            ui.available_width(),
            (ui.available_height() - controls_height).max(80.0),
        );
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        // Overlays (messages, spinners) go in a child, so they never move the controls below.
        let open_video = {
            let mut overlay = ui.new_child(UiBuilder::new().max_rect(rect));
            self.preview_image(&mut overlay, rect)
        };
        if open_video {
            self.pick_video();
        }
        ui.add_space(6.0);
        self.controls(ui);
    }

    /// Middle-drag pans the preview, the wheel zooms around the pointer, and F fits the frame.
    fn preview_input(&mut self, ui: &Ui, rect: egui::Rect) {
        let Some(pointer) = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
        else {
            return;
        };
        let (scroll, pinch, panning, delta) = ui.input(|i| {
            (
                i.smooth_scroll_delta.y,
                i.zoom_delta(),
                i.pointer.middle_down(),
                i.pointer.delta(),
            )
        });
        let factor = (scroll * 0.0015).exp() * pinch;
        if factor != 1.0 {
            self.preview_view.zoom_at(rect, pointer, factor);
        }
        if panning {
            self.preview_view.pan(delta);
        }
        if !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::F)) {
            self.preview_view.fit();
        }
    }

    /// Paints the frame (or, with the split on, the two feeds either side of a draggable divider)
    /// at the current pan and zoom.
    fn paint_preview(&mut self, ui: &mut Ui, rect: egui::Rect, theme: &Theme) {
        self.preview_rect = rect;
        let size = [&self.preview, &self.source_preview]
            .into_iter()
            .flatten()
            .map(|(_, f)| egui::vec2(f.width as f32, f.height as f32))
            .next();
        let Some(size) = size else { return };
        let shown = self.preview_view.frame_rect(rect, size);
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        let painter = ui.painter().with_clip_rect(rect);
        let draw = |painter: &egui::Painter, feed: Feed| {
            let slot = match feed {
                Feed::Processed => &self.preview,
                Feed::Unprocessed => &self.source_preview,
            };
            // Keep showing the last frame while the next renders.
            if let Some((texture, _)) = slot {
                painter.image(texture.id(), shown, uv, Color32::WHITE);
            }
        };
        if !self.preview_split {
            draw(&painter, self.preview_feed);
            return;
        }
        let (left, right) = split_sides(self.preview_feed);
        let (left_pane, right_pane) = split_rects(rect, self.split_position);
        draw(&painter.with_clip_rect(left_pane), left);
        draw(&painter.with_clip_rect(right_pane), right);

        let label = |feed: Feed, pos: egui::Pos2, align: egui::Align2| {
            let galley = painter.layout_no_wrap(
                feed.label().to_owned(),
                egui::FontId::proportional(12.0),
                Color32::WHITE,
            );
            let area = align.anchor_size(pos, galley.size());
            painter.rect_filled(
                area.expand(3.0),
                CornerRadius::same(3),
                Color32::from_black_alpha(150),
            );
            painter.galley(area.min, galley, Color32::WHITE);
        };
        label(
            left,
            left_pane.left_top() + egui::vec2(8.0, 6.0),
            egui::Align2::LEFT_TOP,
        );
        label(
            right,
            right_pane.right_top() + egui::vec2(-8.0, 6.0),
            egui::Align2::RIGHT_TOP,
        );

        // The divider and its handle.
        let x = left_pane.right();
        let grab = egui::Rect::from_min_max(
            egui::pos2(x - 7.0, rect.top()),
            egui::pos2(x + 7.0, rect.bottom()),
        );
        let response = ui
            .interact(grab, ui.id().with("preview-split"), egui::Sense::drag())
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        if response.dragged() {
            self.split_position =
                clamp_split(self.split_position + response.drag_delta().x / rect.width());
        }
        let active = response.hovered() || response.dragged();
        let line = Color32::WHITE.gamma_multiply(if active { 1.0 } else { 0.8 });
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(if active { 2.0 } else { 1.5 }, line),
        );
        let centre = egui::pos2(x, rect.center().y);
        painter.circle_filled(centre, 11.0, theme.accent);
        painter.circle_stroke(centre, 11.0, egui::Stroke::new(1.5, Color32::WHITE));
        for dx in [-3.0, 3.0] {
            painter.line_segment(
                [centre + egui::vec2(dx, -4.0), centre + egui::vec2(dx, 4.0)],
                egui::Stroke::new(1.5, Color32::WHITE),
            );
        }
    }

    /// Draws the preview into `rect`. Returns true if the user asked to open a video.
    fn preview_image(&mut self, ui: &mut Ui, rect: egui::Rect) -> bool {
        let theme = Theme::of(ui.ctx());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), theme.preview_bg);

        let ctx = ui.ctx().clone();
        if let Some(frame) = self.engine.frame(self.clock.frame()) {
            update_texture(&ctx, &mut self.preview, "preview", frame);
        }
        if self.wants_source()
            && let Some(frame) = self.source_engine.frame(self.clock.frame())
        {
            update_texture(&ctx, &mut self.source_preview, "preview-source", frame);
        }
        self.preview_input(ui, rect);
        self.paint_preview(ui, rect, theme);

        let message = |ui: &mut Ui, text: &str| {
            ui.put(
                egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width() - 40.0, 60.0)),
                egui::Label::new(RichText::new(text).color(theme.text_dim).size(15.0)).wrap(),
            );
        };
        let now = ui.input(|i| i.time);
        let waiting = matches!(self.engine.status(), EngineStatus::Ready)
            && self.engine.frame(self.clock.frame()).is_none();
        self.waiting_since = if waiting {
            Some(self.waiting_since.unwrap_or(now))
        } else {
            None
        };
        let mut open_video = false;
        match self.engine.status() {
            EngineStatus::Idle => {
                let area =
                    egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 80.0));
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(area)
                        .layout(egui::Layout::top_down(egui::Align::Center)),
                    |ui| {
                        ui.label(
                            RichText::new(tr("preview.empty"))
                                .color(theme.text_dim)
                                .size(15.0),
                        );
                        ui.add_space(4.0);
                        open_video = ui
                            .button(tr("menu.file.open_video"))
                            .on_hover_text(tr("preview.open_video.help"))
                            .clicked();
                    },
                );
            }
            EngineStatus::Loading => busy_centered(ui, rect, None),
            // The graph panel explains failures; the picture just stays as it was.
            EngineStatus::Failed(_) if self.preview.is_none() => {
                message(ui, tr("preview.graph_failed"))
            }
            EngineStatus::Ready if self.engine.frame(self.clock.frame()).is_none() => {
                let waited = self.waiting_since.map_or(0.0, |since| now - since);
                let warming = self.engine.progress().filter(|p| p.warming).map(|p| {
                    tr_args(
                        "preview.warming",
                        &[
                            ("done", &p.done.to_string()),
                            ("total", &p.total.to_string()),
                        ],
                    )
                });
                if self.preview.is_none() {
                    // Nothing to show yet: the spinner stays where "loading" put it.
                    busy_centered(ui, rect, warming.as_deref());
                } else if waited > BUSY_DELAY_SECS {
                    // The last frame stays up; say what's happening, but only once it's a wait
                    // rather than a flicker.
                    busy_badge(
                        ui,
                        rect,
                        warming.as_deref().unwrap_or(tr("preview.rendering")),
                        theme,
                    );
                }
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
            _ => {}
        }
        open_video
    }

    /// Playback controls under the preview.
    fn controls(&mut self, ui: &mut Ui) {
        let info = self.engine.info();
        ui.horizontal(|ui| {
            let label = if self.clock.is_playing() {
                "⏸"
            } else {
                "▶"
            };
            let play =
                egui::Button::new(RichText::new(label).size(16.0)).min_size(egui::vec2(34.0, 26.0));
            if ui
                .add_enabled(info.is_some(), play)
                .on_hover_text(tr("controls.play.help"))
                .clicked()
            {
                self.clock.toggle();
            }
            let looping = self.project.loop_region.is_some_and(|l| l.enabled);
            let loop_button = egui::Button::selectable(looping, tr("controls.loop"))
                .min_size(egui::vec2(0.0, 26.0));
            let hover = if self.project.loop_region.is_some() {
                tr("controls.loop.help")
            } else {
                tr("controls.loop.help_none")
            };
            if ui
                .add_enabled(self.project.loop_region.is_some(), loop_button)
                .on_hover_text(hover)
                .on_disabled_hover_text(hover)
                .clicked()
            {
                self.toggle_loop();
            }
            if let Some(info) = info {
                let fps = info.frame_rate.as_f64();
                let frame = self.clock.frame();
                ui.monospace(format!(
                    "{} / {}",
                    timecode(frame as f64 / fps),
                    timecode(info.frames as f64 / fps)
                ));
                ui.weak(tr_args("controls.frame", &[("frame", &frame.to_string())]));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.cache_status(ui, info.frames, fps);
                });
            }
        });
        ui.horizontal(|ui| {
            ui.label(tr("controls.preview"));
            let mut scale = self.settings.preview_scale();
            egui::ComboBox::from_id_salt("preview-scale")
                .selected_text(scale.label())
                .width(84.0)
                .show_ui(ui, |ui| {
                    for option in PreviewScale::ALL {
                        ui.selectable_value(&mut scale, option, option.label());
                    }
                })
                .response
                .on_hover_text(tr("controls.preview.help"));
            if scale != self.settings.preview_scale() {
                self.settings.set_preview_scale(scale);
                self.engine.set_preview_scale(scale);
            }
            let unprocessed = self.preview_feed == Feed::Unprocessed;
            if ui
                .add(egui::Button::selectable(
                    unprocessed,
                    tr("controls.unprocessed"),
                ))
                .on_hover_text(tr("controls.unprocessed.help"))
                .clicked()
            {
                self.preview_feed = self.preview_feed.other();
            }
            if ui
                .add(egui::Button::selectable(
                    self.preview_split,
                    tr("controls.split"),
                ))
                .on_hover_text(tr("controls.split.help"))
                .clicked()
            {
                self.preview_split = !self.preview_split;
            }
            if let (Some(project), Some(preview)) = (self.first_video_info(), info) {
                ui.weak(tr_args(
                    "controls.resolution",
                    &[
                        ("width", &preview.width.to_string()),
                        ("height", &preview.height.to_string()),
                        ("project_width", &project.width.to_string()),
                        ("project_height", &project.height.to_string()),
                    ],
                ))
                .on_hover_text(tr("controls.resolution.help"));
            }
        });
        ui.horizontal(|ui| {
            ui.label("🔊");
            ui.spacing_mut().slider_width = 110.0;
            let volume = self.settings.volume;
            ui.add(egui::Slider::new(&mut self.settings.volume, 0.0..=1.0).show_value(false))
                .on_hover_text(tr_args(
                    "controls.volume",
                    &[("percent", &format!("{:.0}", volume * 100.0))],
                ));
        });
    }

    /// What the render cache is doing, at the right of the controls: warming up after a seek, or
    /// how far ahead of the playhead frames are ready.
    fn cache_status(&self, ui: &mut Ui, frames: usize, fps: f64) {
        let theme = Theme::of(ui.ctx());
        if let Some(progress) = self.engine.progress().filter(|p| p.warming) {
            ui.colored_label(
                theme.warning,
                tr_args(
                    "preview.warming",
                    &[
                        ("done", &progress.done.to_string()),
                        ("total", &progress.total.to_string()),
                    ],
                ),
            )
            .on_hover_text(tr_args(
                "preview.warming.help",
                &[("frame", &progress.frame.to_string())],
            ));
            return;
        }
        let frame = self.clock.frame();
        let buffered_frames = self.buffered_ahead();
        let buffered = buffered_frames as f64 / fps;
        let speed = self.clock.speed();
        let to_end = frame + buffered_frames >= frames;
        let round_loop = self
            .clock
            .active_loop()
            .is_some_and(|lp| buffered_frames >= lp.end - frame.min(lp.start));
        let ahead = if round_loop {
            tr("cache.loop_rendered").to_owned()
        } else if to_end {
            tr("cache.rendered_to_end").to_owned()
        } else {
            tr_args("cache.ahead", &[("seconds", &format!("{buffered:.1}"))])
        };
        let (text, color) = if !self.clock.is_playing() || speed >= 1.0 {
            (ahead, ui.visuals().weak_text_color())
        } else {
            (
                tr_args(
                    "cache.slowed",
                    &[("speed", &format!("{speed:.2}")), ("ahead", &ahead)],
                ),
                theme.warning,
            )
        };
        ui.colored_label(color, text)
            .on_hover_text(tr("cache.help"));
    }

    fn timeline(&mut self, ui: &mut Ui) {
        let area = ui.available_rect_before_wrap();
        let Some(info) = self.engine.info() else {
            ui.weak(tr("timeline.empty"));
            if ui.button(tr("timeline.track.add_audio_file")).clicked() {
                self.pick_audio_tracks();
            }
            // A resource dropped on the empty timeline starts at the beginning.
            if let Some(id) = dropped_resource(ui, area) {
                self.add_resource_track(id, 0.0);
            }
            return;
        };
        self.tempo_bar(ui);
        self.tidy_item_selection();
        let loaded = self.engine.loaded_tracks();
        self.update_thumbnails(ui.ctx());
        let view = |t: &ProjectTrack, kind: TrackKind| TrackView {
            name: t.name.clone(),
            kind,
            items: t.items.clone(),
            muted: t.muted,
            solo: t.solo,
            silenced: t.mix_gain(kind, self.project.soloing(kind)) == 0.0,
            volume: t.volume,
            height: t.height.unwrap_or(LANE_HEIGHT),
            linked: self.project.linked_to(&t.name),
            bus: t.bus.clone(),
            missing: t.resource.is_some_and(|r| self.missing.contains(&r)),
            selected_items: self
                .selected_items
                .iter()
                .filter(|r| r.track == t.name)
                .map(|r| r.item)
                .collect(),
            ..TrackView::new(t.name.clone(), kind)
        };
        let mut tracks: Vec<TrackView> = self
            .project
            .video_tracks
            .iter()
            .map(|t| {
                let key = self.video_key(t);
                let own = key.as_ref().and_then(|k| self.thumbnails.get(k));
                let shown = own.and_then(|o| o.service.info());
                let shown = shown.as_ref();
                TrackView {
                    duration: if t.resource.is_none() {
                        Some(0.0)
                    } else {
                        shown.map(|v| v.frame_count as f64 / v.frame_rate.as_f64())
                    },
                    details: shown.map(|v| {
                        format!(
                            "{}×{} · {:.3} fps",
                            v.width,
                            v.height,
                            v.frame_rate.as_f64()
                        )
                        .replace(".000 fps", " fps")
                    }),
                    thumbnails: own.zip(shown).map(|(o, v)| TrackThumbnails {
                        frames: Arc::new(
                            o.textures
                                .iter()
                                .map(|(&frame, (_, thumbnail))| (frame, *thumbnail))
                                .collect(),
                        ),
                        rate: v.frame_rate.as_f64(),
                    }),
                    ..view(t, TrackKind::Video)
                }
            })
            .collect();
        tracks.extend(self.project.audio_tracks.iter().map(|t| {
            let loaded = loaded.iter().find(|l| l.name == t.name);
            TrackView {
                duration: if t.resource.is_none() {
                    Some(0.0)
                } else {
                    loaded.map(|l| l.clip.duration_secs())
                },
                waveform: loaded.map(|l| l.waveform.clone()),
                ..view(t, TrackKind::Audio)
            }
        }));
        let cached = self.engine.cached_ranges();
        let graph_names: HashMap<u32, String> = self
            .project
            .graph_entries()
            .into_iter()
            .map(|g| (g.id, g.name))
            .collect();
        let soloing_layers = self.project.soloing_layers();
        // The top layer first, as the timeline draws them.
        let layers: Vec<LayerView> = self
            .project
            .layers
            .iter()
            .enumerate()
            .rev()
            .map(|(index, l)| LayerView {
                index,
                name: l.name.clone(),
                muted: l.muted,
                solo: l.solo,
                silenced: l.muted || (soloing_layers && !l.solo),
                items: l
                    .items
                    .iter()
                    .map(|i| GraphItemView {
                        name: graph_names.get(&i.graph).cloned().unwrap_or_default(),
                        position: i.position,
                        length: i.length,
                        muted: i.muted,
                    })
                    .collect(),
                selected_item: self
                    .selected_graph_item
                    .filter(|&(layer, _)| layer == index)
                    .map(|(_, item)| item),
            })
            .collect();
        let model = TimelineModel {
            layers,
            frame_count: info.frames,
            frame_rate: info.frame_rate.as_f64(),
            playhead: self.clock.frame(),
            cached: &cached,
            tracks,
            selected_track: self.selected_track,
            loop_region: self.project.loop_region,
            tempo: self.project.tempo,
            mode: self.project.timeline_mode,
            buses: self.project.buses.iter().map(|b| b.name.clone()).collect(),
            snap: self.settings.snap,
            drop_ghost: self.drop_ghost(ui.ctx()),
        };
        self.timeline_area = ui.available_rect_before_wrap();
        let response = timeline(ui, &model, &mut self.timeline_view);
        // A drop lands where its ghost was drawn: at the snapped time, or (released over the
        // headers) at the start.
        let at = response.drop_at.unwrap_or(0.0);
        if let Some(id) = dropped_resource(ui, area) {
            self.drop_resource(id, response.row_under_pointer, at);
        }
        if let Some(graph) = dropped_graph(ui, area) {
            self.drop_graph(graph, response.layer_under_pointer, at);
        }
        for track in &self.project.video_tracks {
            let frames: Vec<usize> = response
                .wanted_thumbnails
                .iter()
                .filter(|(name, _)| *name == track.name)
                .map(|&(_, frame)| frame)
                .collect();
            if let Some(thumbs) = self.video_key(track).and_then(|k| self.thumbnails.get(&k)) {
                thumbs.service.request(&frames);
            }
        }
        if let Some(region) = response.loop_region {
            self.project.loop_region = region;
        }
        if response.toggle_snap {
            self.settings.snap = !self.settings.snap;
        }
        if response.toggle_mode {
            self.project.timeline_mode = match self.project.timeline_mode {
                TimelineMode::Time => TimelineMode::Tempo,
                TimelineMode::Tempo => TimelineMode::Time,
            };
        }
        if let Some(frame) = response.seek {
            self.clock.seek(frame);
        }
        for action in response.layer_actions {
            self.layer_action(action);
        }
        for action in response.actions {
            self.track_action(ui.ctx(), action);
        }
    }

    /// How long a graph item is when it is dropped: the whole project, or 5 seconds for an
    /// empty one.
    /// What the Resources panel is dragging over the timeline, for the drop preview.
    fn drop_ghost(&self, ctx: &egui::Context) -> Option<DropGhost> {
        if let Some(dragged) = egui::DragAndDrop::payload::<DraggedResource>(ctx) {
            let id = dragged.0;
            let resource = self.project.resource(id)?;
            let rows = self.project.video_tracks.len() + self.project.audio_tracks.len();
            // Known once a track plays it.
            let length = self
                .project
                .tracks()
                .find(|t| t.resource == Some(id))
                .and_then(|t| self.track_duration(t));
            return Some(DropGhost {
                name: resource.name.clone(),
                length,
                target: GhostTarget::Media {
                    kind: resource.kind.track_kind(),
                    lands_on: (0..rows)
                        .map(|row| self.takes_resource(id, row).is_some())
                        .collect(),
                },
            });
        }
        let graph = egui::DragAndDrop::payload::<DraggedGraph>(ctx)?.0;
        let name = self
            .project
            .graph_entries()
            .into_iter()
            .find(|g| g.id == graph)?
            .name;
        Some(DropGhost {
            name,
            length: Some(self.default_graph_item_length()),
            target: GhostTarget::Graph,
        })
    }

    fn default_graph_item_length(&self) -> f64 {
        let frames = self.engine.info().map_or(0, |info| info.frames);
        if frames == 0 {
            DEFAULT_GRAPH_ITEM_SECS
        } else {
            frames as f64 / self.clock_rate()
        }
    }

    /// Drops graph `graph` on the layer with project index `layer` at `position` seconds, for the
    /// project's length; off the layers (`None`) it makes a new layer on top first. Whatever it
    /// lands on is trimmed. Returns the layer and item index it landed at, and selects it.
    pub fn drop_graph(
        &mut self,
        graph: u32,
        layer: Option<usize>,
        position: f64,
    ) -> Option<(usize, usize)> {
        self.project.graph_desc(graph)?;
        let layer = match layer.filter(|&l| l < self.project.layers.len()) {
            Some(layer) => layer,
            None => self.project.add_layer(tr("timeline.layer.default_name")),
        };
        let length = self.default_graph_item_length();
        let item = self.project.place_graph(layer, graph, position, length)?;
        self.select_graph_item(layer, item);
        Some((layer, item))
    }

    fn select_graph_item(&mut self, layer: usize, item: usize) {
        self.selected_items.clear();
        self.selected_graph_item = Some((layer, item));
    }

    /// Applies a change from the graph item inspector to the selected item.
    fn graph_item_edit(&mut self, edit: ItemEdit) {
        let Some((layer, item)) = self.selected_graph_item else {
            return;
        };
        match edit {
            ItemEdit::PreRoll(on) => {
                if let Some(it) = self
                    .project
                    .layers
                    .get_mut(layer)
                    .and_then(|l| l.items.get_mut(item))
                {
                    it.pre_roll = on;
                }
            }
            ItemEdit::Bind { port, binding } => {
                self.project.set_graph_binding(layer, item, &port, binding);
            }
        }
    }

    fn layer_action(&mut self, action: LayerAction) {
        match action {
            LayerAction::Add => {
                self.project.add_layer(tr("timeline.layer.default_name"));
            }
            LayerAction::SelectItem { layer, item } => self.select_graph_item(layer, item),
            LayerAction::MoveItem { layer, item, delta } => {
                if let Some(item) = self.project.move_graph_item(layer, item, delta) {
                    self.select_graph_item(layer, item);
                }
            }
            LayerAction::TrimItem {
                layer,
                item,
                edge,
                to,
            } => self.project.trim_graph_item(layer, item, edge, to),
            LayerAction::ToggleItemMute { layer, item } => {
                if let Some(it) = self
                    .project
                    .layers
                    .get_mut(layer)
                    .and_then(|l| l.items.get_mut(item))
                {
                    it.muted = !it.muted;
                }
            }
            LayerAction::SplitItem { layer, item, at } => {
                self.project.split_graph_item(layer, item, at);
            }
            LayerAction::DeleteItem { layer, item } => self.delete_graph_item(layer, item),
            LayerAction::OpenItem { layer, item } => {
                let graph = self
                    .project
                    .layers
                    .get(layer)
                    .and_then(|l| l.items.get(item))
                    .map(|i| i.graph);
                if let Some(graph) = graph {
                    self.open_graph(graph);
                }
            }
            LayerAction::ToggleMute(layer) => {
                if let Some(l) = self.project.layers.get_mut(layer) {
                    l.muted = !l.muted;
                }
            }
            LayerAction::ToggleSolo(layer) => {
                if let Some(l) = self.project.layers.get_mut(layer) {
                    l.solo = !l.solo;
                }
            }
            LayerAction::Rename(layer, name) => {
                self.project.rename_layer(layer, &name);
            }
            LayerAction::Remove(layer) => {
                if self.project.remove_layer(layer) {
                    self.selected_graph_item = match self.selected_graph_item {
                        Some((l, _)) if l == layer => None,
                        Some((l, i)) if l > layer => Some((l - 1, i)),
                        other => other,
                    };
                }
            }
        }
    }

    /// Removes a graph item, keeping the selection on what it was on.
    fn delete_graph_item(&mut self, layer: usize, item: usize) {
        if !self.project.delete_graph_item(layer, item) {
            return;
        }
        self.selected_graph_item = match self.selected_graph_item {
            Some((l, i)) if l == layer && i == item => None,
            Some((l, i)) if l == layer && i > item => Some((l, i - 1)),
            other => other,
        };
    }

    /// In tempo mode, the tempo of the music that beat and bar units follow. Time mode shows
    /// nothing: the mode button lives in the timeline's ruler header.
    fn tempo_bar(&mut self, ui: &mut Ui) {
        if self.project.timeline_mode != TimelineMode::Tempo {
            return;
        }
        let tempo = &mut self.project.tempo;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            settings_window::tempo_fields(ui, tempo);
            ui.toggle_value(&mut self.settings.metronome, tr("tempo.metronome"))
                .on_hover_text(tr("tempo.metronome.help"));
        });
    }

    /// The stream of a file a video track plays, which thumbnails are made of.
    fn video_key(&self, track: &ProjectTrack) -> Option<VideoKey> {
        let resource = self.project.track_resource(track)?;
        Some((resource.path.clone(), resource.stream))
    }

    /// The first video track's file, which the project's picture size follows.
    fn first_video_info(&self) -> Option<VideoInfo> {
        let key = self.video_key(self.project.video()?)?;
        self.thumbnails.get(&key)?.service.info()
    }

    /// Keeps a thumbnail service on each video stream the timeline plays, and a texture for each
    /// thumbnail it has decoded.
    fn update_thumbnails(&mut self, ctx: &egui::Context) {
        let keys: Vec<VideoKey> = self
            .project
            .video_tracks
            .iter()
            .filter_map(|t| self.video_key(t))
            .collect();
        let first = keys.first().cloned();
        if self.thumbnail_video != first {
            self.thumbnail_video = first;
            self.timeline_view = TimelineView::default();
        }
        let wanted: HashSet<&VideoKey> = keys.iter().collect();
        self.thumbnails.retain(|key, _| wanted.contains(key));
        for key in keys {
            let backend = &self.backend;
            let entry = self.thumbnails.entry(key.clone()).or_insert_with(|| {
                let service = Thumbnails::new(backend.clone());
                let ctx = ctx.clone();
                service.on_update(move || ctx.request_repaint());
                service.set_video(Some(key));
                VideoThumbnails {
                    service,
                    textures: BTreeMap::new(),
                }
            });
            let frames = entry.service.frames();
            entry.textures.retain(|frame, _| frames.contains_key(frame));
            for (frame, image) in frames {
                entry.textures.entry(frame).or_insert_with(|| {
                    let size = [image.width as usize, image.height as usize];
                    let texture = ctx.load_texture(
                        format!("thumbnail-{frame}"),
                        egui::ColorImage::from_rgb(size, &image.data),
                        egui::TextureOptions::LINEAR,
                    );
                    let thumbnail = Thumbnail {
                        texture: texture.id(),
                        size: egui::vec2(size[0] as f32, size[1] as f32),
                    };
                    (texture, thumbnail)
                });
            }
        }
    }

    /// Renames the track `old`, and the bindings that read it. Returns false, changing nothing,
    /// if there's no such track or the name is empty or another track's.
    pub fn rename_track(&mut self, old: &str, new: &str) -> bool {
        self.project.rename_track(old, new)
    }

    /// The track in timeline row `row`: its kind and index among the tracks of that kind.
    fn track_row(&self, row: usize) -> Option<(TrackKind, usize)> {
        let videos = self.project.video_tracks.len();
        if row < videos {
            Some((TrackKind::Video, row))
        } else {
            (row - videos < self.project.audio_tracks.len())
                .then(|| (TrackKind::Audio, row - videos))
        }
    }

    fn track_at(&mut self, row: usize) -> Option<&mut ProjectTrack> {
        match self.track_row(row)? {
            (TrackKind::Video, i) => self.project.video_tracks.get_mut(i),
            (TrackKind::Audio, i) => self.project.audio_tracks.get_mut(i),
        }
    }

    /// The length of a track's file in seconds, once it is known.
    fn track_duration(&self, track: &ProjectTrack) -> Option<f64> {
        if track.resource.is_none() {
            return Some(0.0);
        }
        if self
            .project
            .video_tracks
            .iter()
            .any(|v| v.name == track.name)
        {
            return self
                .video_key(track)
                .and_then(|k| self.thumbnails.get(&k))
                .and_then(|t| t.service.info())
                .map(|v| v.frame_count as f64 / v.frame_rate.as_f64());
        }
        self.engine
            .loaded_tracks()
            .iter()
            .find(|l| l.name == track.name)
            .map(|l| l.clip.duration_secs())
    }

    /// Each track's resource length, as the item edits take it.
    fn item_lengths(&self) -> impl Fn(&ProjectTrack) -> Option<f64> + use<> {
        let lengths: Vec<(String, Option<f64>)> = self
            .project
            .tracks()
            .map(|t| (t.name.clone(), self.track_duration(t)))
            .collect();
        move |t: &ProjectTrack| {
            lengths
                .iter()
                .find(|(n, _)| *n == t.name)
                .and_then(|(_, d)| *d)
        }
    }

    /// Drops selected items that no longer exist (after an undo, or a track removed or renamed).
    fn tidy_item_selection(&mut self) {
        let project = &self.project;
        self.selected_graph_item = self.selected_graph_item.filter(|&(layer, item)| {
            project
                .layers
                .get(layer)
                .is_some_and(|l| item < l.items.len())
        });
        self.selected_items.retain(|r| {
            project
                .track(&r.track)
                .is_some_and(|t| r.item < t.items.len())
        });
    }

    fn track_action(&mut self, ctx: &egui::Context, action: TrackAction) {
        let name = |app: &mut Self, row: usize| app.track_at(row).map(|t| t.name.clone());
        match action {
            TrackAction::Select(row) => self.selected_track = Some(row),
            TrackAction::SelectItem { row, item, toggle } => {
                let Some(track) = name(self, row) else { return };
                let at = ItemRef::new(track, item);
                self.selected_track = Some(row);
                self.selected_graph_item = None;
                if !toggle {
                    self.selected_items = vec![at];
                } else if let Some(k) = self.selected_items.iter().position(|r| *r == at) {
                    self.selected_items.remove(k);
                } else {
                    self.selected_items.push(at);
                }
            }
            TrackAction::ClearItems => {
                self.selected_items.clear();
                self.selected_graph_item = None;
            }
            TrackAction::SelectItems { items, additive } => {
                self.selected_graph_item = None;
                if !additive {
                    self.selected_items.clear();
                }
                for (row, item) in items {
                    let Some(track) = name(self, row) else {
                        continue;
                    };
                    let at = ItemRef::new(track, item);
                    if !self.selected_items.contains(&at) {
                        self.selected_items.push(at);
                    }
                }
            }
            TrackAction::MoveItem { row, item, delta } => {
                let Some(track) = name(self, row) else { return };
                let at = ItemRef::new(track, item);
                let lengths = self.item_lengths();
                if self.selected_items.contains(&at) {
                    self.project
                        .move_items(&self.selected_items, delta, lengths);
                } else {
                    self.project.move_items(&[at], delta, lengths);
                }
            }
            TrackAction::TrimItem {
                row,
                item,
                edge,
                to,
                stretch,
            } => {
                let Some(track) = name(self, row) else { return };
                let lengths = self.item_lengths();
                self.project
                    .trim_item(&ItemRef::new(track, item), edge, to, stretch, lengths);
            }
            TrackAction::SplitItems { at } => {
                if let Some((layer, item)) = self.selected_graph_item {
                    self.project.split_graph_item(layer, item, at);
                    return;
                }
                let lengths = self.item_lengths();
                if self.selected_items.is_empty() {
                    // With nothing selected, graph items under the playhead split too.
                    for layer in 0..self.project.layers.len() {
                        for item in (0..self.project.layers[layer].items.len()).rev() {
                            self.project.split_graph_item(layer, item, at);
                        }
                    }
                    // With nothing selected, every item under the playhead.
                    let under: Vec<ItemRef> = self
                        .project
                        .tracks()
                        .flat_map(|t| {
                            let length = lengths(t).unwrap_or(f64::INFINITY);
                            t.items
                                .iter()
                                .enumerate()
                                .filter(move |(_, i)| {
                                    i.position < at && at < i.timeline_end(length)
                                })
                                .map(|(k, _)| ItemRef::new(t.name.clone(), k))
                        })
                        .collect();
                    self.project.split_items(&under, at, lengths);
                } else {
                    self.selected_items =
                        self.project.split_items(&self.selected_items, at, lengths);
                }
            }
            TrackAction::DeleteItems => {
                if let Some((layer, item)) = self.selected_graph_item {
                    self.delete_graph_item(layer, item);
                    return;
                }
                let lengths = self.item_lengths();
                self.project.delete_items(&self.selected_items, lengths);
                self.selected_items.clear();
            }
            TrackAction::CopyItems | TrackAction::CutItems => {
                if self.selected_items.is_empty() {
                    return;
                }
                let lengths = self.item_lengths();
                self.item_clipboard = self.project.copy_items(&self.selected_items, &lengths);
                // The platform only sends Ctrl+V on when the system clipboard holds text, so the
                // copy puts a marker there; a paste pastes the items only while it is still there.
                ctx.copy_text(ITEM_CLIPBOARD_MARKER.to_owned());
                if action == TrackAction::CutItems {
                    self.project.delete_items(&self.selected_items, lengths);
                    self.selected_items.clear();
                }
            }
            TrackAction::PasteItems { at, text } => {
                let ours = text.is_none_or(|t| t == ITEM_CLIPBOARD_MARKER);
                if ours && !self.item_clipboard.is_empty() {
                    self.selected_items = self.project.paste_items(&self.item_clipboard, at);
                }
            }
            TrackAction::ToggleItemMute { row, item } => {
                if let Some(item) = self.track_at(row).and_then(|t| t.items.get_mut(item)) {
                    item.muted = !item.muted;
                }
            }
            TrackAction::ToggleMute(row) => {
                if let Some(track) = self.track_at(row) {
                    track.muted = !track.muted;
                }
            }
            TrackAction::ToggleSolo(row) => {
                if let Some(track) = self.track_at(row) {
                    track.solo = !track.solo;
                }
            }
            TrackAction::SetVolume(row, volume) => {
                if let Some(track) = self.track_at(row) {
                    track.volume = volume.clamp(0.0, 1.0);
                }
            }
            TrackAction::SetHeight(row, height) => {
                if let Some(track) = self.track_at(row) {
                    track.height = (height != LANE_HEIGHT).then_some(height);
                }
            }
            TrackAction::Link(row, other) => {
                if let (Some(a), Some(b)) = (name(self, row), name(self, other)) {
                    self.project.link_tracks(&a, &b);
                }
            }
            TrackAction::Unlink(row) => {
                if let Some(track) = name(self, row) {
                    self.project.unlink_track(&track);
                }
            }
            TrackAction::SetBus(row, bus) => {
                if let Some(track) = self.track_at(row) {
                    track.bus = bus;
                }
            }
            TrackAction::Move { from, to } => {
                let (Some((kind, from)), Some((to_kind, to))) =
                    (self.track_row(from), self.track_row(to))
                else {
                    return;
                };
                if kind != to_kind {
                    return;
                }
                // Rows of this kind start at `first`; a selected track of the other kind stays
                // selected where it is.
                let (tracks, first) = match kind {
                    TrackKind::Video => (&mut self.project.video_tracks, 0),
                    TrackKind::Audio => {
                        let videos = self.project.video_tracks.len();
                        (&mut self.project.audio_tracks, videos)
                    }
                };
                let selected = self
                    .selected_track
                    .and_then(|row| row.checked_sub(first))
                    .filter(|&i| i < tracks.len());
                if let Some(moved) = move_track(tracks, from, to, selected) {
                    self.selected_track = Some(first + moved);
                }
            }
            TrackAction::Rename(row, new) => {
                if let Some(old) = self.track_at(row).map(|t| t.name.clone()) {
                    self.project.rename_track(&old, &new);
                }
            }
            TrackAction::Remove(row) => {
                let Some((kind, i)) = self.track_row(row) else {
                    return;
                };
                if kind == TrackKind::Video {
                    let removed = self.project.video_tracks.remove(i);
                    self.project.unlink_track(&removed.name);
                    let rows = self.project.video_tracks.len() + self.project.audio_tracks.len();
                    self.selected_track = self.selected_track.filter(|&s| s < rows);
                    return;
                }
                let removed = self.project.audio_tracks.remove(i);
                self.project.unlink_track(&removed.name);
                let rows = self.project.video_tracks.len() + self.project.audio_tracks.len();
                self.selected_track = match self.selected_track {
                    Some(s) if s >= row && s > 0 => Some(s - 1),
                    other => other,
                }
                .filter(|&s| s < rows)
                .or(first_audio_row(&self.project));
            }
            TrackAction::Add => self.pick_audio_tracks(),
            TrackAction::AddEmpty(kind) => {
                self.add_empty_track(kind);
            }
        }
    }

    fn windows(&mut self, ui: &Ui) {
        if let Some(e) = self.audio.error.take() {
            self.error
                .get_or_insert_with(|| tr_args("error.audio_unavailable", &[("error", &e)]));
        }
        if let Some(message) = self.error.clone() {
            let mut open = true;
            egui::Window::new(tr("dialog.problem"))
                .collapsible(false)
                .open(&mut open)
                .show(ui.ctx(), |ui| ui.label(message));
            if !open {
                self.error = None;
            }
        }

        let mut open = self.show_about;
        egui::Window::new(tr("about.title"))
            .open(&mut open)
            .collapsible(false)
            .show(ui.ctx(), |ui| {
                ui.heading(tr_args(
                    "about.heading",
                    &[("version", env!("CARGO_PKG_VERSION"))],
                ));
                ui.label(tr("about.copyright"));
                ui.separator();
                ui.label(tr("about.ffmpeg"));
                ui.hyperlink_to("ffmpeg.org", "https://ffmpeg.org");
                // The text of the version the loaded FFmpeg is under: the release build is LGPL 2.1
                // or later, the development build LGPL 3.
                let license = self.backend_info.as_ref().map_or("", BackendInfo::license);
                ui.hyperlink_to("GNU LGPL", rastersong_engine::lgpl_url(license));
                if let Some(info) = &self.backend_info {
                    ui.label(tr_args(
                        "about.ffmpeg_license",
                        &[("license", info.license())],
                    ));
                    egui::CollapsingHeader::new(tr("about.ffmpeg_libraries")).show(ui, |ui| {
                        for lib in &info.libraries {
                            ui.monospace(format!("{:<11} {}", lib.name, lib.version));
                        }
                        ui.label(tr("about.configuration"));
                        ui.add(
                            egui::Label::new(RichText::new(info.configuration).monospace().small())
                                .wrap(),
                        );
                    });
                }
            });
        self.show_about = open;
        self.settings_window(ui.ctx());
    }

    fn project_name(&self) -> String {
        self.project_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or_else(
                || tr("project.untitled").to_owned(),
                |n| n.to_string_lossy().into_owned(),
            )
    }

    fn update_title(&mut self, ui: &Ui) {
        let name = self.project_name();
        let title = tr_args(
            if self.is_dirty() {
                "window.title_dirty"
            } else {
                "window.title"
            },
            &[("name", &name)],
        );
        if title != self.title {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

/// How long the preview waits on a frame before saying so, when it has an older one to show.
const BUSY_DELAY_SECS: f64 = 0.25;

/// A spinner in the middle of the preview, with a line of text under it when there is one.
/// The resource dragged from the Resources panel and released over `area` this frame, taken.
fn dropped_resource(ui: &Ui, area: egui::Rect) -> Option<ResourceId> {
    dropped_payload::<DraggedResource>(ui, area).map(|p| p.0)
}

/// The graph dragged from the Resources panel and released over `area` this frame, taken.
fn dropped_graph(ui: &Ui, area: egui::Rect) -> Option<u32> {
    dropped_payload::<DraggedGraph>(ui, area).map(|p| p.0)
}

/// The drag-and-drop payload of type `T` released over `area` this frame, taken.
fn dropped_payload<T: std::any::Any + Send + Sync>(ui: &Ui, area: egui::Rect) -> Option<Arc<T>> {
    let released = ui.input(|i| i.pointer.any_released());
    let over = ui
        .input(|i| i.pointer.interact_pos())
        .is_some_and(|p| area.contains(p));
    if !(released && over) || !egui::DragAndDrop::has_payload_of_type::<T>(ui.ctx()) {
        return None;
    }
    egui::DragAndDrop::take_payload::<T>(ui.ctx())
}

fn busy_centered(ui: &mut Ui, rect: egui::Rect, text: Option<&str>) {
    ui.put(
        egui::Rect::from_center_size(rect.center(), egui::vec2(32.0, 32.0)),
        egui::Spinner::new(),
    );
    if let Some(text) = text {
        ui.put(
            egui::Rect::from_center_size(
                rect.center() + egui::vec2(0.0, 34.0),
                egui::vec2(rect.width() - 20.0, 20.0),
            ),
            egui::Label::new(RichText::new(text).color(Theme::of(ui.ctx()).text_dim)),
        );
    }
}

/// A small spinner and text in the corner of the preview, over the frame being shown.
fn busy_badge(ui: &mut Ui, rect: egui::Rect, text: &str, theme: &Theme) {
    let mut badge = ui.new_child(
        UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(
                rect.min + egui::vec2(8.0, 8.0),
                egui::vec2(rect.width() - 16.0, 28.0),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    egui::Frame::new()
        .fill(theme.preview_bg.gamma_multiply(0.75))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(6, 3))
        .show(&mut badge, |ui| {
            ui.add(egui::Spinner::new().size(14.0));
            ui.label(RichText::new(text).color(theme.text_dim).small());
        });
}

/// An editor for the project's open graph, with its output in place.
fn graph_editor(project: &Project) -> GraphEditor {
    let mut editor = GraphEditor::new(&project.graph);
    editor.ensure_output();
    editor
}

/// Puts `frame` in `slot`'s texture, making the texture the first time. Does nothing if the frame
/// is the one already there.
fn update_texture(
    ctx: &egui::Context,
    slot: &mut Option<(egui::TextureHandle, Arc<Frame>)>,
    name: &str,
    frame: Arc<Frame>,
) {
    if slot
        .as_ref()
        .is_some_and(|(_, shown)| Arc::ptr_eq(shown, &frame))
    {
        return;
    }
    let image =
        egui::ColorImage::from_rgb([frame.width as usize, frame.height as usize], &frame.rgb);
    match slot {
        Some((texture, shown)) => {
            texture.set(image, egui::TextureOptions::LINEAR);
            *shown = frame;
        }
        None => {
            let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
            *slot = Some((texture, frame));
        }
    }
}
