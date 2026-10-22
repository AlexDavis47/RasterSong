//! The Export window: format, bus and size, then a render of the whole project into a file on
//! a worker thread, with progress, time left and Cancel.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};
use rastersong_engine::export::{ExportJob, ExportSettings, export};
use rastersong_engine::{
    EngineError, ExportFormat, FormatKind, ProResProfile, Quality, RenderInfo, Timebase,
    export_formats,
};
use rastersong_lang::{tr, tr_args};

use super::App;
use crate::value_box::ValueBox;

/// What the Export window is set to, kept while the app runs.
#[derive(Debug, Clone)]
pub(super) struct ExportDialog {
    pub open: bool,
    format: ExportFormat,
    /// The bus written; `None` is the master.
    bus: Option<String>,
    /// Output size; `None` is the project's.
    size: Option<(u32, u32)>,
}

impl Default for ExportDialog {
    fn default() -> Self {
        Self {
            open: false,
            format: FormatKind::H264.default_format(),
            bus: None,
            size: None,
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

/// The name a format is listed under.
fn format_name(kind: FormatKind) -> &'static str {
    match kind {
        FormatKind::H264 => tr("export.format.h264"),
        FormatKind::ProRes => tr("export.format.prores"),
        FormatKind::Ffv1 => tr("export.format.ffv1"),
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
            frames: None,
            bus,
        };
        let done = Arc::new(AtomicUsize::new(0));
        let total = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let backend = self.backend.clone();
        let cache = self.audio_cache.clone();
        let thread = {
            let (done, total, cancel, path) =
                (done.clone(), total.clone(), cancel.clone(), path.clone());
            std::thread::Builder::new()
                .name("export".into())
                .spawn(move || {
                    export(
                        backend.as_ref(),
                        cache.as_ref(),
                        &job,
                        &settings,
                        &path,
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
        self.running_export = Some(RunningExport {
            path,
            done,
            total,
            cancel,
            started: Instant::now(),
            thread,
        });
    }

    /// Picks up a finished export.
    fn poll_export(&mut self) {
        if !self
            .running_export
            .as_ref()
            .is_some_and(|run| run.thread.is_finished())
        {
            return;
        }
        let run = self.running_export.take().expect("checked");
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
            // Keep the progress moving while nothing else repaints.
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let mut open = self.export.open;
        egui::Window::new(tr("export.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                if self.running_export.is_some() {
                    self.export_progress(ui);
                } else {
                    self.export_settings(ui);
                }
            });
        // Closing the window doesn't stop an export; reopening it shows the progress.
        self.export.open = open;
    }

    fn export_settings(&mut self, ui: &mut Ui) {
        let available = export_formats();
        if available.is_empty() {
            ui.label(tr("export.no_formats"));
            return;
        }
        let Some(mut format) = self.export_format() else {
            return;
        };

        egui::Grid::new("export-settings")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label(tr("export.format"));
                egui::ComboBox::from_id_salt("export-format")
                    .selected_text(format_name(format.kind()))
                    .show_ui(ui, |ui| {
                        for f in available {
                            if ui
                                .selectable_label(f.kind == format.kind(), format_name(f.kind))
                                .clicked()
                                && f.kind != format.kind()
                            {
                                format = f.kind.default_format();
                            }
                        }
                    });
                ui.end_row();

                match &mut format {
                    ExportFormat::H264 { quality } => {
                        ui.label(tr("export.quality"));
                        egui::ComboBox::from_id_salt("export-quality")
                            .selected_text(quality_name(*quality))
                            .show_ui(ui, |ui| {
                                for q in Quality::ALL {
                                    ui.selectable_value(quality, q, quality_name(q));
                                }
                            });
                        ui.end_row();
                    }
                    ExportFormat::ProRes { profile } => {
                        ui.label(tr("export.profile"));
                        egui::ComboBox::from_id_salt("export-profile")
                            .selected_text(profile_name(*profile))
                            .show_ui(ui, |ui| {
                                for p in ProResProfile::ALL {
                                    ui.selectable_value(profile, p, profile_name(p));
                                }
                            });
                        ui.end_row();
                    }
                    ExportFormat::Ffv1 => {}
                }

                let buses = ExportJob::from_project(&self.project).buses();
                let master = buses[0].name.clone();
                let mut bus = self.export.bus.clone().unwrap_or_else(|| master.clone());
                ui.label(tr("export.bus"));
                egui::ComboBox::from_id_salt("export-bus")
                    .selected_text(&bus)
                    .show_ui(ui, |ui| {
                        for b in &buses {
                            ui.selectable_value(&mut bus, b.name.clone(), &b.name);
                        }
                    });
                ui.end_row();
                self.export.bus = (bus != master).then_some(bus);

                ui.label(tr("export.size"));
                self.export_size(ui);
                ui.end_row();
            });
        self.export.format = format;

        let encoder = available
            .iter()
            .find(|f| f.kind == format.kind())
            .map_or("", |f| f.video_encoder);
        ui.label(
            RichText::new(tr_args("export.encoder", &[("encoder", encoder)]))
                .weak()
                .small(),
        );

        let (width, height) = self.export_output_size();
        let odd = format.kind().needs_even_size() && (width % 2 == 1 || height % 2 == 1);
        if odd {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                tr_args(
                    "export.odd_size",
                    &[
                        ("width", &width.to_string()),
                        ("height", &height.to_string()),
                    ],
                ),
            );
        }

        match &self.export_outcome {
            Some(ExportState::Done(path)) => {
                ui.label(tr_args(
                    "export.done",
                    &[("path", &path.display().to_string())],
                ));
            }
            Some(ExportState::Cancelled) => {
                ui.label(tr("export.cancelled"));
            }
            Some(ExportState::Failed(message)) => {
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
            _ => {}
        }

        ui.add_space(6.0);
        if ui
            .add_enabled(!odd, egui::Button::new(tr("export.start")))
            .clicked()
            && let Some(path) = self.pick_export_path(format.kind())
        {
            self.start_export(path);
        }
    }

    /// The size the export renders at: the chosen one, or the project's.
    fn export_output_size(&self) -> (u32, u32) {
        self.export.size.unwrap_or_else(|| {
            let timebase = self
                .project
                .timebase
                .or_else(|| self.engine.info().map(|info| info.timebase))
                .unwrap_or(Timebase::DEFAULT);
            (timebase.width, timebase.height)
        })
    }

    /// The project's size, or a size of its own.
    fn export_size(&mut self, ui: &mut Ui) {
        let (width, height) = self.export_output_size();
        ui.horizontal(|ui| {
            let mut custom = self.export.size.is_some();
            if ui
                .checkbox(&mut custom, tr("export.size.custom"))
                .on_hover_text(tr("export.size.help"))
                .changed()
            {
                self.export.size = custom.then_some((width, height));
            }
            match &mut self.export.size {
                Some((w, h)) => {
                    for value in [w, h] {
                        let mut size = f64::from(*value);
                        if ui
                            .add(
                                ValueBox::new(&mut size)
                                    .range(16.0..=8192.0)
                                    .max_decimals(0)
                                    .speed(2.0)
                                    .suffix(tr("settings.resolution.suffix")),
                            )
                            .changed()
                        {
                            *value = size.round() as u32;
                        }
                    }
                }
                None => {
                    ui.label(format!("{width}×{height}"));
                }
            }
        });
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

    fn export_progress(&mut self, ui: &mut Ui) {
        let Some(run) = &self.running_export else {
            return;
        };
        let done = run.done.load(Ordering::Relaxed);
        let total = run.total.load(Ordering::Relaxed);
        let fraction = if total == 0 {
            0.0
        } else {
            done as f32 / total as f32
        };
        ui.label(tr_args(
            "export.writing",
            &[("path", &run.path.display().to_string())],
        ));
        ui.add(egui::ProgressBar::new(fraction).show_percentage());
        let elapsed = run.started.elapsed().as_secs_f64();
        let left = if done == 0 {
            String::from("–")
        } else {
            duration_label(elapsed / done as f64 * (total - done) as f64)
        };
        ui.label(tr_args(
            "export.progress",
            &[
                ("done", &done.to_string()),
                ("total", &total.to_string()),
                ("left", &left),
            ],
        ));
        let cancelling = run.cancel.load(Ordering::Relaxed);
        if ui
            .add_enabled(!cancelling, egui::Button::new(tr("dialog.cancel")))
            .clicked()
        {
            self.cancel_export();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::duration_label;

    #[test]
    fn durations_read_as_clock_time() {
        assert_eq!(duration_label(0.4), "0:00");
        assert_eq!(duration_label(65.0), "1:05");
        assert_eq!(duration_label(3725.0), "1:02:05");
    }
}
