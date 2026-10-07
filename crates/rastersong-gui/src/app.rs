//! The application window. All rendering goes through the engine; this only holds UI state.
//!
//! Layout: the timeline along the bottom; above it the preview (with its playback controls),
//! the node graph and the inspector, side by side.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Ui, UiBuilder};
use rastersong_engine::LoadedTrack;
use rastersong_engine::playback::{MixTrack, Mixer};
use rastersong_engine::{
    AudioSink, AudioTrackSpec, BackendInfo, CompileOptions, Engine, EngineConfig, EngineStatus,
    Frame, Graph, GraphDesc, MediaBackend, NodeStats, PROJECT_EXTENSION, PlaybackClock,
    PreviewScale, Project, ProjectTrack, Registry, Thumbnails, TimelineMode,
};
use rastersong_lang::{tr, tr_args};

mod settings_window;

use crate::audio_out::AudioOut;

/// The sample rates offered for the project's rendered sound.
const AUDIO_RATES: [u32; 4] = [44_100, 48_000, 88_200, 96_000];

/// One track as playback mixes it: its name, offset in seconds and gain.
type MixEntry = (String, f64, f32);
use crate::editor::{CanvasContext, GraphEditor, InspectorContext, LinkedRename, without_layout};
use crate::history::History;
use crate::preview::{Feed, PreviewView, clamp_split, split_rects, split_sides};
use crate::settings::Settings;
use crate::theme::{Theme, ThemeChoice, apply_style};
use crate::timeline::{
    Thumbnail, TimelineModel, TimelineView, TrackAction, TrackView, timecode, timeline,
};
use crate::track_ops::{SoloState, move_track, toggle_solo};

/// The graph a new project starts with: the basic workflow from docs/concepts.md.
pub const STARTER_GRAPH: &str = include_str!("../../../examples/graphs/am_bands.json");

/// What the source engine renders: the video, untouched.
const SOURCE_GRAPH: &str = r#"{ "version": 0,
    "nodes": [ { "id": "video", "type": "video_input" }, { "id": "out", "type": "output" } ],
    "connections": [ { "from": "video", "to": "out" } ] }"#;

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "avi", "webm", "m4v", "ts", "mts", "m2ts", "mpg",
];
const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "mp3", "flac", "ogg", "m4a", "aac", "opus", "aiff", "mp4", "mkv", "mov",
];

/// Something that would throw away unsaved changes, waiting for the user to decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Close,
    NewProject,
    OpenProject,
}

pub struct App {
    engine: Engine,
    /// For quick checks on media files (e.g. whether a video has sound).
    backend: Arc<dyn MediaBackend>,
    thumbnails: Thumbnails,
    /// Textures of the decoded thumbnails, and the video they're of.
    thumbnail_textures: BTreeMap<usize, (egui::TextureHandle, Thumbnail)>,
    thumbnail_video: Option<PathBuf>,
    timeline_view: TimelineView,
    /// Where the timeline and the inspector were last drawn, for tests.
    timeline_area: egui::Rect,
    inspector_rect: egui::Rect,
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
    clock: PlaybackClock,
    /// Frame count and rate the clock was made for.
    clock_shape: Option<(usize, f64)>,
    settings: Settings,
    /// The theme last handed to egui.
    applied_theme: Option<ThemeChoice>,
    selected_track: Option<usize>,
    /// What an Alt+click solo remembers, to put the mutes back.
    solo: Option<SoloState>,
    /// When (egui time) the preview started waiting on a frame to render, if it is.
    waiting_since: Option<f64>,
    preview: Option<(egui::TextureHandle, Arc<Frame>)>,
    /// Renders the original video, the preview's second feed. It only has a video while the
    /// preview shows it, so it costs nothing otherwise.
    source_engine: Engine,
    /// The video the source engine was given.
    source_video_sent: Option<PathBuf>,
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
        let thumbnails = Thumbnails::new(backend.clone());
        let engine = Engine::new(backend.clone(), EngineConfig::default());
        let source_engine = Engine::new(backend.clone(), EngineConfig::default());
        let editor = linked_editor(&project);
        // The editor fills in positions, full port names and missing linked nodes; that isn't an
        // unsaved change.
        project.graph = editor.to_desc();
        let mut app = Self {
            engine,
            backend,
            thumbnails,
            thumbnail_textures: BTreeMap::new(),
            thumbnail_video: None,
            timeline_view: TimelineView::default(),
            timeline_area: egui::Rect::NOTHING,
            inspector_rect: egui::Rect::NOTHING,
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
            waiting_since: None,
            selected_track: (!project.audio_tracks.is_empty()).then_some(0),
            solo: None,
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
            source_preview: None,
            preview_view: PreviewView::default(),
            preview_feed: Feed::default(),
            preview_split: false,
            split_position: 0.5,
            error: None,
            show_about: false,
            show_settings: false,
            settings_tab: settings_window::SettingsTab::default(),
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

    /// How many video thumbnails are ready to draw.
    pub fn thumbnail_count(&self) -> usize {
        self.thumbnail_textures.len()
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

    /// The track specs the engine should be rendering with.
    pub fn track_specs(&self) -> Vec<AudioTrackSpec> {
        self.project.track_specs()
    }

    /// Hands the whole project to the engine.
    fn send_project(&mut self) {
        self.engine.set_preview_scale(self.settings.preview_scale());
        self.engine.set_video(self.project.video.clone());
        self.engine.set_audio_tracks(self.project.track_specs());
        self.sent_graph = without_layout(&self.project.graph);
        self.engine.set_graph(self.sent_graph.clone());
        self.engine.set_tempo(self.project.tempo);
        self.engine.set_bypass_all(self.project.bypass_graph);
        self.sent_mix = None;
    }

    fn load_project(&mut self, mut project: Project, path: Option<PathBuf>) {
        self.editor = linked_editor(&project);
        if !self.editor.warnings.is_empty() {
            self.error = Some(self.editor.warnings.join("\n"));
        }
        // The editor fills in positions and full port names; that isn't an unsaved change.
        project.graph = self.editor.to_desc();
        self.saved = project.clone();
        self.history = History::new(project.clone());
        self.selected_track = (!project.audio_tracks.is_empty()).then_some(0);
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

    /// Opens a video. If it has a sound track of its own, that's added as an audio track too
    /// (unless the project already has a track of that file).
    pub fn open_video(&mut self, path: PathBuf) {
        let has_audio = self.backend.has_audio(&path);
        self.project.video = Some(path.clone());
        self.project.video_name = None;
        self.engine.set_video(self.project.video.clone());
        self.clock.seek(0);
        self.link_project_inputs();
        let known = self.project.audio_tracks.iter().any(|t| t.path == path);
        if has_audio && !known {
            self.add_audio_tracks([path]);
        }
    }

    /// Adds an audio track for each file, named after it, each with its own audio input node.
    pub fn add_audio_tracks(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            let name = self.project.track_name_for(&path);
            self.project
                .audio_tracks
                .push(ProjectTrack::new(name.clone(), path));
            self.selected_track = Some(self.project.audio_tracks.len() - 1);
            self.editor
                .set_project_inputs(self.video_name(), self.track_name_list());
            self.editor.link_track(&name);
        }
    }

    /// The video's name: the user's, or else its file's. The timeline and the linked video node
    /// show the same name.
    fn video_name(&self) -> Option<String> {
        self.project.video_display_name()
    }

    fn track_name_list(&self) -> Vec<String> {
        self.project
            .audio_tracks
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }

    /// Tells the editor about the project's inputs and adds any linked nodes that are missing.
    fn link_project_inputs(&mut self) {
        self.editor
            .set_project_inputs(self.video_name(), self.track_name_list());
        self.editor.ensure_linked_nodes();
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
        if project.video != self.project.video {
            self.engine.set_video(project.video.clone());
        }
        self.editor.restore(&project.graph);
        self.selected_track = self
            .selected_track
            .filter(|&i| i < project.audio_tracks.len())
            .or((!project.audio_tracks.is_empty()).then_some(0));
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
            self.thumbnails.on_update(move || ctx.request_repaint());
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
            .show(ui, |ui| self.timeline(ui));
        egui::Panel::left("preview")
            .resizable(true)
            .default_size(ui.available_width() * 0.33)
            .min_size(280.0)
            .frame(panel(8))
            .show(ui, |ui| self.preview_column(ui));
        self.inspector_rect = egui::Panel::right("inspector")
            .resizable(true)
            .default_size(340.0)
            .min_size(300.0)
            .frame(panel(10))
            .show(ui, |ui| {
                let tracks: Vec<String> = self
                    .project
                    .audio_tracks
                    .iter()
                    .map(|t| t.name.clone())
                    .collect();
                let frame = self.engine.frame(self.clock.frame());
                let params = frame.as_ref().map_or(&[][..], |f| &f.params[..]);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.editor.show_inspector(
                        ui,
                        &InspectorContext {
                            tracks: &tracks,
                            params,
                        },
                    );
                });
            })
            .response
            .rect;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(Margin::ZERO))
            .show(ui, |ui| self.graph(ui));

        self.sync();
        self.record_history(ui);
        self.tick(ui);
        self.windows(ui);
        self.confirm_dialog(ui);
        self.update_title(ui);
    }

    fn graph(&mut self, ui: &mut Ui) {
        self.editor.keep_connections = self.settings.keep_connections;
        self.editor.max_warmup_frames = self.project.max_warmup_frames;
        let frame = self.engine.frame(self.clock.frame());
        let failure = match self.engine.status() {
            EngineStatus::Failed(failure) => Some(failure),
            _ => None,
        };
        let levels = frame.as_ref().map_or(&[][..], |f| &f.levels[..]);
        let params = frame.as_ref().map_or(&[][..], |f| &f.params[..]);
        let canvas = self.editor.show(
            ui,
            &CanvasContext {
                levels,
                params,
                failure: failure.as_ref(),
                wire_style: self.settings.wire_style,
                show_stats: self.settings.node_stats,
            },
        );
        self.bypass_all_button(ui, canvas.rect);
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
        let video = self
            .wants_source()
            .then(|| self.project.video.clone())
            .flatten();
        if self.source_video_sent != video {
            self.source_engine.set_video(video.clone());
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
        // Names changed on a node in the graph are the project's names too.
        for rename in self.editor.take_renames() {
            match rename {
                LinkedRename::Video(name) => self.rename_video(&name),
                LinkedRename::Track { from, to } => {
                    self.rename_track(&from, &to);
                }
            }
        }
        self.editor
            .set_project_inputs(self.video_name(), self.track_name_list());
        let graph = self.editor.to_desc();
        let semantic = without_layout(&graph);
        if semantic != self.sent_graph {
            self.engine.set_graph(semantic.clone());
            self.sent_graph = semantic;
        }
        self.project.graph = graph;
        self.engine.set_audio_tracks(self.project.track_specs());
        self.engine.set_tempo(self.project.tempo);
        self.engine.set_bypass_all(self.project.bypass_graph);
        self.engine.set_audio_rate(self.project.audio_rate);
        self.engine
            .set_max_warmup_frames(self.project.max_warmup_frames);
        self.sync_source_engine();

        // Rebuild the playback mix when tracks, offsets or levels change, once decoded, or when
        // the graph starts or stops rendering its own sound.
        let mix: Vec<(String, f64, f32)> = self
            .project
            .audio_tracks
            .iter()
            .map(|t| {
                (
                    t.name.clone(),
                    t.offset,
                    if t.muted { 0.0 } else { t.volume },
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
                AudioSink::Source | AudioSink::Passthrough(_) => {
                    Mixer::new(Self::mix_tracks(sink, mix, &loaded))
                }
            };
            self.audio.set_mixer(mixer);
            self.sent_mix = Some(wanted);
        }
    }

    /// The tracks playback mixes: every one for the source audio, or just the track an audio
    /// output passes through.
    fn mix_tracks(sink: &AudioSink, mix: &[MixEntry], loaded: &[LoadedTrack]) -> Vec<MixTrack> {
        mix.iter()
            .filter(|(name, ..)| match sink {
                AudioSink::Passthrough(track) => name == track,
                _ => true,
            })
            .filter_map(|(name, offset, gain)| {
                let clip = loaded.iter().find(|l| &l.name == name)?.clip.clone();
                Some(MixTrack {
                    clip,
                    offset: *offset,
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
            if let (Some(project), Some(preview)) = (self.thumbnails.info(), info) {
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
        let Some(info) = self.engine.info() else {
            ui.weak(tr("timeline.empty"));
            if ui.button(tr("timeline.track.add")).clicked() {
                self.pick_audio_tracks();
            }
            return;
        };
        self.tempo_bar(ui);
        let loaded = self.engine.loaded_tracks();
        let tracks = self
            .project
            .audio_tracks
            .iter()
            .map(|t| {
                let loaded = loaded.iter().find(|l| l.name == t.name);
                TrackView {
                    name: t.name.clone(),
                    duration: loaded.map(|l| l.clip.duration_secs()),
                    offset: t.offset,
                    muted: t.muted,
                    waveform: loaded.map(|l| l.waveform.clone()),
                }
            })
            .collect();
        self.update_thumbnails(ui.ctx());
        let thumbnails: BTreeMap<usize, Thumbnail> = self
            .thumbnail_textures
            .iter()
            .map(|(&frame, (_, thumbnail))| (frame, *thumbnail))
            .collect();
        let video_details = self.thumbnails.info().map(|v| {
            format!(
                "{}×{} · {:.3} fps",
                v.width,
                v.height,
                v.frame_rate.as_f64()
            )
            .replace(".000 fps", " fps")
        });
        let cached = self.engine.cached_ranges();
        let model = TimelineModel {
            frame_count: info.frames,
            frame_rate: info.frame_rate.as_f64(),
            playhead: self.clock.frame(),
            cached: &cached,
            video_name: self.video_name(),
            video_details,
            tracks,
            selected_track: self.selected_track,
            thumbnails: &thumbnails,
            loop_region: self.project.loop_region,
            tempo: self.project.tempo,
            mode: self.project.timeline_mode,
        };
        self.timeline_area = ui.available_rect_before_wrap();
        let response = timeline(ui, &model, &mut self.timeline_view);
        self.thumbnails.request(&response.wanted_thumbnails);
        if let Some(region) = response.loop_region {
            self.project.loop_region = region;
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
        for action in response.actions {
            self.track_action(action);
        }
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

    /// Keeps the thumbnail service on the project's video, and a texture for each thumbnail it
    /// has decoded.
    fn update_thumbnails(&mut self, ctx: &egui::Context) {
        if self.thumbnail_video != self.project.video {
            self.thumbnail_video = self.project.video.clone();
            self.thumbnails.set_video(self.project.video.clone());
            self.thumbnail_textures.clear();
            self.timeline_view = TimelineView::default();
        }
        let frames = self.thumbnails.frames();
        self.thumbnail_textures
            .retain(|frame, _| frames.contains_key(frame));
        for (frame, image) in frames {
            self.thumbnail_textures.entry(frame).or_insert_with(|| {
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

    /// Renames the track `old` (and the audio inputs reading it). Returns false, changing
    /// nothing, if there's no such track or the name is empty or another track's.
    pub fn rename_track(&mut self, old: &str, new: &str) -> bool {
        let new = new.trim();
        let Some(i) = self.project.audio_tracks.iter().position(|t| t.name == old) else {
            return false;
        };
        if new == old {
            return true;
        }
        if new.is_empty() || self.project.audio_tracks.iter().any(|t| t.name == new) {
            return false;
        }
        self.editor.rename_track(old, new);
        self.project.audio_tracks[i].name = new.to_owned();
        self.editor
            .set_project_inputs(self.video_name(), self.track_name_list());
        true
    }

    /// Names the video. A name equal to the file's is no rename at all.
    pub fn rename_video(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.project.video_name = Some(name.to_owned());
        if self.project.video_name == self.project.video_display_name_from_file() {
            self.project.video_name = None;
        }
        self.editor
            .set_project_inputs(self.video_name(), self.track_name_list());
    }

    fn track_action(&mut self, action: TrackAction) {
        match action {
            TrackAction::Select(i) => self.selected_track = Some(i),
            TrackAction::SetOffset(i, offset) => {
                if let Some(track) = self.project.audio_tracks.get_mut(i) {
                    track.offset = offset;
                }
            }
            TrackAction::ToggleMute(i) => {
                if let Some(track) = self.project.audio_tracks.get_mut(i) {
                    track.muted = !track.muted;
                }
            }
            TrackAction::Solo(i) => {
                toggle_solo(&mut self.project.audio_tracks, i, &mut self.solo);
            }
            TrackAction::Move { from, to } => {
                self.selected_track = move_track(
                    &mut self.project.audio_tracks,
                    from,
                    to,
                    self.selected_track,
                );
            }
            TrackAction::Rename(i, name) => {
                if let Some(old) = self.project.audio_tracks.get(i).map(|t| t.name.clone()) {
                    self.rename_track(&old, &name);
                }
            }
            TrackAction::RenameVideo(name) => self.rename_video(&name),
            TrackAction::Remove(i) => {
                let removed = self.project.audio_tracks.remove(i);
                self.editor
                    .set_project_inputs(self.video_name(), self.track_name_list());
                self.editor.unlink_track(&removed.name);
                self.selected_track = match self.selected_track {
                    _ if self.project.audio_tracks.is_empty() => None,
                    Some(s) if s >= i && s > 0 => Some(s - 1),
                    other => other,
                };
            }
            TrackAction::Add => self.pick_audio_tracks(),
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
                ui.hyperlink_to("GNU LGPL", "https://www.gnu.org/licenses/lgpl-3.0.html");
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

/// An editor for the project's graph, with the nodes linked to its video and tracks in place.
fn linked_editor(project: &Project) -> GraphEditor {
    let mut editor = GraphEditor::new(&project.graph);
    let video = project.video_display_name();
    let tracks = project
        .audio_tracks
        .iter()
        .map(|t| t.name.clone())
        .collect();
    editor.set_project_inputs(video, tracks);
    editor.ensure_linked_nodes();
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
