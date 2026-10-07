//! The inspector: name, shared node settings and parameters of the active node.

use std::collections::BTreeMap;

use eframe::egui::{self, RichText, Ui};
use rastersong_engine::{AUDIO_INPUT, SOURCE_PARAM};
use rastersong_engine::{
    Channels, GeneratorLayout, Grouping, Interpolation, MeterKind, Modulation, NodeMeters,
    NodeStats, NodeType, ParamKind, ParamLevel, ParamSpec, ParamValue, Severity, ShownWhen,
};
use rastersong_lang::{tr, tr_args};

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
    /// What the nodes' meters read at the playhead.
    pub meters: &'a [NodeMeters],
}

impl GraphEditor {
    pub fn show_inspector(&mut self, ui: &mut Ui, ctx: &InspectorContext) {
        let Some(key) = self.active.filter(|&k| self.node(k).is_some()) else {
            ui.add_space(8.0);
            ui.weak(tr("editor.inspector.empty_select"));
            ui.add_space(4.0);
            ui.weak(tr("editor.inspector.empty_add"));
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
        // What the last compile found: the node's signals and warnings.
        let compiled = self.compiled(key).cloned();
        let mut toggled: Option<(usize, bool)> = None;
        let mut disconnect: Option<usize> = None;
        let node = self.nodes.iter_mut().find(|n| n.key == key).unwrap();
        let Some(kind) = kind else {
            ui.colored_label(
                Theme::of(ui.ctx()).error,
                tr_args("editor.inspector.unknown_kind", &[("kind", &node.kind)]),
            );
            return;
        };

        // Name: shown on the node instead of the type.
        if let Some(current) = &linked_name {
            let edit = name_edit(ui, egui::Id::new(("linked-name", key)), current, |e| {
                e.font(egui::TextStyle::Heading)
                    .desired_width(f32::INFINITY)
            });
            edit.response
                .on_hover_text(tr("editor.inspector.linked_name_help"));
            renamed = edit.committed;
        } else {
            let mut name = node.label.clone().unwrap_or_default();
            let edit = egui::TextEdit::singleline(&mut name)
                .hint_text(kind.label())
                .font(egui::TextStyle::Heading)
                .desired_width(f32::INFINITY);
            if ui
                .add(edit)
                .on_hover_text(tr("editor.inspector.name_help"))
                .changed()
            {
                let name = name.trim();
                node.label = (!name.is_empty()).then(|| name.to_owned());
            }
        }
        ui.horizontal(|ui| {
            ui.weak(kind.label());
            ui.weak(tr("editor.inspector.separator"));
            ui.weak(&node.id);
        });
        ui.add_space(2.0);
        ui.label(RichText::new(kind.description()).small());
        if let Some(compiled) = &compiled {
            for diagnostic in &compiled.diagnostics {
                ui.add_space(4.0);
                match diagnostic.severity {
                    Severity::Note => ui.label(
                        RichText::new(tr_args(
                            "editor.inspector.note_line",
                            &[("message", &diagnostic.message)],
                        ))
                        .small()
                        .color(theme.text_dim),
                    ),
                    Severity::Warning => ui.colored_label(
                        theme.warning,
                        tr_args(
                            "editor.inspector.warning_line",
                            &[("message", &diagnostic.message)],
                        ),
                    ),
                };
            }
            signals(ui, &kind, compiled);
            node_meters(ui, &kind, ctx.meters, &node.id);
        }

        let shared_settings =
            kind.spec.inputs.len() > 1 || kind.spec.per_channel || kind.spec.takes_layout;
        if shared_settings {
            section(ui, tr("editor.inspector.node_settings"));
            egui::Grid::new("node-settings")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    if kind.spec.inputs.len() > 1 {
                        ui.label(tr("editor.setting.resampling"))
                            .on_hover_text(tr("editor.setting.resampling.help"));
                        choice(
                            ui,
                            "resampling",
                            &mut node.interpolation,
                            Interpolation::default(),
                            &[
                                (
                                    Interpolation::Hold,
                                    tr("editor.setting.resampling.hold"),
                                    tr("editor.setting.resampling.hold.help"),
                                ),
                                (
                                    Interpolation::Linear,
                                    tr("editor.setting.resampling.linear"),
                                    tr("editor.setting.resampling.linear.help"),
                                ),
                            ],
                        );
                        ui.end_row();
                        ui.label(tr("editor.setting.grouping"))
                            .on_hover_text(tr("editor.setting.grouping.help"));
                        choice(
                            ui,
                            "grouping",
                            &mut node.grouping,
                            Grouping::default(),
                            &[
                                (
                                    Grouping::Pixels,
                                    tr("editor.setting.grouping.pixels"),
                                    tr("editor.setting.grouping.pixels.help"),
                                ),
                                (
                                    Grouping::Samples,
                                    tr("editor.setting.grouping.samples"),
                                    tr("editor.setting.grouping.samples.help"),
                                ),
                            ],
                        );
                        ui.end_row();
                    }
                    if kind.spec.takes_layout {
                        ui.label(tr("editor.setting.layout"))
                            .on_hover_text(tr("editor.setting.layout.help"));
                        choice(
                            ui,
                            "layout",
                            &mut node.layout,
                            GeneratorLayout::default(),
                            &[
                                (
                                    GeneratorLayout::Video,
                                    tr("editor.setting.layout.video"),
                                    tr("editor.setting.layout.video.help"),
                                ),
                                (
                                    GeneratorLayout::Audio,
                                    tr("editor.setting.layout.audio"),
                                    tr("editor.setting.layout.audio.help"),
                                ),
                            ],
                        );
                        ui.end_row();
                    }
                    if kind.spec.per_channel {
                        ui.label(tr("editor.setting.channels"))
                            .on_hover_text(tr("editor.setting.channels.help"));
                        choice(
                            ui,
                            "channels",
                            &mut node.channels,
                            Channels::default(),
                            &[
                                (
                                    Channels::Together,
                                    tr("editor.setting.channels.together"),
                                    tr("editor.setting.channels.together.help"),
                                ),
                                (
                                    Channels::Separate,
                                    tr("editor.setting.channels.separate"),
                                    tr("editor.setting.channels.separate.help"),
                                ),
                            ],
                        );
                        ui.end_row();
                    }
                });
        }

        if !kind.spec.params.is_empty() {
            section(ui, tr("editor.inspector.parameters"));
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
                            ui.label(kind.param_label(spec.name))
                                .on_hover_text(kind.param_help(spec.name));
                        });
                        let source = super::linked::track_of(node).to_owned();
                        let shown = if node.kind == super::linked::VIDEO_INPUT {
                            tr("editor.inspector.linked_video").to_owned()
                        } else {
                            tr_args("editor.inspector.linked_track", &[("track", &source)])
                        };
                        ui.horizontal(|ui| {
                            ui.add_space(GUTTER_WIDTH + ui.spacing().item_spacing.x);
                            ui.weak(shown)
                                .on_hover_text(tr("editor.inspector.linked_help"));
                        });
                        ui.add_space(PARAM_GAP);
                        continue;
                    }
                    let tracks = (node.kind == AUDIO_INPUT && spec.name == SOURCE_PARAM)
                        .then_some(ctx.tracks);
                    let (exposed, wire) = pins[index];
                    // A parameter the node's settings leave unused is hidden, unless a signal is
                    // wired to it (or its pin shown): that stays in view, greyed, with the reason.
                    let used =
                        spec.is_used(|name| choice_value(kind.spec.params, &node.params, name));
                    if !used && !exposed && wire.is_none() {
                        continue;
                    }
                    let mut modulation = wire.map(|color| {
                        let current = node.modulation.get(spec.name).copied();
                        let value = current.unwrap_or_else(|| spec.default_modulation());
                        (value, current, color)
                    });
                    let mut expose = spec.modulatable.then_some(exposed);
                    // Whole-only parameters are always whole, so they have no toggle.
                    let mut integer = (matches!(spec.kind, ParamKind::Number { .. })
                        && !spec.integer)
                        .then(|| node.integer.contains(spec.name));
                    let live = ctx
                        .params
                        .iter()
                        .find(|p| *p.node == node.id && p.index == index)
                        .map(|p| f64::from(p.value));
                    // Scoped to the node, so same-named parameters of different nodes (and their
                    // stored slider ranges and open menus) never share ids.
                    let node_id = node.id.clone();
                    let mut slider = node.ranges.get(spec.name).map(|r| (r[0], r[1]));
                    let disconnected = ui
                        .push_id(node_id, |ui| {
                            if !used {
                                ui.set_opacity(0.5);
                            }
                            let disconnected = param_row(ParamRow {
                                ui,
                                kind: &kind,
                                spec,
                                params: &mut node.params,
                                tracks,
                                track_width,
                                integer: integer.as_mut(),
                                expose: expose.as_mut(),
                                modulation: modulation.as_mut().map(|(m, _, c)| (m, *c)),
                                live,
                                slider: &mut slider,
                            });
                            if !used && let Some(when) = spec.when {
                                ui.horizontal(|ui| {
                                    ui.add_space(GUTTER_WIDTH + ui.spacing().item_spacing.x);
                                    ui.weak(unused_reason(&kind, &when));
                                });
                            }
                            disconnected
                        })
                        .inner;
                    ui.add_space(PARAM_GAP);
                    if disconnected {
                        disconnect = Some(index);
                    }
                    match slider {
                        Some((lo, hi)) => {
                            node.ranges.insert(spec.name.to_owned(), [lo, hi]);
                        }
                        None => {
                            node.ranges.remove(spec.name);
                        }
                    }
                    if let Some(on) = integer {
                        if on {
                            node.integer.insert(spec.name.to_owned());
                        } else {
                            node.integer.remove(spec.name);
                        }
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
            ui.weak(tr("editor.inspector.no_settings"));
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

/// A choice parameter's current value: what the node stores, else the default.
fn choice_value(
    specs: &[ParamSpec],
    params: &BTreeMap<String, ParamValue>,
    name: &str,
) -> Option<String> {
    match params.get(name) {
        Some(ParamValue::Text(s)) => Some(s.clone()),
        _ => match specs.iter().find(|p| p.name == name)?.default_value() {
            ParamValue::Text(s) => Some(s),
            _ => None,
        },
    }
}

/// Why a parameter shown despite its rule isn't being used, in the controlling parameter's words.
fn unused_reason(kind: &NodeType, when: &ShownWhen) -> String {
    let label = kind
        .spec
        .params
        .iter()
        .find(|p| p.name == when.param)
        .map_or(when.param, |p| kind.param_label(p.name));
    tr_args(
        "editor.inspector.unused_reason",
        &[
            ("param", label),
            (
                "values",
                &when
                    .values
                    .join(&format!(" {} ", tr("editor.inspector.or"))),
            ),
        ],
    )
}

/// What one parameter row shows and edits.
struct ParamRow<'a, 'u> {
    ui: &'u mut Ui,
    kind: &'a NodeType,
    spec: &'a ParamSpec,
    params: &'a mut BTreeMap<String, ParamValue>,
    /// For an audio input's track: the project's tracks.
    tracks: Option<&'a [String]>,
    track_width: f32,
    /// Whether the number is rounded to whole numbers.
    integer: Option<&'a mut bool>,
    /// Whether the parameter's pin shows on the node, for parameters that can be modulated.
    expose: Option<&'a mut bool>,
    /// The modulation of a connected parameter, and the wire's colour.
    modulation: Option<(&'a mut Modulation, egui::Color32)>,
    /// A modulated parameter's value at the playhead.
    live: Option<f64>,
    /// The slider range the user set, if any.
    slider: &'a mut Option<(f64, f64)>,
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

/// What each output carries, as the last compile worked it out: its size and tag.
fn signals(ui: &mut Ui, kind: &NodeType, compiled: &NodeStats) {
    if compiled.outputs.is_empty() {
        return;
    }
    ui.add_space(4.0);
    let output = |i: usize| kind.spec.outputs.get(i).map_or("?", |o| o.name);
    // Split's unused channels are silence; listing them adds nothing.
    let shown = match compiled.inputs.first() {
        Some(input) if kind.kind == rastersong_engine::SPLIT => {
            (input.samples_per_pixel as usize).clamp(1, compiled.outputs.len())
        }
        _ => compiled.outputs.len(),
    };
    for (i, layout) in compiled.outputs.iter().take(shown).enumerate() {
        let text = if compiled.outputs.len() > 1 {
            tr_args(
                "editor.inspector.output_line",
                &[
                    ("port", output(i)),
                    ("layout", &layout.to_string()),
                    ("tag", &layout.tag.to_string()),
                ],
            )
        } else {
            tr_args(
                "editor.inspector.output_single",
                &[
                    ("layout", &layout.to_string()),
                    ("tag", &layout.tag.to_string()),
                ],
            )
        };
        ui.label(RichText::new(text).small().weak())
            .on_hover_text(tr("editor.inspector.signal_help"));
    }
}

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
        kind,
        spec,
        params,
        tracks,
        track_width,
        integer,
        expose,
        mut modulation,
        live,
        slider,
    } = row;
    let mut disconnect = false;
    let integer_on = integer.as_ref().is_some_and(|i| **i);
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
                    tr("editor.param.pin_hide")
                } else {
                    tr("editor.param.pin_show")
                };
                if diamond_toggle(ui, *exposed, color)
                    .on_hover_text(hover)
                    .clicked()
                {
                    *exposed = !*exposed;
                }
            }
            None if spec.locked => {
                locked_diamond(ui).on_hover_text(tr_args(
                    "editor.param.locked",
                    &[("reason", kind.param_locked(spec).trim_end_matches('.'))],
                ));
            }
            None => {
                ui.allocate_space(egui::vec2(GUTTER_WIDTH, 14.0));
            }
        }
        ui.label(kind.param_label(spec.name))
            .on_hover_text(kind.param_help(spec.name));
        if !spec.unit.is_empty() {
            ui.weak(spec.unit);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let reset_button = ui
                .add_enabled(
                    value != default,
                    egui::Button::new("↺").small().frame(false),
                )
                .on_hover_text(tr("editor.param.reset"));
            reset = reset_button.clicked();
            if let Some(integer) = integer {
                ui.toggle_value(integer, tr("editor.param.int"))
                    .on_hover_text(tr("editor.param.int.help"));
            }
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
                whole: integer_on || spec.integer,
            };
            let rounded = range.whole;
            ui.horizontal(|ui| {
                let modulated = modulation.as_mut().map(|(m, color)| Modulated {
                    spec,
                    modulation: m,
                    color: *color,
                    live,
                });
                let response = param_field(
                    ui,
                    spec.name,
                    n,
                    range,
                    (track_width, VALUE_WIDTH),
                    modulated,
                    slider,
                );
                disconnect = response.disconnect;
                if rounded {
                    *n = n.round();
                }
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
                    tr_args("editor.param.no_such_track", &[("track", s)])
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
                            ui.weak(tr("editor.param.no_tracks"));
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

/// The mark in place of the expose toggle on a parameter that can't be modulated: the diamond
/// crossed out and dim, with the reason on hover.
fn locked_diamond(ui: &mut Ui) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(GUTTER_WIDTH, 14.0), egui::Sense::hover());
    let c = rect.center();
    let r = 4.5;
    let color = ui.visuals().weak_text_color().gamma_multiply(0.6);
    let stroke = egui::Stroke::new(1.0, color);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            c + egui::vec2(0.0, -r),
            c + egui::vec2(r, 0.0),
            c + egui::vec2(0.0, r),
            c + egui::vec2(-r, 0.0),
        ],
        egui::Color32::TRANSPARENT,
        stroke,
    ));
    ui.painter()
        .line_segment([c + egui::vec2(-r, r), c + egui::vec2(r, -r)], stroke);
    response
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

/// The meters `kind` declares, drawn from what the node `id` published for the last frame.
fn node_meters(ui: &mut Ui, kind: &NodeType, meters: &[NodeMeters], id: &str) {
    let spec = kind.spec.meters;
    if spec.is_empty() {
        return;
    }
    // The meters are always drawn: while a frame is rendering after an edit there are no
    // readings, and the last ones are shown so the inspector does not change height.
    let values = meters.iter().find(|m| &*m.node == id);
    ui.add_space(6.0);
    for (i, meter) in spec.iter().enumerate() {
        let label = crate::widgets::meter_label(meter.kind);
        let key = egui::Id::new(("meter", id, meter.id));
        let last = key.with("last");
        let value = match values.and_then(|v| v.values.get(i)) {
            Some(&value) => {
                ui.data_mut(|d| d.insert_temp(last, value));
                value
            }
            None => ui.data(|d| d.get_temp::<f32>(last)).unwrap_or(0.0),
        };
        match meter.kind {
            MeterKind::Level => crate::widgets::level_meter(ui, key, label, value),
            MeterKind::GainReduction => {
                crate::widgets::gain_reduction_meter(ui, key, label, value);
            }
        }
    }
}
