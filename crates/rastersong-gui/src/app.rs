//! The application window. All rendering goes through the engine; this only holds UI state.
//!
//! Layout: the timeline along the bottom; above it the preview (with its playback controls),
//! the node graph and the inspector, side by side.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Ui, UiBuilder};
use rastersong_engine::playback::{MixTrack, Mixer};
use rastersong_engine::{
    AudioTrackSpec, BackendInfo, Engine, EngineConfig, EngineStatus, Frame, GraphDesc,
    MediaBackend, PROJECT_EXTENSION, PlaybackClock, PreviewScale, Project, ProjectTrack,
};

use crate::audio_out::AudioOut;
use crate::editor::{CanvasContext, GraphEditor, InspectorContext, without_layout};
use crate::settings::Settings;
use crate::theme::{Theme, ThemeChoice, apply_style};
use crate::timeline::{TimelineModel, TrackAction, TrackView, timecode, timeline};

/// The graph a new project starts with: the basic workflow from the readme.
pub const STARTER_GRAPH: &str = include_str!("../../../examples/graphs/am_bands.json");

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "avi", "webm", "m4v", "ts", "mts", "m2ts", "mpg",
];
const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "mp3", "flac", "ogg", "m4a", "aac", "opus", "aiff", "mp4", "mkv", "mov",
];

pub struct App {
    engine: Engine,
    audio: AudioOut,
    backend_info: Option<BackendInfo>,
    project: Project,
    project_path: Option<PathBuf>,
    /// The project as last saved or opened, to tell whether there are unsaved changes.
    saved: Project,
    editor: GraphEditor,
    /// The graph (without layout) the engine is rendering.
    sent_graph: GraphDesc,
    /// Mix last given to the audio output: (track, offset, gain) per track.
    sent_mix: Option<Vec<(String, f64, f32)>>,
    clock: PlaybackClock,
    /// Frame count and rate the clock was made for.
    clock_shape: Option<(usize, f64)>,
    settings: Settings,
    /// The theme last handed to egui.
    applied_theme: Option<ThemeChoice>,
    selected_track: Option<usize>,
    /// Edit buffers for the track names in the timeline.
    track_names: Vec<String>,
    preview: Option<(egui::TextureHandle, Arc<Frame>)>,
    error: Option<String>,
    show_about: bool,
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
        let engine = Engine::new(backend, EngineConfig::default());
        let editor = GraphEditor::new(&project.graph);
        // The editor fills in positions and full port names; that isn't an unsaved change.
        project.graph = editor.to_desc();
        let mut app = Self {
            engine,
            audio,
            backend_info,
            saved: project.clone(),
            sent_graph: GraphDesc::from_json(r#"{ "version": 1, "nodes": [] }"#).unwrap(),
            sent_mix: None,
            track_names: project
                .audio_tracks
                .iter()
                .map(|t| t.name.clone())
                .collect(),
            selected_track: (!project.audio_tracks.is_empty()).then_some(0),
            project,
            project_path: None,
            editor,
            clock: PlaybackClock::new(30.0, 0),
            clock_shape: None,
            settings: Settings::default(),
            applied_theme: None,
            preview: None,
            error: None,
            show_about: false,
            initialized: false,
            title: String::new(),
        };
        app.send_project();
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

    /// The user's settings, saved between sessions.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
        self.engine.set_preview_scale(self.settings.preview_scale());
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
        self.sent_mix = None;
    }

    fn load_project(&mut self, mut project: Project, path: Option<PathBuf>) {
        self.editor = GraphEditor::new(&project.graph);
        if !self.editor.warnings.is_empty() {
            self.error = Some(self.editor.warnings.join("\n"));
        }
        // The editor fills in positions and full port names; that isn't an unsaved change.
        project.graph = self.editor.to_desc();
        self.saved = project.clone();
        self.track_names = project
            .audio_tracks
            .iter()
            .map(|t| t.name.clone())
            .collect();
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

    pub fn open_video(&mut self, path: PathBuf) {
        self.project.video = Some(path);
        self.engine.set_video(self.project.video.clone());
        self.clock.seek(0);
    }

    pub fn add_audio_track(&mut self, path: PathBuf) {
        let name = self.project.unused_track_name();
        self.project
            .audio_tracks
            .push(ProjectTrack::new(name.clone(), path));
        self.track_names.push(name);
        self.selected_track = Some(self.project.audio_tracks.len() - 1);
    }

    fn save(&mut self, choose_path: bool) {
        let path = match (&self.project_path, choose_path) {
            (Some(path), false) => Some(path.clone()),
            _ => rfd::FileDialog::new()
                .add_filter("RasterSong project", &[PROJECT_EXTENSION])
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

    /// Whether the project has changed since it was last saved or opened.
    pub fn is_dirty(&self) -> bool {
        self.project != self.saved
    }

    /// The root of the UI.
    pub fn ui(&mut self, ui: &mut Ui) {
        if !self.initialized {
            let ctx = ui.ctx().clone();
            self.engine.on_update(move || ctx.request_repaint());
            apply_style(ui.ctx());
            self.initialized = true;
        }
        if self.applied_theme != Some(self.settings.theme) {
            ui.ctx().set_theme(self.settings.theme.preference());
            self.applied_theme = Some(self.settings.theme);
        }
        self.shortcuts(ui);

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
            .default_size(190.0)
            .min_size(150.0)
            .frame(panel(8))
            .show(ui, |ui| self.timeline(ui));
        egui::Panel::left("preview")
            .resizable(true)
            .default_size(ui.available_width() * 0.33)
            .min_size(280.0)
            .frame(panel(8))
            .show(ui, |ui| self.preview_column(ui));
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(300.0)
            .min_size(220.0)
            .frame(panel(10))
            .show(ui, |ui| {
                let tracks: Vec<String> = self
                    .project
                    .audio_tracks
                    .iter()
                    .map(|t| t.name.clone())
                    .collect();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.editor
                        .show_inspector(ui, &InspectorContext { tracks: &tracks });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(Margin::ZERO))
            .show(ui, |ui| self.graph(ui));

        self.sync();
        self.tick(ui);
        self.windows(ui);
        self.update_title(ui);
    }

    fn graph(&mut self, ui: &mut Ui) {
        let frame = self.engine.frame(self.clock.frame());
        let failure = match self.engine.status() {
            EngineStatus::Failed(failure) => Some(failure),
            _ => None,
        };
        let levels = frame.as_ref().map_or(&[][..], |f| &f.levels[..]);
        self.editor.show(
            ui,
            &CanvasContext {
                levels,
                failure: failure.as_ref(),
            },
        );
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
        self.engine.set_audio_tracks(self.project.track_specs());

        // Rebuild the playback mix when tracks, offsets or levels change, once decoded.
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
        if decoded && self.sent_mix.as_ref() != Some(&mix) {
            let tracks = mix
                .iter()
                .filter_map(|(name, offset, gain)| {
                    let clip = loaded.iter().find(|l| &l.name == name)?.clip.clone();
                    Some(MixTrack {
                        clip,
                        offset: *offset,
                        gain: *gain,
                    })
                })
                .collect();
            self.audio.set_mixer(Mixer::new(tracks));
            self.sent_mix = Some(mix);
        }
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
        let dt = f64::from(ui.input(|i| i.stable_dt)).min(0.1);
        let buffered = self.engine.buffered_from(self.clock.frame());
        self.clock.advance(dt, buffered);
        self.engine.set_playhead(self.clock.frame());
        let fps = self.clock_shape.map_or(30.0, |s| s.1);
        self.audio.update(
            self.clock.position() / fps,
            self.clock.speed(),
            self.clock.is_playing(),
            self.settings.volume,
        );
        if self.clock.is_playing() {
            ui.ctx().request_repaint();
        }
    }

    fn shortcuts(&mut self, ui: &Ui) {
        // Don't steal keys from text fields.
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (space, left, right, home, save) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::Home),
                i.modifiers.command && i.key_pressed(egui::Key::S),
            )
        });
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
            ui.menu_button("File", |ui| {
                if ui.button("New Project").clicked() {
                    let graph = GraphDesc::from_json(STARTER_GRAPH).unwrap();
                    self.load_project(Project::new(graph), None);
                    ui.close();
                }
                if ui.button("Open Project…").clicked() {
                    ui.close();
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("RasterSong project", &[PROJECT_EXTENSION])
                        .pick_file()
                    {
                        self.open_project(&path);
                    }
                }
                if ui.button("Save Project").clicked() {
                    ui.close();
                    self.save(false);
                }
                if ui.button("Save Project As…").clicked() {
                    ui.close();
                    self.save(true);
                }
                ui.separator();
                if ui.button("Open Video…").clicked() {
                    ui.close();
                    self.pick_video();
                }
                if ui.button("Add Audio Track…").clicked() {
                    ui.close();
                    self.pick_audio_track();
                }
                ui.separator();
                if ui.button("Import Graph…").clicked() {
                    ui.close();
                    self.import_graph();
                }
                if ui.button("Export Graph…").clicked() {
                    ui.close();
                    self.export_graph();
                }
            });
            ui.menu_button("View", |ui| {
                ui.label(RichText::new("Theme").weak());
                for choice in ThemeChoice::ALL {
                    if ui
                        .radio_value(&mut self.settings.theme, choice, choice.label())
                        .clicked()
                    {
                        ui.close();
                    }
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About RasterSong").clicked() {
                    self.show_about = true;
                    ui.close();
                }
            });
        });
    }

    fn pick_video(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Video", VIDEO_EXTENSIONS)
            .pick_file()
        {
            self.open_video(path);
        }
    }

    fn pick_audio_track(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio", AUDIO_EXTENSIONS)
            .pick_file()
        {
            self.add_audio_track(path);
        }
    }

    fn import_graph(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Graph", &["json"])
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
            .add_filter("Graph", &["json"])
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
        let controls_height = 64.0;
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

    /// Draws the preview into `rect`. Returns true if the user asked to open a video.
    fn preview_image(&mut self, ui: &mut Ui, rect: egui::Rect) -> bool {
        let theme = Theme::of(ui.ctx());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), theme.preview_bg);

        if let Some(frame) = self.engine.frame(self.clock.frame()) {
            let changed = self
                .preview
                .as_ref()
                .is_none_or(|(_, shown)| !Arc::ptr_eq(shown, &frame));
            if changed {
                let image = egui::ColorImage::from_rgb(
                    [frame.width as usize, frame.height as usize],
                    &frame.rgb,
                );
                match &mut self.preview {
                    Some((texture, shown)) => {
                        texture.set(image, egui::TextureOptions::LINEAR);
                        *shown = frame;
                    }
                    None => {
                        let texture =
                            ui.ctx()
                                .load_texture("preview", image, egui::TextureOptions::LINEAR);
                        self.preview = Some((texture, frame));
                    }
                }
            }
        }

        // Keep showing the last frame while the next renders.
        if let Some((texture, frame)) = &self.preview {
            let size = egui::vec2(frame.width as f32, frame.height as f32);
            let fit = (rect.width() / size.x).min(rect.height() / size.y);
            let shown = egui::Rect::from_center_size(rect.center(), size * fit);
            ui.painter().image(
                texture.id(),
                shown,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        let message = |ui: &mut Ui, text: &str| {
            ui.put(
                egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width() - 40.0, 60.0)),
                egui::Label::new(RichText::new(text).color(theme.text_dim).size(15.0)).wrap(),
            );
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
                            RichText::new("Open a video to start")
                                .color(theme.text_dim)
                                .size(15.0),
                        );
                        ui.add_space(4.0);
                        open_video = ui
                            .button("Open Video…")
                            .on_hover_text("Choose the video to process")
                            .clicked();
                    },
                );
            }
            EngineStatus::Loading => {
                ui.put(
                    egui::Rect::from_center_size(rect.center(), egui::vec2(32.0, 32.0)),
                    egui::Spinner::new(),
                );
            }
            // The graph panel explains failures; the picture just stays as it was.
            EngineStatus::Failed(_) if self.preview.is_none() => message(
                ui,
                "The graph can't render; see the bottom of the graph panel",
            ),
            EngineStatus::Ready if self.engine.frame(self.clock.frame()).is_none() => {
                ui.put(
                    egui::Rect::from_min_size(
                        rect.min + egui::vec2(10.0, 10.0),
                        egui::vec2(18.0, 18.0),
                    ),
                    egui::Spinner::new(),
                );
            }
            _ => {}
        }
        open_video
    }

    /// Playback controls under the preview.
    fn controls(&mut self, ui: &mut Ui) {
        let info = self.engine.info();
        ui.horizontal(|ui| {
            let label = if self.clock.is_playing() { "⏸" } else { "▶" };
            let play = egui::Button::new(RichText::new(label).size(16.0)).min_size(egui::vec2(34.0, 26.0));
            if ui
                .add_enabled(info.is_some(), play)
                .on_hover_text("Play / pause (Space)")
                .clicked()
            {
                self.clock.toggle();
            }
            if let Some(info) = info {
                let fps = info.frame_rate.as_f64();
                let frame = self.clock.frame();
                ui.monospace(format!(
                    "{} / {}",
                    timecode(frame as f64 / fps),
                    timecode(info.frames as f64 / fps)
                ));
                ui.weak(format!("frame {frame}"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let buffered_frames = self.engine.buffered_from(frame);
                    let buffered = buffered_frames as f64 / fps;
                    let speed = self.clock.speed();
                    let to_end = frame + buffered_frames >= info.frames;
                    let ahead = if to_end {
                        "rendered to the end".to_owned()
                    } else {
                        format!("{buffered:.1} s ahead")
                    };
                    let (text, color) = if !self.clock.is_playing() || speed >= 1.0 {
                        (ahead, ui.visuals().weak_text_color())
                    } else {
                        (
                            format!("{speed:.2}× · {ahead}"),
                            Theme::of(ui.ctx()).warning,
                        )
                    };
                    ui.colored_label(color, text).on_hover_text(
                        "Rendered ahead of the playhead. Playback slows to what rendering can keep up with.",
                    );
                });
            }
        });
        ui.horizontal(|ui| {
            ui.label("Preview");
            let mut scale = self.settings.preview_scale();
            egui::ComboBox::from_id_salt("preview-scale")
                .selected_text(scale_label(scale))
                .width(60.0)
                .show_ui(ui, |ui| {
                    for option in PreviewScale::ALL {
                        ui.selectable_value(&mut scale, option, scale_label(option));
                    }
                })
                .response
                .on_hover_text("Preview resolution. Lower is faster; export is always full size.");
            if scale != self.settings.preview_scale() {
                self.settings.set_preview_scale(scale);
                self.engine.set_preview_scale(scale);
            }

            ui.separator();
            ui.label("🔊");
            ui.spacing_mut().slider_width = 70.0;
            let volume = self.settings.volume;
            ui.add(egui::Slider::new(&mut self.settings.volume, 0.0..=1.0).show_value(false))
                .on_hover_text(format!("Playback volume {:.0}%", volume * 100.0));
        });
    }

    fn timeline(&mut self, ui: &mut Ui) {
        let Some(info) = self.engine.info() else {
            ui.weak("The timeline appears once a video is loaded.");
            if ui.button("+ Audio track").clicked() {
                self.pick_audio_track();
            }
            return;
        };
        let loaded = self.engine.loaded_tracks();
        let tracks = self
            .project
            .audio_tracks
            .iter()
            .map(|t| TrackView {
                name: t.name.clone(),
                duration: loaded
                    .iter()
                    .find(|l| l.name == t.name)
                    .map(|l| l.clip.duration_secs()),
                offset: t.offset,
                muted: t.muted,
            })
            .collect();
        let cached = self.engine.cached_ranges();
        let model = TimelineModel {
            frame_count: info.frames,
            frame_rate: info.frame_rate.as_f64(),
            playhead: self.clock.frame(),
            cached: &cached,
            video_name: self
                .project
                .video
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned()),
            tracks,
            selected_track: self.selected_track,
        };
        self.track_names
            .resize(self.project.audio_tracks.len(), String::new());
        let response = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| timeline(ui, &model, &mut self.track_names))
            .inner;
        if let Some(frame) = response.seek {
            self.clock.seek(frame);
        }
        for action in response.actions {
            self.track_action(action);
        }
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
            TrackAction::Rename(i, name) => {
                let old = self.project.audio_tracks[i].name.clone();
                let taken = self.project.audio_tracks.iter().any(|t| t.name == name);
                if name.is_empty() || taken {
                    self.track_names[i] = old;
                } else {
                    self.editor.rename_track(&old, &name);
                    self.project.audio_tracks[i].name = name.clone();
                    self.track_names[i] = name;
                }
            }
            TrackAction::Remove(i) => {
                self.project.audio_tracks.remove(i);
                self.track_names.remove(i);
                self.selected_track = match self.selected_track {
                    _ if self.project.audio_tracks.is_empty() => None,
                    Some(s) if s >= i && s > 0 => Some(s - 1),
                    other => other,
                };
            }
            TrackAction::Add => self.pick_audio_track(),
        }
    }

    fn windows(&mut self, ui: &Ui) {
        if let Some(e) = self.audio.error.take() {
            self.error
                .get_or_insert_with(|| format!("Audio playback is unavailable: {e}"));
        }
        if let Some(message) = self.error.clone() {
            let mut open = true;
            egui::Window::new("Problem")
                .collapsible(false)
                .open(&mut open)
                .show(ui.ctx(), |ui| ui.label(message));
            if !open {
                self.error = None;
            }
        }

        let mut open = self.show_about;
        egui::Window::new("About RasterSong")
            .open(&mut open)
            .collapsible(false)
            .show(ui.ctx(), |ui| {
                ui.heading(format!("RasterSong {}", env!("CARGO_PKG_VERSION")));
                ui.label("Copyright © 2025-2026 Alexander Davis. Source-available; see LICENSE.");
                ui.separator();
                ui.label(
                    "This software uses libraries from the FFmpeg project under the GNU LGPL.",
                );
                ui.hyperlink_to("ffmpeg.org", "https://ffmpeg.org");
                ui.hyperlink_to("GNU LGPL", "https://www.gnu.org/licenses/lgpl-3.0.html");
                if let Some(info) = &self.backend_info {
                    ui.label(format!("FFmpeg license: {}", info.license()));
                    egui::CollapsingHeader::new("FFmpeg libraries").show(ui, |ui| {
                        for lib in &info.libraries {
                            ui.monospace(format!("{:<11} {}", lib.name, lib.version));
                        }
                        ui.label("Configuration:");
                        ui.add(
                            egui::Label::new(RichText::new(info.configuration).monospace().small())
                                .wrap(),
                        );
                    });
                }
            });
        self.show_about = open;
    }

    fn update_title(&mut self, ui: &Ui) {
        let name = self
            .project_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or_else(
                || "Untitled".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            );
        let title = format!(
            "{name}{} — RasterSong",
            if self.is_dirty() { " •" } else { "" }
        );
        if title != self.title {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

fn scale_label(scale: PreviewScale) -> &'static str {
    match scale {
        PreviewScale::Full => "Full",
        PreviewScale::Half => "½",
        PreviewScale::Quarter => "¼",
        PreviewScale::Eighth => "⅛",
        PreviewScale::Sixteenth => "1/16",
    }
}
