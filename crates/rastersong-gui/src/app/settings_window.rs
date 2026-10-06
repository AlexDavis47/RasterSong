//! The Settings window: application settings (kept for the user) and project settings (saved
//! in the project file), each with a line of help.

use eframe::egui::{self, RichText, Ui};
use rastersong_engine::Tempo;

use super::{AUDIO_RATES, App};
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
            .suffix(" bpm"),
    )
    .on_hover_text("Beats per minute. Beat and bar units in nodes follow it.");
    let mut beats = f64::from(tempo.beats_per_bar);
    if ui
        .add(
            ValueBox::new(&mut beats)
                .range(1.0..=64.0)
                .max_decimals(0)
                .suffix(" beats/bar"),
        )
        .on_hover_text("Beats in a bar (the time signature's top number)")
        .changed()
    {
        tempo.beats_per_bar = beats.round() as u32;
    }
    ui.add(
        ValueBox::new(&mut tempo.offset_secs)
            .speed(0.005)
            .max_decimals(3)
            .prefix("first beat ")
            .suffix(" s"),
    )
    .on_hover_text("Seconds from the start of the video to the first beat");
}

impl App {
    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.settings_tab,
                        SettingsTab::Application,
                        "Application",
                    )
                    .on_hover_text("Remembered on this computer, for every project");
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::Project, "Project")
                        .on_hover_text("Saved in the project file");
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
        section(ui, "Appearance");
        ui.horizontal(|ui| {
            ui.label("Theme");
            for choice in ThemeChoice::ALL {
                ui.radio_value(&mut self.settings.theme, choice, choice.label());
            }
        });
        help(ui, "Dark, light, or whichever the system uses.");
        ui.horizontal(|ui| {
            ui.label("Wires");
            for style in WireStyle::ALL {
                ui.radio_value(&mut self.settings.wire_style, style, style.label());
            }
        });
        help(ui, "How connections are drawn in the graph.");

        section(ui, "Graph editor");
        ui.checkbox(
            &mut self.settings.node_stats,
            "Show node latency and warmup",
        );
        help(
            ui,
            "Under each node, show how many frames it delays its output and how many it needs to settle after a seek.",
        );
        ui.checkbox(
            &mut self.settings.keep_connections,
            "Keep input connections when duplicating and pasting",
        );
        help(
            ui,
            "New copies are connected to the same sources as the originals. Hold Shift (Ctrl+Shift+D, Ctrl+Shift+V) to do the opposite once.",
        );
    }

    fn project_settings(&mut self, ui: &mut Ui) {
        section(ui, "Tempo");
        ui.horizontal_wrapped(|ui| tempo_fields(ui, &mut self.project.tempo));
        help(
            ui,
            "The tempo of the music. Beat and bar units in nodes, and the tempo ruler, follow it.",
        );

        section(ui, "Audio");
        ui.horizontal(|ui| {
            ui.label("Audio output rate");
            egui::ComboBox::from_id_salt("settings-audio-rate")
                .selected_text(format!(
                    "{:.1} kHz",
                    f64::from(self.project.audio_rate) / 1000.0
                ))
                .show_ui(ui, |ui| {
                    for rate in AUDIO_RATES {
                        ui.selectable_value(
                            &mut self.project.audio_rate,
                            rate,
                            format!("{:.1} kHz", f64::from(rate) / 1000.0),
                        );
                    }
                });
        });
        help(
            ui,
            "The sample rate of the sound an Audio Output node renders.",
        );
    }
}
