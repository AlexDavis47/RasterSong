//! The inspector: name, shared node settings and parameters of the active node.

use std::collections::BTreeMap;

use eframe::egui::{self, RichText, Ui};
use rastersong_engine::{
    Channels, Interpolation, ModMode, Modulation, ParamKind, ParamLevel, ParamSpec, ParamValue,
};

use super::param_field::{GUTTER_WIDTH, Modulated, NumberRange, param_field, reset_gesture};
use super::{GraphEditor, param_port};
use crate::name_edit::name_edit;
use crate::theme::Theme;

/// What the inspector needs from outside the editor.
#[derive(Debug, Default)]
pub struct InspectorContext<'a> {
    /// Names of the project's audio tracks, offered by audio inputs.
    pub tracks: &'a [String],
    /// Modulated parameters' values at the playhead, for their ghost handles.
    pub params: &'a [ParamLevel],
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
        let linked = self.is_linked(key);
        // The name a linked node shows is the project's, so renaming it renames that.
        let linked_name = self.node(key).and_then(|n| self.linked_name(n));
        let mut renamed: Option<String> = None;
        // Per parameter: whether its pin shows, and the colour of the wire modulating it, if any.
        let theme = Theme::of(ui.ctx());
        let pins: Vec<(bool, Option<egui::Color32>)> = {
            let node = self.node(key).unwrap();
            let count = kind.as_ref().map_or(0, |k| k.spec.params.len());
            (0..count)
                .map(|i| {
                    let wire = self.wires.iter().find(|w| w.to == (key, param_port(i)));
                    let color = wire.map(|w| self.output_color(w.from.0, w.from.1, theme).solid());
                    (self.param_exposed(node, i), color)
                })
                .collect()
        };
        let mut toggled: Option<(usize, bool)> = None;
        let mut disconnect: Option<usize> = None;
        let node = self.nodes.iter_mut().find(|n| n.key == key).unwrap();
        let Some(kind) = kind else {
            ui.colored_label(
                Theme::of(ui.ctx()).error,
                format!("Unknown node type `{}`", node.kind),
            );
            return;
        };

        // Name: shown on the node instead of the type.
        if let Some(current) = &linked_name {
            let edit = name_edit(ui, egui::Id::new(("linked-name", key)), current, |e| {
                e.font(egui::TextStyle::Heading)
                    .desired_width(f32::INFINITY)
            });
            edit.response.on_hover_text(
                "Name shown on the node. It's the project's name for this input, so the \
                 timeline shows it too.",
            );
            renamed = edit.committed;
        } else {
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
                        choice(ui, "resampling", &mut node.interpolation, Interpolation::default(), &[
                            (Interpolation::Hold, "Hold", "Repeat samples; a pixel's R, G and B move together"),
                            (Interpolation::Linear, "Linear", "Ramp smoothly between samples"),
                        ]);
                        ui.end_row();
                    }
                    if kind.spec.per_channel {
                        ui.label("Channels").on_hover_text(
                            "How this node treats the red, green and blue of each pixel.",
                        );
                        choice(ui, "channels", &mut node.channels, Channels::default(), &[
                            (
                                Channels::Together,
                                "Together",
                                "Runs R, G, B, R, G, B… through the node as one stream. Colors bleed \
                                 into each other, as in a low pass or a modulated delay.",
                            ),
                            (
                                Channels::Separate,
                                "Separate",
                                "Runs red, green and blue each through their own copy of the node. \
                                 The same as Split → node ×3 → Combine.",
                            ),
                        ]);
                        ui.end_row();
                    }
                });
        }

        if !kind.spec.params.is_empty() {
            section(ui, "Parameters");
            // Sized from the panel once: sizing from the row's own contents would feed back
            // through the layout and make sliders change size while dragged.
            let track_width = (ui.available_width() - CONTROL_ROOM).max(MIN_TRACK);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for (index, spec) in kind.spec.params.iter().enumerate() {
                    // A linked input's source is the project's: shown, not edited.
                    if linked && spec.name == "source" {
                        ui.horizontal(|ui| {
                            ui.add_space(GUTTER_WIDTH + ui.spacing().item_spacing.x);
                            ui.label(spec.label).on_hover_text(spec.help);
                        });
                        let source = super::linked::track_of(node).to_owned();
                        let shown = if node.kind == super::linked::VIDEO_INPUT {
                            "The project's video".to_owned()
                        } else {
                            format!("Track \"{source}\"")
                        };
                        ui.horizontal(|ui| {
                            ui.add_space(GUTTER_WIDTH + ui.spacing().item_spacing.x);
                            ui.weak(shown).on_hover_text(
                                "Linked to the project: rename or remove it in the timeline",
                            );
                        });
                        ui.add_space(PARAM_GAP);
                        continue;
                    }
                    let tracks =
                        (node.kind == "audio_input" && spec.name == "source").then_some(ctx.tracks);
                    let (exposed, wire) = pins[index];
                    let mut modulation = wire.map(|color| {
                        let current = node.modulation.get(spec.name).copied();
                        let base = spec.number_value(&node.params).unwrap_or(0.0);
                        let value = current.unwrap_or(Modulation {
                            amount: spec.default_modulation_amount(base),
                            mode: ModMode::Bipolar,
                        });
                        (value, current, color)
                    });
                    let mut expose = spec.modulatable.then_some(exposed);
                    let live = ctx
                        .params
                        .iter()
                        .find(|p| *p.node == node.id && p.index == index)
                        .map(|p| f64::from(p.value));
                    let disconnected = param_row(ParamRow {
                        ui,
                        spec,
                        params: &mut node.params,
                        tracks,
                        track_width,
                        expose: expose.as_mut(),
                        modulation: modulation.as_mut().map(|(m, _, c)| (m, *c)),
                        live,
                    });
                    ui.add_space(PARAM_GAP);
                    if disconnected {
                        disconnect = Some(index);
                    }
                    if expose.is_some_and(|e| e != exposed) {
                        toggled = Some((index, !exposed));
                    }
                    if let Some((value, current, _)) = modulation
                        && Some(value) != current
                    {
                        node.modulation.insert(spec.name.to_owned(), value);
                    }
                }
            });
        } else if !shared_settings {
            ui.add_space(8.0);
            ui.weak("No settings.");
        }
        if let Some((index, exposed)) = toggled {
            self.set_param_exposed(key, index, exposed);
        }
        if let Some(index) = disconnect {
            self.disconnect_input((key, param_port(index)));
        }
        if let Some(name) = renamed
            && let Some(request) = self.node(key).and_then(|n| self.rename_request(n, name))
        {
            self.renames.push(request);
        }
    }
}

/// What one parameter row shows and edits.
struct ParamRow<'a, 'u> {
    ui: &'u mut Ui,
    spec: &'a ParamSpec,
    params: &'a mut BTreeMap<String, ParamValue>,
    /// For an audio input's track: the project's tracks.
    tracks: Option<&'a [String]>,
    track_width: f32,
    /// Whether the parameter's pin shows on the node, for parameters that can be modulated.
    expose: Option<&'a mut bool>,
    /// The modulation of a connected parameter, and the wire's colour.
    modulation: Option<(&'a mut Modulation, egui::Color32)>,
    /// A modulated parameter's value at the playhead.
    live: Option<f64>,
}

/// Width a parameter's control line needs besides the slider: the gutter (under the expose
/// toggle, holding the amount knob), the value box and the spacing between them.
const CONTROL_ROOM: f32 = GUTTER_WIDTH + VALUE_WIDTH + 24.0;
/// The narrowest a slider gets in a narrow inspector.
const MIN_TRACK: f32 = 60.0;
/// Minimum width of a slider's value box, so it doesn't resize as the digits change.
const VALUE_WIDTH: f32 = 58.0;
/// Space between parameters.
const PARAM_GAP: f32 = 6.0;

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(12.0);
    ui.label(RichText::new(title).strong());
    ui.separator();
}

/// A drop-down of `options` (value, label, help). Alt+click or right-click resets it to `default`.
fn choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    default: T,
    options: &[(T, &str, &str)],
) {
    let current = options.iter().find(|o| o.0 == *value).map_or("", |o| o.1);
    let combo = egui::ComboBox::from_id_salt(id)
        .selected_text(current)
        .width(150.0)
        .show_ui(ui, |ui| {
            for &(option, label, help) in options {
                ui.selectable_value(value, option, label)
                    .on_hover_text(help);
            }
        })
        .response;
    if reset_gesture(ui, &combo, *value != default) {
        *value = default;
        egui::Popup::close_all(ui.ctx());
    }
}

/// One parameter, on two lines: the expose toggle, label and reset button; then its editor (with
/// the modulation of a connected parameter). Values equal to the default are not stored, which
/// keeps graph files short. Returns true if the user asked to disconnect the modulating signal.
fn param_row(row: ParamRow) -> bool {
    let ParamRow {
        ui,
        spec,
        params,
        tracks,
        track_width,
        expose,
        mut modulation,
        live,
    } = row;
    let mut disconnect = false;
    let default = spec.default_value();
    let mut value = params
        .get(spec.name)
        .cloned()
        .unwrap_or_else(|| default.clone());
    let mut reset = false;
    let value_differs = value != default;
    ui.horizontal(|ui| {
        match expose {
            Some(exposed) => {
                let theme = Theme::of(ui.ctx());
                let color = if *exposed {
                    theme.accent
                } else {
                    ui.visuals().weak_text_color()
                };
                let hover = if *exposed {
                    "Hide this parameter's modulation pin (disconnects it)"
                } else {
                    "Show a pin on the node to modulate this parameter with a signal"
                };
                if diamond_toggle(ui, *exposed, color)
                    .on_hover_text(hover)
                    .clicked()
                {
                    *exposed = !*exposed;
                }
            }
            None => {
                ui.allocate_space(egui::vec2(GUTTER_WIDTH, 14.0));
            }
        }
        ui.label(spec.label).on_hover_text(spec.help);
        if !spec.unit.is_empty() {
            ui.weak(spec.unit);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            reset = ui
                .add_enabled(
                    value != default,
                    egui::Button::new("↺").small().frame(false),
                )
                .on_hover_text("Reset to default")
                .clicked();
        });
    });
    // Controls other than numbers start where a number's track does, past the gutter.
    let indent = GUTTER_WIDTH + ui.spacing().item_spacing.x;
    match (spec.kind, &mut value) {
        (
            ParamKind::Number {
                min,
                max,
                limit_min,
                limit_max,
                ..
            },
            ParamValue::Number(n),
        ) => {
            let range = NumberRange {
                default: match &default {
                    ParamValue::Number(d) => *d,
                    _ => min,
                },
                soft: (min, max),
                limits: (limit_min, limit_max),
            };
            ui.horizontal(|ui| {
                let modulated = modulation.as_mut().map(|(m, color)| Modulated {
                    spec,
                    modulation: m,
                    color: *color,
                    live,
                });
                let response =
                    param_field(ui, spec.name, n, range, track_width, VALUE_WIDTH, modulated);
                disconnect = response.disconnect;
            });
        }
        (ParamKind::Choice { options, .. }, ParamValue::Text(s)) => {
            ui.horizontal(|ui| {
                ui.add_space(indent);
                let combo = egui::ComboBox::from_id_salt(spec.name)
                    .selected_text(s.as_str())
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for &option in options {
                            ui.selectable_value(s, option.to_owned(), option);
                        }
                    })
                    .response;
                if reset_gesture(ui, &combo, value_differs) {
                    reset = true;
                    egui::Popup::close_all(ui.ctx());
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
                let combo = egui::ComboBox::from_id_salt(spec.name)
                    .selected_text(RichText::new(shown).color(if missing {
                        Theme::of(ui.ctx()).error
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
                    })
                    .response;
                if reset_gesture(ui, &combo, value_differs) {
                    reset = true;
                    egui::Popup::close_all(ui.ctx());
                }
            }
            None => {
                let field = ui.add(egui::TextEdit::singleline(s).desired_width(150.0));
                if reset_gesture(ui, &field, value_differs) {
                    reset = true;
                }
            }
        },
        // A value of the wrong type (from a hand-edited file): offer to reset it.
        _ => {
            ui.colored_label(Theme::of(ui.ctx()).error, format!("{value:?}"));
        }
    }
    if reset {
        value = default.clone();
    }
    if value == default {
        params.remove(spec.name);
    } else {
        params.insert(spec.name.to_owned(), value);
    }
    disconnect
}

/// The expose toggle: a diamond like the parameter pins, filled when the pin is shown. It
/// takes the width of the gutter, so the label after it lines up with the controls below.
fn diamond_toggle(ui: &mut Ui, on: bool, color: egui::Color32) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(GUTTER_WIDTH, 14.0), egui::Sense::click());
    let c = rect.center();
    let r = if response.hovered() { 5.5 } else { 4.5 };
    let points = vec![
        c + egui::vec2(0.0, -r),
        c + egui::vec2(r, 0.0),
        c + egui::vec2(0.0, r),
        c + egui::vec2(-r, 0.0),
    ];
    let fill = if on {
        color
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().add(egui::Shape::convex_polygon(
        points,
        fill,
        egui::Stroke::new(1.2, color),
    ));
    response
}
