//! The Settings window: application settings (kept for the user) and project settings (saved
//! in the project file), each with a line of help.

use eframe::egui::{self, RichText, Ui};
use rastersong_engine::{MAX_WARMUP_FRAMES_LIMIT, PreviewScale, Tempo, UNBOUNDED_WARMUP};
use rastersong_lang::{tr, tr_args};

use super::{AUDIO_RATES, App};
use crate::settings::Settings;
use crate::theme::{ThemeChoice, WireStyle};
use crate::value_box::ValueBox;

/// Which of the window's two pages is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum SettingsTab {
    #[default]
    Application,
    Project,
}

/// A heading for a group of settings.
fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).strong());
    ui.separator();
}

/// A small button that puts a setting back to its default; shown only when it differs.
/// Returns true when it was clicked.
fn reset_button(ui: &mut Ui, differs: bool) -> bool {
    differs
        && ui
            .small_button("{21ba}")
            .on_hover_text(tr("settings.reset"))
            .clicked()
}

/// The help text under a setting.
fn help(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).weak().small());
    ui.add_space(4.0);
}

/// The bpm, beats per bar and first-beat fields, shared by the Settings window and the
/// timeline's tempo bar so the two cannot drift apart.
pub(super) fn tempo_fields(ui: &mut Ui, tempo: &mut Tempo) {
    ui.add(
        ValueBox::new(&mut tempo.bpm)
            .range(Tempo::MIN_BPM..=Tempo::MAX_BPM)
            .speed(0.2)
            .max_decimals(2)
            .suffix(tr("tempo.bpm.suffix")),
    )
    .on_hover_text(tr("tempo.bpm.help"));
    let mut beats = f64::from(tempo.beats_per_bar);
    if ui
        .add(
            ValueBox::new(&mut beats)
                .range(1.0..=64.0)
                .max_decimals(0)
                .suffix(tr("tempo.beats.suffix")),
        )
        .on_hover_text(tr("tempo.beats.help"))
        .changed()
    {
        tempo.beats_per_bar = beats.round() as u32;
    }
    ui.add(
        ValueBox::new(&mut tempo.offset_secs)
            .speed(0.005)
            .max_decimals(3)
            .prefix(tr("tempo.first_beat.prefix"))
            .suffix(tr("unit.seconds.suffix")),
    )
    .on_hover_text(tr("tempo.first_beat.help"));
}

impl App {
    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new(tr("settings.title"))
            .open(&mut open)
            .collapsible(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.settings_tab,
                        SettingsTab::Application,
                        tr("settings.application"),
                    )
                    .on_hover_text(tr("settings.application.help"));
                    ui.selectable_value(
                        &mut self.settings_tab,
                        SettingsTab::Project,
                        tr("settings.project"),
                    )
                    .on_hover_text(tr("settings.project.help"));
                });
                ui.separator();
                match self.settings_tab {
                    SettingsTab::Application => self.application_settings(ui),
                    SettingsTab::Project => self.project_settings(ui),
                }
            });
        self.show_settings = open;
    }

    fn application_settings(&mut self, ui: &mut Ui) {
        section(ui, tr("settings.appearance"));
        ui.horizontal(|ui| {
            ui.label(tr("settings.theme"));
            for choice in ThemeChoice::ALL {
                ui.radio_value(&mut self.settings.theme, choice, choice.label());
            }
        });
        help(ui, tr("settings.theme.help"));
        ui.horizontal(|ui| {
            ui.label(tr("settings.wires"));
            for style in WireStyle::ALL {
                ui.radio_value(&mut self.settings.wire_style, style, style.label());
            }
        });
        help(ui, tr("settings.wires.help"));
        ui.horizontal(|ui| {
            ui.label(tr("settings.language"));
            let current = self.settings.language.clone();
            let mut chosen = current.clone();
            egui::ComboBox::from_id_salt("settings-language")
                .selected_text(&current)
                .show_ui(ui, |ui| {
                    for code in rastersong_lang::available_locales(&crate::settings::locale_dir()) {
                        ui.selectable_value(&mut chosen, code.clone(), code);
                    }
                });
            if chosen != current {
                self.settings.language = chosen;
                self.settings.apply_language();
            }
        });
        help(ui, tr("settings.language.help"));

        section(ui, tr("settings.rendering"));
        ui.horizontal(|ui| {
            ui.label(tr("settings.preview_default"));
            let mut scale = self.settings.preview_scale();
            egui::ComboBox::from_id_salt("settings-preview-scale")
                .selected_text(scale.label())
                .show_ui(ui, |ui| {
                    for option in PreviewScale::ALL {
                        ui.selectable_value(&mut scale, option, option.label());
                    }
                });
            if scale != self.settings.preview_scale() {
                self.settings.set_preview_scale(scale);
                self.engine.set_preview_scale(scale);
            }
        });
        help(ui, tr("settings.preview_default.help"));
        let mut engine_changed = false;
        ui.horizontal(|ui| {
            ui.label(tr("settings.cache"));
            let mut mib = f64::from(self.settings.cache_mib);
            if ui
                .add(
                    ValueBox::new(&mut mib)
                        .range(
                            f64::from(*Settings::CACHE_MIB_RANGE.start())
                                ..=f64::from(*Settings::CACHE_MIB_RANGE.end()),
                        )
                        .max_decimals(0)
                        .speed(16.0)
                        .suffix(tr("settings.cache.suffix")),
                )
                .changed()
            {
                self.settings.cache_mib = mib.round() as u32;
                engine_changed = true;
            }
            if reset_button(ui, self.settings.cache_mib != Settings::DEFAULT_CACHE_MIB) {
                self.settings.cache_mib = Settings::DEFAULT_CACHE_MIB;
                engine_changed = true;
            }
        });
        help(ui, tr("settings.cache.help"));
        ui.horizontal(|ui| {
            ui.label(tr("settings.render_ahead"));
            if ui
                .add(
                    ValueBox::new(&mut self.settings.render_ahead_secs)
                        .range(Settings::RENDER_AHEAD_RANGE)
                        .max_decimals(1)
                        .speed(0.5)
                        .suffix(tr("unit.seconds.suffix")),
                )
                .changed()
            {
                engine_changed = true;
            }
            if reset_button(
                ui,
                self.settings.render_ahead_secs != Settings::DEFAULT_RENDER_AHEAD_SECS,
            ) {
                self.settings.render_ahead_secs = Settings::DEFAULT_RENDER_AHEAD_SECS;
                engine_changed = true;
            }
        });
        help(ui, tr("settings.render_ahead.help"));
        if engine_changed {
            self.engine.set_config(self.settings.engine_config());
        }

        section(ui, tr("settings.graph_editor"));
        ui.checkbox(&mut self.settings.node_stats, tr("settings.node_stats"));
        help(ui, tr("settings.node_stats.help"));
        ui.checkbox(
            &mut self.settings.keep_connections,
            tr("settings.keep_connections"),
        );
        help(ui, tr("settings.keep_connections.help"));
    }

    /// Says so when the graph needs more warmup than the limit allows, and which node needs it.
    fn warmup_warning(&self, ui: &mut Ui) {
        let stats = self.engine.node_stats();
        let Some(worst) = stats.iter().max_by_key(|s| s.warmup_frames) else {
            return;
        };
        let limit = self.project.max_warmup_frames;
        if worst.warmup_frames <= limit {
            return;
        }
        let needs = if worst.warmup_frames == UNBOUNDED_WARMUP {
            tr("settings.warmup.never").to_owned()
        } else {
            tr_args(
                "settings.warmup.needs",
                &[("frames", &worst.warmup_frames.to_string())],
            )
        };
        ui.colored_label(
            ui.visuals().warn_fg_color,
            tr_args(
                "settings.warmup.warning",
                &[
                    ("node", &worst.node),
                    ("needs", &needs),
                    ("limit", &limit.to_string()),
                ],
            ),
        );
    }

    fn project_settings(&mut self, ui: &mut Ui) {
        section(ui, tr("settings.tempo"));
        ui.horizontal_wrapped(|ui| tempo_fields(ui, &mut self.project.tempo));
        help(ui, tr("settings.tempo.help"));

        section(ui, tr("settings.seeking"));
        ui.horizontal(|ui| {
            ui.label(tr("settings.warmup"));
            let mut frames = f64::from(self.project.max_warmup_frames);
            if ui
                .add(
                    ValueBox::new(&mut frames)
                        .range(0.0..=f64::from(MAX_WARMUP_FRAMES_LIMIT))
                        .max_decimals(0)
                        .speed(1.0),
                )
                .changed()
            {
                self.project.max_warmup_frames = frames.round() as u32;
            }
        });
        help(ui, tr("settings.warmup.help"));
        self.warmup_warning(ui);

        section(ui, tr("settings.audio"));
        ui.horizontal(|ui| {
            ui.label(tr("settings.audio_rate"));
            egui::ComboBox::from_id_salt("settings-audio-rate")
                .selected_text(tr_args(
                    "unit.khz",
                    &[(
                        "value",
                        &format!("{:.1}", f64::from(self.project.audio_rate) / 1000.0),
                    )],
                ))
                .show_ui(ui, |ui| {
                    for rate in AUDIO_RATES {
                        ui.selectable_value(
                            &mut self.project.audio_rate,
                            rate,
                            tr_args(
                                "unit.khz",
                                &[("value", &format!("{:.1}", f64::from(rate) / 1000.0))],
                            ),
                        );
                    }
                });
        });
        help(ui, tr("settings.audio_rate.help"));
    }
}
