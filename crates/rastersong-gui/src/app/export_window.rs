//! The Export window, in two columns: on the left format, quality, bus, range and size; on the
//! right the latest rendered frame, a summary, the progress with time left and Cancel, and the
//! Export… button. The render runs on a worker thread, so the app stays usable.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Layout, Margin, Rect, RichText, Stroke, Ui, UiBuilder, Vec2, vec2,
};
use rastersong_engine::export::{ExportJob, ExportSettings, LatestFrame, PreviewFrame, export};
use rastersong_engine::{
    EngineError, ExportFormat, FormatKind, MAX_WARMUP_FRAMES_LIMIT, ProResProfile, Quality,
    RenderInfo, Timebase, UNBOUNDED_WARMUP, export_formats,
};
use rastersong_lang::{tr, tr_args};

use super::App;
use super::settings_window::frame_rate_label;
use crate::theme::{self, CARD_RADIUS, CONTROL_RADIUS, SMALL_SIZE, Theme};
use crate::timeline::timecode;
use crate::value_box::ValueBox;
use crate::widgets::{
    Icon, IconButton, channels_label, choice_card, picture, section, section_label, segmented,
};

/// The window's size, and the width of the result column on its right.
const WINDOW_SIZE: Vec2 = vec2(800.0, 720.0);
const RESULT_WIDTH: f32 = 288.0;
/// The height of the title row.
const HEADER_HEIGHT: f32 = 56.0;

/// What the Export window is set to, kept while the app runs.
#[derive(Debug, Clone)]
pub(super) struct ExportDialog {
    pub open: bool,
    format: ExportFormat,
    /// The bus written; `None` is the master.
    bus: Option<String>,
    /// Output size; `None` is the project's.
    size: Option<(u32, u32)>,
    /// Export only the loop region, when there is one.
    region: bool,
    /// The latest frame the export wrote, and a count that changes with it.
    frame: Option<PreviewFrame>,
    frame_version: u64,
}

impl Default for ExportDialog {
    fn default() -> Self {
        Self {
            open: false,
            format: FormatKind::H264.default_format(),
            bus: None,
            size: None,
            region: false,
            frame: None,
            frame_version: 0,
        }
    }
}

/// An export on its worker thread.
#[derive(Debug)]
pub(super) struct RunningExport {
    path: PathBuf,
    done: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
    latest: Arc<LatestFrame>,
    started: Instant,
    thread: JoinHandle<Result<RenderInfo, EngineError>>,
}

/// Where the export stands, for the window and for tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportState {
    Idle,
    Running { done: usize, total: usize },
    Done(PathBuf),
    Cancelled,
    Failed(String),
}

/// The frames before the range an export renders without writing them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Warmup {
    None,
    /// As many as the graph remembers.
    Frames(usize),
    /// Every one, because the graph has feedback.
    FromStart,
}

/// The name a format is listed under.
fn format_name(kind: FormatKind) -> &'static str {
    match kind {
        FormatKind::H264 => tr("export.format.h264"),
        FormatKind::ProRes => tr("export.format.prores"),
        FormatKind::Ffv1 => tr("export.format.ffv1"),
    }
}

/// What a format is for, and the sound it carries.
fn format_about(kind: FormatKind) -> &'static str {
    match kind {
        FormatKind::H264 => tr("export.format.h264.about"),
        FormatKind::ProRes => tr("export.format.prores.about"),
        FormatKind::Ffv1 => tr("export.format.ffv1.about"),
    }
}

fn quality_name(quality: Quality) -> &'static str {
    match quality {
        Quality::Low => tr("export.quality.low"),
        Quality::Medium => tr("export.quality.medium"),
        Quality::High => tr("export.quality.high"),
    }
}

fn profile_name(profile: ProResProfile) -> &'static str {
    match profile {
        ProResProfile::Proxy => tr("export.profile.proxy"),
        ProResProfile::Lt => tr("export.profile.lt"),
        ProResProfile::Standard => tr("export.profile.standard"),
        ProResProfile::Hq => tr("export.profile.hq"),
    }
}

/// "1:05", or "1:02:05" past an hour.
fn duration_label(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "H.264 needs an even width; 1281 is odd.", or `None` when `kind` can encode the size.
fn odd_size_note(kind: FormatKind, width: u32, height: u32) -> Option<String> {
    let (even_width, even_height) = kind.nearest_size(width, height);
    let (side, size) = match (even_width != width, even_height != height) {
        (false, false) => return None,
        (true, false) => (tr("export.odd_side.width"), width.to_string()),
        (false, true) => (tr("export.odd_side.height"), height.to_string()),
        (true, true) => (tr("export.odd_side.both"), format!("{width}×{height}")),
    };
    Some(tr_args(
        "export.odd_size.short",
        &[
            ("format", kind.codec_name()),
            ("side", side),
            ("size", &size),
        ],
    ))
}

/// The frames before `start` an export renders first, for a graph whose nodes need up to
/// `needed` frames of warmup.
/// How much of the `warm` warmup frames an export that has rendered `done` frames has covered.
fn warmup_fraction(done: usize, warm: usize) -> f32 {
    if warm == 0 {
        0.0
    } else {
        done.min(warm) as f32 / warm as f32
    }
}

fn warmup_before(start: usize, needed: u32) -> Warmup {
    if start == 0 || needed == 0 {
        Warmup::None
    } else if needed == UNBOUNDED_WARMUP || needed >= MAX_WARMUP_FRAMES_LIMIT {
        Warmup::FromStart
    } else {
        Warmup::Frames(start.min(needed as usize))
    }
}

/// A name on the left and its value in monospace on the right.
fn summary_row(ui: &mut Ui, name: &str, value: &str) -> egui::Response {
    let theme = Theme::of(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).size(12.5).color(theme.text_dim));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(value).monospace().size(12.0)).truncate())
        })
        .inner
    })
    .inner
}

/// A small tag on a dark ground over the preview, at `pos` anchored by `align`.
fn preview_tag(
    ui: &Ui,
    pos: egui::Pos2,
    align: egui::Align2,
    text: &str,
    dot: Option<egui::Color32>,
) {
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        egui::FontId::monospace(10.0),
        egui::Color32::WHITE,
    );
    let dot_width = if dot.is_some() { 11.0 } else { 0.0 };
    let size = galley.size() + vec2(12.0 + dot_width, 4.0);
    let rect = align.anchor_size(pos, size);
    painter.rect_filled(rect, 4, egui::Color32::from_black_alpha(166));
    if let Some(color) = dot {
        painter.circle_filled(egui::pos2(rect.left() + 9.0, rect.center().y), 3.0, color);
    }
    painter.galley(
        rect.min + vec2(6.0 + dot_width, 2.0),
        galley,
        egui::Color32::WHITE,
    );
}

impl App {
    /// Opens the Export window.
    pub fn open_export(&mut self) {
        self.export.open = true;
    }

    /// Where the export stands.
    pub fn export_state(&self) -> ExportState {
        match (&self.running_export, &self.export_outcome) {
            (Some(run), _) => ExportState::Running {
                done: run.done.load(Ordering::Relaxed),
                total: run.total.load(Ordering::Relaxed),
            },
            (None, Some(outcome)) => outcome.clone(),
            (None, None) => ExportState::Idle,
        }
    }

    /// The index of the latest frame the export window shows, if it has one.
    pub fn export_frame(&self) -> Option<usize> {
        self.export.frame.as_ref().map(|f| f.index)
    }

    /// Asks the running export, if any, to stop.
    pub fn cancel_export(&mut self) {
        if let Some(run) = &self.running_export {
            run.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The format the Export window writes; the first that works here if the chosen one doesn't.
    fn export_format(&self) -> Option<ExportFormat> {
        let available = export_formats();
        if available
            .iter()
            .any(|f| f.kind == self.export.format.kind())
        {
            Some(self.export.format)
        } else {
            available.first().map(|f| f.kind.default_format())
        }
    }

    /// Starts exporting the project into `path` with the window's settings.
    pub fn start_export(&mut self, path: PathBuf) {
        if self.running_export.is_some() {
            return;
        }
        let Some(format) = self.export_format() else {
            self.export_outcome = Some(ExportState::Failed(tr("export.no_formats").into()));
            return;
        };
        // Resources whose files are missing are left out, as in the preview.
        let job = ExportJob {
            timeline: self.timeline_of(&self.project),
            ..ExportJob::from_project(&self.project)
        };
        let bus = self
            .export
            .bus
            .as_deref()
            .and_then(|name| job.bus(name))
            .unwrap_or_else(|| job.timeline.master());
        let settings = ExportSettings {
            format,
            size: self.export.size,
            range: self.export_region(),
            bus,
        };
        let done = Arc::new(AtomicUsize::new(0));
        let total = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let latest = Arc::new(LatestFrame::default());
        let backend = self.backend.clone();
        let cache = self.audio_cache.clone();
        let thread = {
            let (done, total, cancel, latest, path) = (
                done.clone(),
                total.clone(),
                cancel.clone(),
                latest.clone(),
                path.clone(),
            );
            std::thread::Builder::new()
                .name("export".into())
                .spawn(move || {
                    export(
                        backend.as_ref(),
                        cache.as_ref(),
                        &job,
                        &settings,
                        &path,
                        Some(&latest),
                        &mut |frames, of| {
                            done.store(frames, Ordering::Relaxed);
                            total.store(of, Ordering::Relaxed);
                            !cancel.load(Ordering::Relaxed)
                        },
                    )
                })
                .expect("the export thread starts")
        };
        self.export_outcome = None;
        self.export.frame = None;
        self.running_export = Some(RunningExport {
            path,
            done,
            total,
            cancel,
            latest,
            started: Instant::now(),
            thread,
        });
    }

    /// Picks up the latest frame written and a finished export.
    fn poll_export(&mut self) {
        let Some(run) = &self.running_export else {
            return;
        };
        if let Some(frame) = run.latest.take() {
            self.export.frame = Some(frame);
            self.export.frame_version += 1;
        }
        if !run.thread.is_finished() {
            return;
        }
        let run = self.running_export.take().expect("checked");
        if let Some(frame) = run.latest.take() {
            self.export.frame = Some(frame);
            self.export.frame_version += 1;
        }
        self.export_outcome = Some(match run.thread.join() {
            Ok(Ok(_)) => ExportState::Done(run.path),
            Ok(Err(EngineError::Cancelled)) => ExportState::Cancelled,
            Ok(Err(e)) => ExportState::Failed(e.to_string()),
            Err(_) => ExportState::Failed(tr("export.crashed").into()),
        });
    }

    pub(super) fn export_window(&mut self, ctx: &egui::Context) {
        self.poll_export();
        if self.running_export.is_some() {
            // Keep the progress and the frame moving while nothing else repaints.
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if !self.export.open {
            return;
        }
        let mut close = false;
        egui::Window::new(tr("export.title"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size(WINDOW_SIZE)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .frame(egui::Frame::window(&ctx.global_style()).inner_margin(0))
            .show(ctx, |ui| {
                let theme = Theme::of(ui.ctx());
                let (rect, _) = ui.allocate_exact_size(WINDOW_SIZE, egui::Sense::hover());
                let header = rect.with_max_y(rect.top() + HEADER_HEIGHT);
                let body = rect.with_min_y(header.bottom());
                let result = body.with_min_x(body.right() - RESULT_WIDTH);
                let settings = body.with_max_x(result.left());
                let painter = ui.painter();
                painter.hline(
                    rect.x_range(),
                    header.bottom(),
                    Stroke::new(1.0, theme.surfaces.divider),
                );
                painter.rect_filled(
                    result,
                    egui::CornerRadius {
                        se: theme::WINDOW_RADIUS,
                        ..Default::default()
                    },
                    theme.surfaces.bar,
                );
                painter.vline(
                    result.left(),
                    result.y_range(),
                    Stroke::new(1.0, theme.surfaces.divider),
                );
                close = self.export_header(ui, header);
                let running = self.running_export.is_some();
                let padded = |r: Rect, x: f32| r.shrink2(vec2(x, 18.0));
                ui.new_child(UiBuilder::new().max_rect(padded(settings, 20.0)))
                    .add_enabled_ui(!running, |ui| self.export_settings(ui));
                let mut column = ui.new_child(UiBuilder::new().max_rect(padded(result, 16.0)));
                self.export_result(&mut column);
            });
        // Closing the window doesn't stop an export; reopening it shows the progress.
        if close {
            self.export.open = false;
        }
    }

    /// The title, what exporting does, and Close. Returns whether Close was clicked.
    fn export_header(&self, ui: &mut Ui, rect: Rect) -> bool {
        let theme = Theme::of(ui.ctx());
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(16.0, 0.0)).with_min_x(rect.left() + 20.0))
                .layout(Layout::left_to_right(Align::Center)),
        );
        row.spacing_mut().item_spacing.x = 10.0;
        row.label(RichText::new(tr("export.title")).font(theme::semibold(16.0)));
        row.label(
            RichText::new(tr("export.subtitle"))
                .size(12.0)
                .color(theme.surfaces.text_faint),
        );
        row.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(IconButton::new(Icon::Close, tr("export.close")).size(vec2(32.0, 32.0)))
                .clicked()
        })
        .inner
    }

    fn export_settings(&mut self, ui: &mut Ui) {
        let theme = Theme::of(ui.ctx());
        let available = export_formats();
        if available.is_empty() {
            ui.label(tr("export.no_formats"));
            return;
        }
        let Some(mut format) = self.export_format() else {
            return;
        };
        ui.spacing_mut().item_spacing.y = 18.0;

        section(ui, tr("export.format"), |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for f in available {
                let selected = f.kind == format.kind();
                let about = if selected {
                    format!(
                        "{} {}",
                        format_about(f.kind),
                        tr_args("export.encoder", &[("encoder", f.video_encoder)])
                    )
                } else {
                    format_about(f.kind).to_owned()
                };
                if choice_card(ui, selected, format_name(f.kind), &about).clicked() && !selected {
                    format = f.kind.default_format();
                }
            }
        });

        match &mut format {
            ExportFormat::H264 { quality } => {
                section(ui, tr("export.quality"), |ui| {
                    let options: Vec<_> = Quality::ALL
                        .iter()
                        .map(|q| (quality_name(*q), tr("export.quality.help")))
                        .collect();
                    let on = Quality::ALL.iter().position(|q| q == quality).unwrap_or(0);
                    if let Some(i) = segmented(ui, &options, on) {
                        *quality = Quality::ALL[i];
                    }
                });
            }
            ExportFormat::ProRes { profile } => {
                section(ui, tr("export.profile"), |ui| {
                    let options: Vec<_> = ProResProfile::ALL
                        .iter()
                        .map(|p| (profile_name(*p), tr("export.profile.help")))
                        .collect();
                    let on = ProResProfile::ALL
                        .iter()
                        .position(|p| p == profile)
                        .unwrap_or(0);
                    if let Some(i) = segmented(ui, &options, on) {
                        *profile = ProResProfile::ALL[i];
                    }
                });
            }
            ExportFormat::Ffv1 => {}
        }
        self.export.format = format;

        ui.columns(2, |columns| {
            columns[0].spacing_mut().item_spacing.y = 6.0;
            columns[1].spacing_mut().item_spacing.y = 6.0;
            self.export_bus(&mut columns[0]);
            self.export_range(&mut columns[1]);
        });

        section(ui, tr("export.size"), |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            self.export_size(ui, format.kind());
            let (width, height) = self.export_output_size();
            if let Some(note) = odd_size_note(format.kind(), width, height) {
                let (even_width, even_height) = format.kind().nearest_size(width, height);
                egui::Frame::new()
                    .fill(theme.warning_note)
                    .stroke(Stroke::new(1.0, theme.warning_note_border))
                    .corner_radius(CONTROL_RADIUS)
                    .inner_margin(Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(note)
                                    .size(12.0)
                                    .color(theme.warning_note_text),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let fix = egui::Button::new(
                                    RichText::new(tr_args(
                                        "export.odd_size.fix",
                                        &[
                                            ("width", &even_width.to_string()),
                                            ("height", &even_height.to_string()),
                                        ],
                                    ))
                                    .color(theme.warning_note_text),
                                )
                                .fill(theme.warning_note)
                                .stroke(Stroke::new(1.0, theme.warning_note_border));
                                if ui.add(fix).clicked() {
                                    self.export.size = Some((even_width, even_height));
                                }
                            });
                        });
                    });
            }
        });
    }

    /// The bus written, and where its sound comes from.
    fn export_bus(&mut self, ui: &mut Ui) {
        let theme = Theme::of(ui.ctx());
        section_label(ui, tr("export.bus"));
        let buses = ExportJob::from_project(&self.project).buses();
        let master = buses[0].name.clone();
        let mut bus = self.export.bus.clone().unwrap_or_else(|| master.clone());
        let label = |name: &str| {
            let channels = buses
                .iter()
                .find(|b| b.name == name)
                .map_or(2, |b| b.channels);
            format!("{name} · {}", channels_label(channels))
        };
        egui::ComboBox::from_id_salt("export-bus")
            .width(ui.available_width())
            .selected_text(label(&bus))
            .show_ui(ui, |ui| {
                for b in &buses {
                    ui.selectable_value(&mut bus, b.name.clone(), label(&b.name));
                }
            });
        ui.label(
            RichText::new(tr_args("export.bus.about", &[("bus", &bus)]))
                .size(11.0)
                .color(theme.surfaces.text_faint),
        );
        self.export.bus = (bus != master).then_some(bus);
    }

    /// The project's size and frame rate: its own, or what the engine took from its video.
    fn export_timebase(&self) -> Timebase {
        self.project
            .timebase
            .or_else(|| self.engine.info().map(|info| info.timebase))
            .unwrap_or(Timebase::DEFAULT)
    }

    /// The size the export renders at: the chosen one, or the project's.
    fn export_output_size(&self) -> (u32, u32) {
        self.export.size.unwrap_or_else(|| {
            let timebase = self.export_timebase();
            (timebase.width, timebase.height)
        })
    }

    /// The frames the export writes when set to the loop region and there is one; `None` is the
    /// whole project.
    fn export_region(&self) -> Option<std::ops::Range<usize>> {
        let fps = self.export_timebase().frame_rate.as_f64();
        self.project
            .loop_region
            .filter(|_| self.export.region)
            .and_then(|region| region.frames(fps))
    }

    /// The frames the export writes: the loop region's, or the whole project's.
    fn export_frames(&self) -> std::ops::Range<usize> {
        self.export_region()
            .unwrap_or_else(|| 0..self.engine.info().map_or(0, |info| info.frames))
    }

    /// What the export renders before the frames it writes.
    fn export_warmup(&self) -> Warmup {
        let needed = self
            .engine
            .node_stats()
            .iter()
            .map(|s| s.warmup_frames)
            .max()
            .unwrap_or(0);
        warmup_before(self.export_frames().start, needed)
    }

    /// The whole project or only its loop region, and the span and frames that makes.
    fn export_range(&mut self, ui: &mut Ui) {
        let theme = Theme::of(ui.ctx());
        section_label(ui, tr("export.range"));
        let has_region = self.project.loop_region.is_some();
        let options = [
            (tr("export.range.whole"), tr("export.range.whole.help")),
            (
                tr("export.range.region"),
                if has_region {
                    tr("export.range.region.help")
                } else {
                    tr("export.range.no_region")
                },
            ),
        ];
        let chosen = usize::from(self.export.region && has_region);
        if let Some(choice) = segmented(ui, &options, chosen)
            && has_region
        {
            self.export.region = choice == 1;
        }
        let fps = self.export_timebase().frame_rate.as_f64();
        let frames = self.export_frames();
        ui.label(
            RichText::new(tr_args(
                "export.range.span",
                &[
                    ("start", &timecode(frames.start as f64 / fps)),
                    ("end", &timecode(frames.end as f64 / fps)),
                    ("frames", &frames.len().to_string()),
                ],
            ))
            .monospace()
            .size(11.0)
            .color(theme.surfaces.text_faint),
        );
    }

    /// The project's size, or a size of its own; an odd side is outlined.
    fn export_size(&mut self, ui: &mut Ui, kind: FormatKind) {
        let theme = Theme::of(ui.ctx());
        let (width, height) = self.export_output_size();
        let (even_width, even_height) = kind.nearest_size(width, height);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let options = [
                (tr("export.size.project"), tr("export.size.project.help")),
                (tr("export.size.custom"), tr("export.size.help")),
            ];
            let custom = self.export.size.is_some();
            if let Some(choice) = segmented(ui, &options, usize::from(custom)) {
                self.export.size = (choice == 1).then_some((width, height));
            }
            match &mut self.export.size {
                Some((w, h)) => {
                    for (i, (value, odd)) in [(w, even_width != width), (h, even_height != height)]
                        .into_iter()
                        .enumerate()
                    {
                        if i == 1 {
                            ui.label(RichText::new("×").color(theme.surfaces.text_faint));
                        }
                        let mut size = f64::from(*value);
                        let response = ui.add(
                            ValueBox::new(&mut size)
                                .range(16.0..=8192.0)
                                .max_decimals(0)
                                .speed(2.0)
                                .suffix(tr("settings.resolution.suffix")),
                        );
                        if odd {
                            ui.painter().rect_stroke(
                                response.rect,
                                CONTROL_RADIUS,
                                Stroke::new(1.0, theme.warning_outline),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if response.changed() {
                            *value = size.round() as u32;
                        }
                    }
                }
                None => {
                    ui.label(
                        RichText::new(format!("{width}×{height}"))
                            .monospace()
                            .color(theme.text_dim),
                    );
                }
            }
        });
    }

    /// The file the export writes, or wrote: its name only.
    fn export_file_name(&self, kind: FormatKind) -> String {
        let path = match (&self.running_export, &self.export_outcome) {
            (Some(run), _) => Some(&run.path),
            (None, Some(ExportState::Done(path))) => Some(path),
            _ => None,
        };
        path.and_then(|p| p.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}.{}", self.project_name(), kind.extension()))
    }

    /// The right column: the latest frame, the summary, then at the bottom the progress and the
    /// Export… button.
    fn export_result(&mut self, ui: &mut Ui) {
        let theme = Theme::of(ui.ctx());
        let fps = self.export_timebase().frame_rate.as_f64();
        ui.spacing_mut().item_spacing.y = 12.0;
        ui.horizontal(|ui| {
            section_label(ui, tr("export.latest"));
            if let Some(frame) = &self.export.frame {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(tr_args(
                            "export.latest.frame",
                            &[
                                ("frame", &frame.index.to_string()),
                                ("time", &timecode(frame.index as f64 / fps)),
                            ],
                        ))
                        .monospace()
                        .size(11.0)
                        .color(theme.text_dim),
                    );
                });
            }
        });
        self.export_preview(ui);

        let format = self.export_format();
        let kind = format.map_or(FormatKind::H264, |f| f.kind());
        let (width, height) = self.export_output_size();
        let frames = self.export_frames();
        let warmup = self.export_warmup();
        ui.add_space(4.0);
        section_label(ui, tr("export.summary"));
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            summary_row(ui, tr("export.summary.file"), &self.export_file_name(kind));
            summary_row(
                ui,
                tr("export.summary.picture"),
                &format!("{width}×{height}"),
            );
            summary_row(
                ui,
                tr("export.summary.rate"),
                &frame_rate_label(self.export_timebase().frame_rate),
            );
            summary_row(
                ui,
                tr("export.summary.length"),
                &timecode(frames.len() as f64 / fps),
            );
            let warmup_text = match warmup {
                Warmup::None => tr("export.warmup.none").to_owned(),
                Warmup::Frames(n) => tr_args("export.warmup.frames", &[("frames", &n.to_string())]),
                Warmup::FromStart => tr("export.warmup.from_start").to_owned(),
            };
            summary_row(ui, tr("export.summary.warmup"), &warmup_text);
        });
        let warm_frames = match warmup {
            Warmup::None => 0,
            Warmup::Frames(n) => n,
            Warmup::FromStart => frames.start,
        };
        if warm_frames > 0 {
            let fraction = self.running_export.as_ref().map_or(0.0, |run| {
                warmup_fraction(run.done.load(Ordering::Relaxed), warm_frames)
            });
            let (bar, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 4.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(bar, 2, theme.surfaces.raised_hover);
            ui.painter().rect_filled(
                bar.with_max_x(bar.left() + bar.width() * fraction),
                2,
                theme.accent,
            );
        }

        // Kept to the bottom, top-down, at the height it took last frame (its card grows and
        // shrinks with what it says).
        let height_id = ui.id().with("export-bottom");
        let last_height = ui.data(|d| d.get_temp::<f32>(height_id)).unwrap_or(140.0);
        let max = ui.max_rect();
        let bottom =
            Rect::from_min_max(egui::pos2(max.left(), max.bottom() - last_height), max.max);
        let mut ui = ui.new_child(UiBuilder::new().max_rect(bottom));
        {
            let ui = &mut ui;
            ui.spacing_mut().item_spacing.y = 8.0;
            section_label(ui, tr("export.progress.title"));
            self.export_progress(ui);
            let odd = odd_size_note(kind, width, height).is_some();
            let running = self.running_export.is_some();
            let text = if odd {
                tr("export.start.odd")
            } else {
                tr("export.start")
            };
            let enabled = !odd && !running && format.is_some();
            let button = egui::Button::new(RichText::new(text).color(if enabled {
                theme.surfaces.ground
            } else {
                theme.surfaces.text_faint
            }))
            .fill(if enabled {
                theme.accent
            } else {
                theme.surfaces.raised_active
            });
            let size = vec2(ui.available_width(), 40.0);
            let clicked = ui
                .add_enabled_ui(enabled, |ui| ui.add_sized(size, button))
                .inner
                .clicked();
            if clicked && let Some(path) = self.pick_export_path(kind) {
                self.start_export(path);
            }
        }
        let used = ui.min_rect().height();
        if (used - last_height).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(height_id, used));
            ui.ctx().request_repaint();
        }
    }

    /// The latest frame written, fitted into a 16:9 box, tagged with the output size and, while
    /// the export runs, "Rendering".
    fn export_preview(&self, ui: &mut Ui) {
        let theme = Theme::of(ui.ctx());
        let width = ui.available_width();
        let (rect, _) =
            ui.allocate_exact_size(vec2(width, width * 9.0 / 16.0), egui::Sense::hover());
        ui.painter().rect(
            rect,
            CONTROL_RADIUS,
            theme.preview_bg,
            Stroke::new(1.0, theme.surfaces.border),
            egui::StrokeKind::Outside,
        );
        if let Some(frame) = &self.export.frame {
            let aspect = frame.width as f32 / frame.height.max(1) as f32;
            let size = if aspect > rect.aspect_ratio() {
                vec2(rect.width(), rect.width() / aspect)
            } else {
                vec2(rect.height() * aspect, rect.height())
            };
            let fit = Rect::from_center_size(rect.center(), size);
            let mut inner = ui.new_child(UiBuilder::new().max_rect(fit));
            picture(
                &mut inner,
                ui.id().with("export-frame"),
                self.export.frame_version,
                [frame.width, frame.height],
                &frame.rgb,
                fit.width(),
            );
        }
        let corner = rect.shrink(6.0);
        if self.running_export.is_some() {
            preview_tag(
                ui,
                corner.left_bottom(),
                egui::Align2::LEFT_BOTTOM,
                tr("export.rendering"),
                Some(theme.error),
            );
        }
        let (w, h) = self.export_output_size();
        preview_tag(
            ui,
            corner.right_bottom(),
            egui::Align2::RIGHT_BOTTOM,
            &format!("{w}×{h}"),
            None,
        );
    }

    /// Asks where to write a file of `kind`.
    fn pick_export_path(&self, kind: FormatKind) -> Option<PathBuf> {
        let extension = kind.extension();
        let folder = self
            .project_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_owned);
        let mut dialog = rfd::FileDialog::new()
            .add_filter(format_name(kind), &[extension])
            .set_file_name(format!("{}.{extension}", self.project_name()));
        if let Some(folder) = folder {
            dialog = dialog.set_directory(folder);
        }
        dialog.save_file()
    }

    /// The progress card: frames done, time left, a bar and Cancel while it runs; afterwards
    /// where the file went, or why it didn't.
    fn export_progress(&mut self, ui: &mut Ui) {
        let theme = Theme::of(ui.ctx());
        let mut cancel = false;
        egui::Frame::new()
            .fill(theme.surfaces.inner_card)
            .stroke(Stroke::new(1.0, theme.surfaces.border))
            .corner_radius(CARD_RADIUS)
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    match (&self.running_export, &self.export_outcome) {
                        (Some(run), _) => {
                            let done = run.done.load(Ordering::Relaxed);
                            let total = run.total.load(Ordering::Relaxed);
                            let fraction = if total == 0 {
                                0.0
                            } else {
                                done as f32 / total as f32
                            };
                            let elapsed = run.started.elapsed().as_secs_f64();
                            let left = if done == 0 {
                                String::from("–")
                            } else {
                                duration_label(elapsed / done as f64 * (total - done) as f64)
                            };
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(tr_args(
                                        "export.progress",
                                        &[
                                            ("done", &done.to_string()),
                                            ("total", &total.to_string()),
                                        ],
                                    ))
                                    .size(12.0),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(tr_args("export.left", &[("left", &left)]))
                                            .monospace()
                                            .size(SMALL_SIZE + 1.0)
                                            .color(theme.text_dim),
                                    );
                                });
                            });
                            let (bar, _) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 6.0),
                                egui::Sense::hover(),
                            );
                            ui.painter()
                                .rect_filled(bar, 3, theme.surfaces.raised_hover);
                            let filled = bar.with_max_x(bar.left() + bar.width() * fraction);
                            ui.painter().rect_filled(filled, 3, theme.accent);
                            let cancelling = run.cancel.load(Ordering::Relaxed);
                            cancel = ui
                                .add_enabled(!cancelling, egui::Button::new(tr("dialog.cancel")))
                                .clicked();
                        }
                        (None, Some(ExportState::Done(path))) => {
                            ui.label(
                                RichText::new(tr_args(
                                    "export.done",
                                    &[("path", &path.display().to_string())],
                                ))
                                .size(12.0),
                            );
                        }
                        (None, Some(ExportState::Cancelled)) => {
                            ui.label(RichText::new(tr("export.cancelled")).size(12.0));
                        }
                        (None, Some(ExportState::Failed(message))) => {
                            ui.label(RichText::new(message).size(12.0).color(theme.error));
                        }
                        _ => {
                            ui.label(
                                RichText::new(tr("export.progress.idle"))
                                    .size(12.0)
                                    .color(theme.surfaces.text_faint),
                            );
                        }
                    }
                });
            });
        if cancel {
            self.cancel_export();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_as_clock_time() {
        assert_eq!(duration_label(0.4), "0:00");
        assert_eq!(duration_label(65.0), "1:05");
        assert_eq!(duration_label(3725.0), "1:02:05");
    }

    #[test]
    fn an_odd_size_names_its_odd_side() {
        assert_eq!(odd_size_note(FormatKind::H264, 1280, 720), None);
        assert_eq!(odd_size_note(FormatKind::Ffv1, 1281, 721), None);
        assert_eq!(
            odd_size_note(FormatKind::H264, 1281, 720).unwrap(),
            "H.264 needs an even width; 1281 is odd."
        );
        assert_eq!(
            odd_size_note(FormatKind::ProRes, 1281, 721).unwrap(),
            "ProRes needs an even width and height; 1281×721 is odd."
        );
    }

    #[test]
    fn warmup_bar_fills_while_the_warmup_frames_render() {
        assert_eq!(warmup_fraction(0, 10), 0.0);
        assert_eq!(warmup_fraction(5, 10), 0.5);
        assert_eq!(warmup_fraction(40, 10), 1.0);
        assert_eq!(warmup_fraction(3, 0), 0.0);
    }

    #[test]
    fn warmup_reaches_back_as_far_as_the_graph_remembers() {
        assert_eq!(warmup_before(0, 10), Warmup::None);
        assert_eq!(warmup_before(30, 0), Warmup::None);
        assert_eq!(warmup_before(30, 10), Warmup::Frames(10));
        // Never before the first frame.
        assert_eq!(warmup_before(4, 10), Warmup::Frames(4));
        assert_eq!(warmup_before(30, UNBOUNDED_WARMUP), Warmup::FromStart);
    }
}
