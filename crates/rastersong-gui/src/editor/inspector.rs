//! The inspector of the active node, in the inspector's one layout: the kind with the node's
//! category and Bypass, its name and what it is, then what its outputs carry, its shared settings
//! and its parameters.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, RichText, Ui};
use rastersong_engine::{AUDIO_OUTPUT, BUS_PARAM};
use rastersong_engine::{
    Channels, GeneratorLayout, Grouping, Interpolation, MeterKind, Modulation, NodeMeters,
    NodeStats, NodeType, ParamKind, ParamLevel, ParamSpec, ParamValue, Severity, ShownWhen,
};
use rastersong_lang::{tr, tr_args};

use super::param_field::{
    Modulated, NumberRange, SMALL_BUTTON, param_field, reset_button, reset_gesture,
};
use super::{GraphEditor, NodeKey, param_port};
use crate::name_edit::name_edit;
use crate::theme::Theme;
use crate::widgets::{
    CHOICE_WIDTH, KeyButton, about, inspector, inspector_body, inspector_header, kind_row,
    name_style, section, setting_row, swatch, toggle_box,
};

/// What the inspector needs from outside the editor.
#[derive(Debug, Default)]
pub struct InspectorContext<'a> {
    /// Names of the project's output buses, offered by audio outputs.
    pub buses: &'a [String],
    /// Modulated parameters' values at the playhead, for their ghost handles.
    pub params: &'a [ParamLevel],
    /// What the nodes' meters read at the playhead.
    pub meters: &'a [NodeMeters],
}

/// What the inspector knows of a parameter's pin before borrowing the node to edit it.
#[derive(Debug, Clone)]
struct Pin {
    /// Whether the pin shows on the node.
    exposed: bool,
    /// The wire modulating it: its colour, and where it comes from ("Split · low").
    wire: Option<(Color32, String)>,
}

/// Width of the column naming each output.
const PORT_WIDTH: f32 = 34.0;

impl GraphEditor {
    pub fn show_inspector(&mut self, ui: &mut Ui, ctx: &InspectorContext) {
        let Some(key) = self.active.filter(|&k| self.node(k).is_some()) else {
            inspector(
                ui,
                |ui| {
                    kind_row(ui, tr("inspector.kind.node"), None, |_| {});
                    about(ui, tr("editor.inspector.empty_select"));
                    about(ui, tr("editor.inspector.empty_add"));
                },
                |_| {},
            );
            return;
        };
        let kind = self
            .node(key)
            .and_then(|n| self.registry.get(&n.kind))
            .cloned();
        let Some(kind) = kind else {
            let theme = Theme::of(ui.ctx());
            let missing = self.node(key).map(|n| n.kind.clone()).unwrap_or_default();
            inspector(
                ui,
                |ui| {
                    kind_row(ui, tr("inspector.kind.node"), None, |_| {});
                    ui.colored_label(
                        theme.error,
                        tr_args("editor.inspector.unknown_kind", &[("kind", &missing)]),
                    );
                },
                |_| {},
            );
            return;
        };
        let theme = Theme::of(ui.ctx());
        let pins = self.pins(key, &kind, theme);
        let outputs: Vec<Color32> = (0..kind.spec.outputs.len())
            .map(|i| self.output_color(key, i, theme).solid())
            .collect();
        // What the last compile found: the node's signals and warnings.
        let compiled = self.compiled(key).cloned();
        let mut toggled: Option<(usize, bool)> = None;
        let mut disconnect: Option<usize> = None;
        let mut bypass = false;
        let node = self.nodes.iter_mut().find(|n| n.key == key).unwrap();

        inspector_header(ui, |ui| {
            let category = kind.spec.category;
            let action = if node.bypass {
                tr("inspector.restore")
            } else {
                tr("inspector.bypass")
            };
            kind_row(
                ui,
                tr("inspector.kind.node"),
                Some((category.label(), theme.category(Some(category)))),
                |ui| {
                    let button = KeyButton::new(action, tr("editor.shortcut.alt_click"))
                        .height(SMALL_BUTTON)
                        .ghost();
                    if ui
                        .add(button)
                        .on_hover_text(tr("inspector.bypass.help"))
                        .clicked()
                    {
                        bypass = true;
                    }
                },
            );
            // Name: shown on the node instead of the type. Unnamed, the node is titled with its
            // type, so the field shows that; committing the type's name (or nothing) unnames it.
            let current = node.label.as_deref().unwrap_or(kind.label()).to_owned();
            let edit = name_edit(ui, egui::Id::new("node-name"), &current, name_style);
            edit.response
                .on_hover_text(tr("editor.inspector.name_help"));
            if let Some(name) = edit.committed {
                node.label = (name != kind.label()).then_some(name);
            }
            about(
                ui,
                &tr_args(
                    "inspector.node.about",
                    &[
                        ("kind", kind.label()),
                        ("id", &node.id),
                        ("description", kind.description()),
                    ],
                ),
            );
            if let Some(compiled) = &compiled {
                diagnostics(ui, compiled, theme);
            }
        });
        inspector_body(ui, |ui| {
            if let Some(compiled) = &compiled {
                signals(ui, &kind, compiled, &outputs, ctx.meters, &node.id);
            }
            let shared_settings =
                kind.spec.inputs.len() > 1 || kind.spec.per_channel || kind.spec.takes_layout;
            if shared_settings {
                section(ui, tr("editor.inspector.node_settings"), |ui| {
                    node_settings(ui, &kind, node);
                });
            }
            if !kind.spec.params.is_empty() {
                section(ui, tr("editor.inspector.parameters"), |ui| {
                    ui.spacing_mut().item_spacing.y = 12.0;
                    for (index, spec) in kind.spec.params.iter().enumerate() {
                        let tracks = (node.kind == AUDIO_OUTPUT && spec.name == BUS_PARAM)
                            .then_some(Names {
                                names: ctx.buses,
                                missing: "editor.param.no_such_bus",
                                empty: "editor.param.no_buses",
                            });
                        let Pin { exposed, wire } = pins[index].clone();
                        // A parameter the node's settings leave unused is hidden, unless a
                        // signal is wired to it (or its pin shown): that stays in view,
                        // greyed, with the reason.
                        let used =
                            spec.is_used(|name| choice_value(kind.spec.params, &node.params, name));
                        if !used && !exposed && wire.is_none() {
                            continue;
                        }
                        let mut modulation = wire.map(|(color, source)| {
                            let current = node.modulation.get(spec.name).copied();
                            let value = current.unwrap_or_else(|| spec.default_modulation());
                            (value, current, color, source)
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
                        // Scoped to the node, so same-named parameters of different nodes
                        // (and their stored slider ranges and open menus) never share ids.
                        let node_id = node.id.clone();
                        let mut slider = node.ranges.get(spec.name).map(|r| (r[0], r[1]));
                        let disconnected = ui
                            .push_id(node_id, |ui| {
                                if !used {
                                    ui.set_opacity(0.5);
                                }
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    let disconnected = param_row(ParamRow {
                                        ui,
                                        kind: &kind,
                                        spec,
                                        params: &mut node.params,
                                        tracks,
                                        integer: integer.as_mut(),
                                        expose: expose.as_mut(),
                                        modulation: modulation
                                            .as_mut()
                                            .map(|(m, _, c, s)| (m, *c, s.as_str())),
                                        live,
                                        slider: &mut slider,
                                    });
                                    if !used && let Some(when) = spec.when {
                                        ui.weak(unused_reason(&kind, &when));
                                    }
                                    disconnected
                                })
                                .inner
                            })
                            .inner;
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
                        if let Some((value, current, _, _)) = modulation
                            && Some(value) != current
                        {
                            node.modulation.insert(spec.name.to_owned(), value);
                        }
                    }
                });
            } else if !shared_settings {
                ui.weak(tr("editor.inspector.no_settings"));
            }
        });
        if bypass {
            self.toggle_bypass(&[key].into());
        }
        if let Some((index, exposed)) = toggled {
            self.set_param_exposed(key, index, exposed);
        }
        if let Some(index) = disconnect {
            self.disconnect_input((key, param_port(index)));
        }
    }

    /// Per parameter of node `key`: whether its pin shows, and the wire modulating it, if any.
    fn pins(&self, key: NodeKey, kind: &NodeType, theme: &Theme) -> Vec<Pin> {
        let Some(node) = self.node(key) else {
            return Vec::new();
        };
        (0..kind.spec.params.len())
            .map(|i| {
                let wire = self.wires.iter().find(|w| w.to == (key, param_port(i)));
                let wire = wire.map(|w| {
                    let (from, port) = w.from;
                    let color = self.output_color(from, port, theme).solid();
                    let name = self.node(from).map(|n| self.node_title(n));
                    let port = self
                        .node(from)
                        .and_then(|n| self.registry.get(&n.kind))
                        .and_then(|k| k.spec.outputs.get(port))
                        .map_or("", |o| o.name);
                    let source = tr_args(
                        "inspector.source",
                        &[("node", &name.unwrap_or_default()), ("port", port)],
                    );
                    (color, source)
                });
                Pin {
                    exposed: self.param_exposed(node, i),
                    wire,
                }
            })
            .collect()
    }
}

/// The notes and warnings the last compile left on the node.
fn diagnostics(ui: &mut Ui, compiled: &NodeStats, theme: &Theme) {
    for diagnostic in &compiled.diagnostics {
        match diagnostic.severity {
            Severity::Note => ui.label(
                RichText::new(tr_args(
                    "editor.inspector.note_line",
                    &[("message", &diagnostic.message)],
                ))
                .size(12.0)
                .color(theme.text_dim),
            ),
            Severity::Warning => ui.label(
                RichText::new(tr_args(
                    "editor.inspector.warning_line",
                    &[("message", &diagnostic.message)],
                ))
                .size(12.0)
                .color(theme.warning),
            ),
        };
    }
}

/// The node's shared settings, a row each: Resampling and Grouping for nodes that mix inputs,
/// Layout for generators, Channels for effects that can work channel by channel.
fn node_settings(ui: &mut Ui, kind: &NodeType, node: &mut super::EditorNode) {
    if kind.spec.inputs.len() > 1 {
        choice(
            ui,
            (
                "resampling",
                "editor.setting.resampling",
                "editor.setting.resampling.help",
            ),
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
        choice(
            ui,
            (
                "grouping",
                "editor.setting.grouping",
                "editor.setting.grouping.help",
            ),
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
    }
    if kind.spec.takes_layout {
        choice(
            ui,
            (
                "layout",
                "editor.setting.layout",
                "editor.setting.layout.help",
            ),
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
    }
    if kind.spec.per_channel {
        choice(
            ui,
            (
                "channels",
                "editor.setting.channels",
                "editor.setting.channels.help",
            ),
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

/// Names a text parameter picks from, and the lang keys for a name that isn't among them and
/// for none at all.
#[derive(Debug, Clone, Copy)]
struct Names<'a> {
    names: &'a [String],
    missing: &'static str,
    empty: &'static str,
}

/// What one parameter row shows and edits.
struct ParamRow<'a, 'u> {
    ui: &'u mut Ui,
    kind: &'a NodeType,
    spec: &'a ParamSpec,
    params: &'a mut BTreeMap<String, ParamValue>,
    /// For an audio output's bus: the names to pick from.
    tracks: Option<Names<'a>>,
    /// Whether the number is rounded to whole numbers.
    integer: Option<&'a mut bool>,
    /// Whether the parameter's pin shows on the node, for parameters that can be modulated.
    expose: Option<&'a mut bool>,
    /// The modulation of a connected parameter, the wire's colour and where it comes from.
    modulation: Option<(&'a mut Modulation, Color32, &'a str)>,
    /// A modulated parameter's value at the playhead.
    live: Option<f64>,
    /// The slider range the user set, if any.
    slider: &'a mut Option<(f64, f64)>,
}

/// What each output carries, as the last compile worked it out: its colour, tag and size; then
/// the node's meters.
fn signals(
    ui: &mut Ui,
    kind: &NodeType,
    compiled: &NodeStats,
    colors: &[Color32],
    meters: &[NodeMeters],
    id: &str,
) {
    if compiled.outputs.is_empty() && kind.spec.meters.is_empty() {
        return;
    }
    let theme = Theme::of(ui.ctx());
    let title = if compiled.outputs.len() > 1 {
        tr("inspector.outputs")
    } else {
        tr("inspector.output")
    };
    section(ui, title, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let output = |i: usize| kind.spec.outputs.get(i).map_or("?", |o| o.name);
        // Split's unused channels are silence; listing them adds nothing.
        let shown = match compiled.inputs.first() {
            Some(input) if kind.kind == rastersong_engine::SPLIT => {
                (input.samples_per_pixel as usize).clamp(1, compiled.outputs.len())
            }
            _ => compiled.outputs.len(),
        };
        for (i, layout) in compiled.outputs.iter().take(shown).enumerate() {
            // A line of text, not a row of controls: only as tall as its text.
            let size = egui::vec2(ui.available_width(), 16.0);
            let row = egui::Layout::left_to_right(egui::Align::Center);
            ui.allocate_ui_with_layout(size, row, |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(PORT_WIDTH, 16.0), egui::Sense::hover());
                ui.painter().text(
                    rect.left_center(),
                    egui::Align2::LEFT_CENTER,
                    output(i),
                    egui::FontId::monospace(12.0),
                    theme.text_dim,
                );
                swatch(ui, 18.0, colors.get(i).copied().unwrap_or(theme.text_dim));
                // The layout on the right first, so the signal's name is cut short before it.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(layout.to_string())
                            .monospace()
                            .size(12.0)
                            .color(theme.surfaces.text_faint),
                    );
                    let rest = egui::vec2(ui.available_width(), 16.0);
                    ui.allocate_ui_with_layout(rest, row, |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(layout.tag.to_string()).size(12.0))
                                .truncate(),
                        )
                        .on_hover_text(tr("editor.inspector.signal_help"));
                    });
                });
            });
        }
        node_meters(ui, kind, meters, id);
    });
}

/// A setting's row with a drop-down of `options` (value, label, help). Alt+click or right-click
/// resets it to `default`. `id` is the drop-down's id; `name` and `help` are the lang keys of the
/// setting's name and help.
fn choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    (id, name, help): (&str, &str, &str),
    value: &mut T,
    default: T,
    options: &[(T, &str, &str)],
) {
    setting_row(ui, tr(name), tr(help), |ui| {
        let current = options.iter().find(|o| o.0 == *value).map_or("", |o| o.1);
        let combo = egui::ComboBox::from_id_salt(id)
            .selected_text(current)
            .width(CHOICE_WIDTH)
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
    });
}

/// One parameter: its pin toggle and name over its control. A number's row carries the int
/// toggle, the value box and the reset button, and a modulated number is set in a card with the
/// modulation's controls (see [`param_field`]). Values equal to the default are not stored, which
/// keeps graph files short. Returns true if the user asked to disconnect the modulating signal.
fn param_row(row: ParamRow) -> bool {
    let ParamRow {
        ui,
        kind,
        spec,
        params,
        tracks,
        integer,
        expose,
        modulation,
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
    let label = kind.param_label(spec.name);
    let pin_color = modulation.as_ref().map(|m| m.1);
    // The pin toggle (or the reason there is none) and the name, first on the row.
    let leading = |ui: &mut Ui| {
        let theme = Theme::of(ui.ctx());
        match expose {
            Some(exposed) => {
                let hover = if *exposed {
                    tr("editor.param.pin_hide")
                } else {
                    tr("editor.param.pin_show")
                };
                let color = pin_color.unwrap_or(theme.accent);
                if toggle_box(ui, *exposed, color, hover, |painter, rect, color, on| {
                    diamond(painter, rect, color, on);
                })
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
            None => {}
        }
        ui.label(RichText::new(label).font(crate::theme::medium(13.0)))
            .on_hover_text(kind.param_help(spec.name));
        if !spec.unit.is_empty() {
            ui.label(RichText::new(spec.unit).size(12.0).color(theme.text_dim));
        }
    };
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
            let modulated = modulation.map(|(m, color, source)| Modulated {
                spec,
                modulation: m,
                color,
                live,
                source,
            });
            let trailing = |ui: &mut Ui| {
                if let Some(integer) = integer {
                    let theme = Theme::of(ui.ctx());
                    let text = tr("editor.param.int");
                    if toggle_box(
                        ui,
                        *integer,
                        theme.accent,
                        text,
                        |painter, rect, color, _| {
                            painter.text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                text,
                                egui::FontId::monospace(11.0),
                                color,
                            );
                        },
                    )
                    .on_hover_text(tr("editor.param.int.help"))
                    .clicked()
                    {
                        *integer = !*integer;
                    }
                }
            };
            let response = param_field(
                ui, spec.name, label, n, range, modulated, slider, leading, trailing,
            );
            disconnect = response.disconnect;
            if rounded {
                *n = n.round();
            }
        }
        (kind_of, _) => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                leading(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let shown = match &default {
                        ParamValue::Text(s) => s.clone(),
                        other => format!("{other:?}"),
                    };
                    reset = reset_button(ui, label, &shown, value_differs);
                });
            });
            reset |= other_control(ui, kind_of, spec, &mut value, tracks, value_differs);
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

/// The control of a parameter that isn't a number: a drop-down of its choices, of the project's
/// buses, or a text field. Returns true if the user reset it.
fn other_control(
    ui: &mut Ui,
    kind: ParamKind,
    spec: &ParamSpec,
    value: &mut ParamValue,
    tracks: Option<Names>,
    differs: bool,
) -> bool {
    let width = ui.available_width();
    match (kind, value) {
        (ParamKind::Choice { options, .. }, ParamValue::Text(s)) => {
            let combo = egui::ComboBox::from_id_salt(spec.name)
                .selected_text(s.as_str())
                .width(width)
                .show_ui(ui, |ui| {
                    for &option in options {
                        ui.selectable_value(s, option.to_owned(), option);
                    }
                })
                .response;
            let reset = reset_gesture(ui, &combo, differs);
            if reset {
                egui::Popup::close_all(ui.ctx());
            }
            reset
        }
        // Audio outputs pick one of the project's buses.
        (ParamKind::Text { .. }, ParamValue::Text(s)) if tracks.is_some() => {
            let names = tracks.unwrap();
            let missing = !names.names.contains(s);
            let shown = if missing {
                tr_args(names.missing, &[("track", s)])
            } else {
                s.clone()
            };
            let combo = egui::ComboBox::from_id_salt(spec.name)
                .selected_text(RichText::new(shown).color(if missing {
                    Theme::of(ui.ctx()).error
                } else {
                    ui.visuals().text_color()
                }))
                .width(width)
                .show_ui(ui, |ui| {
                    if names.names.is_empty() {
                        ui.weak(tr(names.empty));
                    }
                    for name in names.names {
                        ui.selectable_value(s, name.clone(), name);
                    }
                })
                .response;
            let reset = reset_gesture(ui, &combo, differs);
            if reset {
                egui::Popup::close_all(ui.ctx());
            }
            reset
        }
        (ParamKind::Text { .. }, ParamValue::Text(s)) => {
            // Named, so it keeps focus while typing changes the lines above it (a renamed
            // port's signals), which would shift an automatic id.
            let field = ui.add(
                egui::TextEdit::singleline(s)
                    .id_salt(spec.name)
                    .desired_width(width),
            );
            reset_gesture(ui, &field, differs)
        }
        // A value of the wrong type (from a hand-edited file): the reset button above fixes it.
        (_, value) => {
            ui.colored_label(Theme::of(ui.ctx()).error, format!("{value:?}"));
            false
        }
    }
}

/// A diamond like the parameter pins in `rect`, filled when `on`.
fn diamond(painter: &egui::Painter, rect: egui::Rect, color: Color32, on: bool) {
    let c = rect.center();
    let r = 4.5;
    let points = vec![
        c + egui::vec2(0.0, -r),
        c + egui::vec2(r, 0.0),
        c + egui::vec2(0.0, r),
        c + egui::vec2(-r, 0.0),
    ];
    let fill = if on { color } else { Color32::TRANSPARENT };
    painter.add(egui::Shape::convex_polygon(
        points,
        fill,
        egui::Stroke::new(1.2, color),
    ));
}

/// The mark in place of the pin toggle on a parameter that can't be modulated: the diamond
/// crossed out and dim, with the reason on hover.
fn locked_diamond(ui: &mut Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
    let color = ui.visuals().weak_text_color().gamma_multiply(0.6);
    diamond(ui.painter(), rect, color, false);
    let c = rect.center();
    ui.painter().line_segment(
        [c + egui::vec2(-4.5, 4.5), c + egui::vec2(4.5, -4.5)],
        egui::Stroke::new(1.0, color),
    );
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
