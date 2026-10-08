//! The Settings window: application settings (kept for the user) and project settings (saved
//! in the project file), each with a line of help.

use eframe::egui::{self, RichText, Ui};
use rastersong_engine::{
    Bus, INSPECT_RATE_RANGE, MAX_BUS_CHANNELS, MAX_WARMUP_FRAMES_LIMIT, PreviewScale, Rational,
    Tempo, Timebase, UNBOUNDED_WARMUP,
};
use rastersong_lang::{tr, tr_args};

use super::{AUDIO_RATES, App};
use crate::name_edit::name_edit;
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
            .small_button("\u{21ba}")
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

/// The frame rates the Picture section offers, NTSC rates as exact fractions.
const FRAME_RATES: [Rational; 8] = [
    Rational::new(24000, 1001),
    Rational::new(24, 1),
    Rational::new(25, 1),
    Rational::new(30000, 1001),
    Rational::new(30, 1),
    Rational::new(50, 1),
    Rational::new(60000, 1001),
    Rational::new(60, 1),
];

/// The widths and heights a project can render at.
const PICTURE_SIZE_RANGE: std::ops::RangeInclusive<f64> = 16.0..=8192.0;

/// "29.97 fps", "25 fps": the rate to at most three decimals.
fn frame_rate_label(rate: Rational) -> String {
    let fps = format!("{:.3}", rate.as_f64());
    let fps = fps.trim_end_matches('0').trim_end_matches('.');
    tr_args("settings.frame_rate.value", &[("fps", fps)])
}

/// Whether two frame rates are the same rate, however they are written (`60/2` is `30/1`).
fn same_rate(a: Rational, b: Rational) -> bool {
    i64::from(a.num) * i64::from(b.den) == i64::from(b.num) * i64::from(a.den)
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
        self.bus_removal_dialog(ctx);
    }

    /// The output buses: name, channels and remove for each, master first, and a button to add
    /// one.
    fn bus_settings(&mut self, ui: &mut Ui) {
        section(ui, tr("settings.buses"));
        let count = self.project.buses.len();
        let mut rename = None;
        let mut remove = None;
        for (i, bus) in self.project.buses.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let edit = name_edit(ui, ui.id().with(("bus-name", i)), &bus.name, |e| {
                    e.desired_width(120.0)
                });
                edit.response.on_hover_text(tr("settings.buses.name.help"));
                if let Some(name) = edit.committed {
                    rename = Some((bus.name.clone(), name));
                }
                egui::ComboBox::from_id_salt(("bus-channels", i))
                    .width(90.0)
                    .selected_text(channels_label(bus.channels))
                    .show_ui(ui, |ui| {
                        for channels in 1..=MAX_BUS_CHANNELS {
                            ui.selectable_value(
                                &mut bus.channels,
                                channels,
                                channels_label(channels),
                            );
                        }
                    })
                    .response
                    .on_hover_text(tr("settings.buses.channels.help"));
                if i == 0 {
                    ui.weak(tr("settings.buses.master"))
                        .on_hover_text(tr("settings.buses.master.help"));
                } else if ui
                    .add_enabled(count > 1, egui::Button::new("×").frame(false))
                    .on_hover_text(tr("settings.buses.remove"))
                    .clicked()
                {
                    remove = Some(bus.name.clone());
                }
            });
        }
        if ui.button(tr("settings.buses.add")).clicked() {
            let name = self.project.unused_bus_name();
            self.project.buses.push(Bus { name, channels: 2 });
        }
        help(ui, tr("settings.buses.help"));
        if let Some((old, new)) = rename {
            self.project.rename_bus(&old, &new);
        }
        if let Some(name) = remove {
            let (tracks, outputs) = self.project.bus_users(&name);
            if tracks.is_empty() && outputs.is_empty() {
                self.project.remove_bus(&name);
            } else {
                self.bus_removal = Some(name);
            }
        }
    }

    /// The warning before removing a bus something uses, listing the tracks routed to it and the
    /// Audio Outputs writing to it.
    fn bus_removal_dialog(&mut self, ctx: &egui::Context) {
        let Some(name) = self.bus_removal.clone() else {
            return;
        };
        let (tracks, outputs) = self.project.bus_users(&name);
        let master = self
            .project
            .buses
            .iter()
            .map(|b| b.name.as_str())
            .find(|b| *b != name)
            .unwrap_or_default()
            .to_owned();
        let output_names: Vec<String> = outputs
            .iter()
            .map(|id| {
                self.project
                    .graph
                    .nodes
                    .iter()
                    .find(|n| n.id == *id)
                    .and_then(|n| n.label.clone())
                    .unwrap_or_else(|| id.clone())
            })
            .collect();
        let mut close = false;
        egui::Modal::new(egui::Id::new("remove-bus")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading(tr_args("dialog.remove_bus.title", &[("bus", &name)]));
            ui.add_space(4.0);
            if !tracks.is_empty() {
                ui.label(tr_args(
                    "dialog.remove_bus.tracks",
                    &[("master", &master), ("tracks", &tracks.join(", "))],
                ));
            }
            if !output_names.is_empty() {
                ui.label(tr_args(
                    "dialog.remove_bus.outputs",
                    &[("outputs", &output_names.join(", "))],
                ));
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button(tr("dialog.remove")).clicked() {
                    self.project.remove_bus(&name);
                    close = true;
                }
                if ui.button(tr("dialog.cancel")).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    close = true;
                }
            });
        });
        if close {
            self.bus_removal = None;
        }
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
        for (label, audio) in [
            ("settings.view_audio", true),
            ("settings.view_other", false),
        ] {
            ui.horizontal(|ui| {
                ui.label(tr(label));
                let mut view = self.settings.views.of(audio);
                egui::ComboBox::from_id_salt(label)
                    .selected_text(view.label())
                    .show_ui(ui, |ui| {
                        for mode in crate::editor::InspectMode::ALL {
                            ui.selectable_value(&mut view, mode, mode.label());
                        }
                    });
                if audio {
                    self.settings.views.audio = view;
                } else {
                    self.settings.views.other = view;
                }
            });
        }
        help(ui, tr("settings.views.help"));
        ui.checkbox(&mut self.settings.node_stats, tr("settings.node_stats"));
        help(ui, tr("settings.node_stats.help"));
        ui.checkbox(
            &mut self.settings.show_performance,
            tr("settings.performance"),
        );
        help(ui, tr("settings.performance.help"));
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

    /// The project's resolution and frame rate. Until one is set they follow the first video
    /// (or the default without one), shown here as the engine resolved them; editing a field sets
    /// the project's own, and the reset button goes back to following the video.
    fn picture_settings(&mut self, ui: &mut Ui) {
        section(ui, tr("settings.picture"));
        let own = self.project.timebase;
        let mut timebase = own
            .or_else(|| self.engine.info().map(|info| info.timebase))
            .unwrap_or(Timebase::DEFAULT);
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(tr("settings.resolution"));
            for (value, hover) in [
                (&mut timebase.width, "settings.resolution.width"),
                (&mut timebase.height, "settings.resolution.height"),
            ] {
                let mut size = f64::from(*value);
                if ui
                    .add(
                        ValueBox::new(&mut size)
                            .range(PICTURE_SIZE_RANGE)
                            .max_decimals(0)
                            .speed(2.0)
                            .suffix(tr("settings.resolution.suffix")),
                    )
                    .on_hover_text(tr(hover))
                    .changed()
                {
                    *value = size.round() as u32;
                    changed = true;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(tr("settings.frame_rate"));
            let current = timebase.frame_rate;
            egui::ComboBox::from_id_salt("settings-frame-rate")
                .selected_text(frame_rate_label(current))
                .show_ui(ui, |ui| {
                    let other =
                        (!FRAME_RATES.iter().any(|&r| same_rate(r, current))).then_some(current);
                    for rate in other.into_iter().chain(FRAME_RATES) {
                        if ui
                            .selectable_label(same_rate(rate, current), frame_rate_label(rate))
                            .clicked()
                            && rate != current
                        {
                            timebase.frame_rate = rate;
                            changed = true;
                        }
                    }
                });
            if own.is_none() {
                ui.label(RichText::new(tr("settings.picture.from_video")).weak());
            } else if reset_button(ui, true) {
                self.project.timebase = None;
            }
        });
        if changed {
            self.project.timebase = Some(timebase);
        }
        help(ui, tr("settings.picture.help"));
    }

    fn project_settings(&mut self, ui: &mut Ui) {
        self.picture_settings(ui);
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

        section(ui, tr("settings.inspect"));
        ui.horizontal(|ui| {
            ui.label(tr("settings.inspect_rate"));
            ui.add(
                ValueBox::new(&mut self.project.inspect_rate)
                    .range(INSPECT_RATE_RANGE)
                    .max_decimals(0)
                    .speed(0.2)
                    .suffix(tr("settings.inspect_rate.suffix")),
            );
        });
        help(ui, tr("settings.inspect_rate.help"));

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
        self.bus_settings(ui);
    }
}

/// "Mono", "Stereo", "5.1", … for a bus of `channels`.
fn channels_label(channels: u32) -> String {
    match channels {
        1 => tr("settings.buses.mono").to_owned(),
        2 => tr("settings.buses.stereo").to_owned(),
        6 => tr("settings.buses.surround_5_1").to_owned(),
        8 => tr("settings.buses.surround_7_1").to_owned(),
        n => tr_args("settings.buses.channels", &[("count", &n.to_string())]),
    }
}
