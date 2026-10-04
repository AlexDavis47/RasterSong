//! The inspector: name, shared node settings and parameters of the active node.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, RichText, Ui};
use rastersong_engine::{Channels, Interpolation, ParamKind, ParamSpec, ParamValue};

use super::GraphEditor;
use super::canvas::ERROR;

/// What the inspector needs from outside the editor.
#[derive(Debug, Default)]
pub struct InspectorContext<'a> {
    /// Names of the project's audio tracks, offered by audio inputs.
    pub tracks: &'a [String],
}

impl GraphEditor {
    pub fn show_inspector(&mut self, ui: &mut Ui, ctx: &InspectorContext) {
        let Some(key) = self.active.filter(|&k| self.node(k).is_some()) else {
            ui.add_space(8.0);
            ui.weak("Click a node to edit it.");
            ui.add_space(4.0);
            ui.weak("Right-click the graph to add nodes.");
            return;
        };
        let kind = self
            .node(key)
            .and_then(|n| self.registry.get(&n.kind))
            .cloned();
        let node = self.nodes.iter_mut().find(|n| n.key == key).unwrap();
        let Some(kind) = kind else {
            ui.colored_label(ERROR, format!("Unknown node type `{}`", node.kind));
            return;
        };

        // Name: shown on the node instead of the type.
        let mut name = node.label.clone().unwrap_or_default();
        let edit = egui::TextEdit::singleline(&mut name)
            .hint_text(kind.spec.label)
            .font(egui::TextStyle::Heading)
            .desired_width(f32::INFINITY);
        if ui
            .add(edit)
            .on_hover_text("Name shown on the node")
            .changed()
        {
            let name = name.trim();
            node.label = (!name.is_empty()).then(|| name.to_owned());
        }
        ui.horizontal(|ui| {
            ui.weak(kind.spec.label);
            ui.weak("·");
            ui.weak(&node.id);
        });
        ui.add_space(2.0);
        ui.label(RichText::new(kind.spec.description).small());

        let shared_settings = kind.inputs.len() > 1 || kind.spec.per_channel;
        if shared_settings {
            section(ui, "Node settings");
            egui::Grid::new("node-settings")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    if kind.inputs.len() > 1 {
                        ui.label("Resampling").on_hover_text(
                            "How the other inputs are stretched or shrunk to the length of the main input",
                        );
                        choice(ui, "resampling", &mut node.interpolation, &[
                            (Interpolation::Hold, "Hold", "Repeat samples; a pixel's R, G and B move together"),
                            (Interpolation::Linear, "Linear", "Ramp smoothly between samples"),
                        ]);
                        ui.end_row();
                    }
                    if kind.spec.per_channel {
                        ui.label("Channels").on_hover_text("How an RGB signal is processed");
                        choice(ui, "channels", &mut node.channels, &[
                            (Channels::Together, "Together", "R, G, B, R, G, B, … as one stream"),
                            (Channels::Separate, "Separate", "R, G and B each processed on their own"),
                        ]);
                        ui.end_row();
                    }
                });
        }

        if !kind.spec.params.is_empty() {
            section(ui, "Parameters");
            egui::Grid::new("params")
                .num_columns(3)
                .spacing([10.0, 8.0])
                .min_col_width(0.0)
                .show(ui, |ui| {
                    for spec in kind.spec.params {
                        let tracks = (node.kind == "audio_input" && spec.name == "source")
                            .then_some(ctx.tracks);
                        param_row(ui, spec, &mut node.params, tracks);
                        ui.end_row();
                    }
                });
        } else if !shared_settings {
            ui.add_space(8.0);
            ui.weak("No settings.");
        }
    }
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(12.0);
    ui.label(RichText::new(title).strong());
    ui.separator();
}

fn choice<T: PartialEq + Copy>(ui: &mut Ui, id: &str, value: &mut T, options: &[(T, &str, &str)]) {
    let current = options.iter().find(|o| o.0 == *value).map_or("", |o| o.1);
    egui::ComboBox::from_id_salt(id)
        .selected_text(current)
        .width(150.0)
        .show_ui(ui, |ui| {
            for &(option, label, help) in options {
                ui.selectable_value(value, option, label)
                    .on_hover_text(help);
            }
        });
}

/// One parameter: label, editor, reset button. Values equal to the default are not stored, which
/// keeps graph files short.
fn param_row(
    ui: &mut Ui,
    spec: &ParamSpec,
    params: &mut BTreeMap<String, ParamValue>,
    tracks: Option<&[String]>,
) {
    ui.label(spec.label).on_hover_text(spec.help);
    let default = spec.default_value();
    let mut value = params
        .get(spec.name)
        .cloned()
        .unwrap_or_else(|| default.clone());
    let suffix = if spec.unit.is_empty() {
        String::new()
    } else {
        format!(" {}", spec.unit)
    };
    match (spec.kind, &mut value) {
        (ParamKind::Number { min, max, .. }, ParamValue::Number(n)) => {
            ui.spacing_mut().slider_width = (ui.available_width() - 70.0).clamp(80.0, 200.0);
            let logarithmic = min > 0.0 && max / min >= 1000.0;
            if logarithmic || max - min <= 2000.0 {
                ui.add(
                    egui::Slider::new(n, min..=max)
                        .logarithmic(logarithmic)
                        .suffix(suffix)
                        .max_decimals(3)
                        .clamping(egui::SliderClamping::Always),
                );
            } else {
                ui.add(
                    egui::DragValue::new(n)
                        .range(min..=max)
                        .suffix(suffix)
                        .max_decimals(3),
                );
            }
        }
        (ParamKind::Choice { options, .. }, ParamValue::Text(s)) => {
            egui::ComboBox::from_id_salt(spec.name)
                .selected_text(s.as_str())
                .width(150.0)
                .show_ui(ui, |ui| {
                    for &option in options {
                        ui.selectable_value(s, option.to_owned(), option);
                    }
                });
        }
        (ParamKind::Text { .. }, ParamValue::Text(s)) => match tracks {
            // Audio inputs pick one of the project's tracks.
            Some(tracks) => {
                let missing = !tracks.contains(s);
                let shown = if missing {
                    format!("{s} (no such track)")
                } else {
                    s.clone()
                };
                egui::ComboBox::from_id_salt(spec.name)
                    .selected_text(RichText::new(shown).color(if missing {
                        ERROR
                    } else {
                        ui.visuals().text_color()
                    }))
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        if tracks.is_empty() {
                            ui.weak("No audio tracks yet");
                        }
                        for track in tracks {
                            ui.selectable_value(s, track.clone(), track);
                        }
                    });
            }
            None => {
                ui.add(egui::TextEdit::singleline(s).desired_width(150.0));
            }
        },
        // A value of the wrong type (from a hand-edited file): offer to reset it.
        _ => {
            ui.colored_label(Color32::LIGHT_RED, format!("{value:?}"));
        }
    }
    let is_default = value == default;
    if ui
        .add_enabled(!is_default, egui::Button::new("↺").small().frame(false))
        .on_hover_text("Reset to default")
        .clicked()
    {
        value = default.clone();
    }
    if value == default {
        params.remove(spec.name);
    } else {
        params.insert(spec.name.to_owned(), value);
    }
}
