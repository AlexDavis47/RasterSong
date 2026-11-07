//! The inspector of the active node, in the inspector's one layout: the kind with the node's
//! category and Bypass, its name and what it does, then its meters, its shared settings and its
//! parameters. What its outputs carry is in the probe, on hovering a pin or wire.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, RichText, Ui};
use rastersong_engine::{
    AUDIO_BUS_OUTPUT, AUDIO_INPUT, BUS_PARAM, NAME_PARAM, NAMED_INPUT, NOTE, SOURCE_PARAM,
    TRACK_PARAM, VIDEO_INPUT,
};
use rastersong_engine::{
    Channels, GeneratorLayout, Grouping, Interpolation, MeterKind, Modulation, NodeMeters,
    NodeStats, NodeType, ParamKind, ParamLevel, ParamSpec, ParamValue, Severity, ShownWhen, Timing,
};
use rastersong_lang::{tr, tr_args};

use super::param_field::{
    Modulated, NumberRange, SMALL_BUTTON, param_field, reset_button, reset_gesture,
};
use super::unit_chip::unit_chip;
use super::{GraphEditor, NodeKey, param_port};
use crate::name_edit::start_rename;
use crate::theme::{SMALL_SIZE, Theme, medium};
use crate::widgets::{
    CHOICE_WIDTH, KeyButton, about, inspector, inspector_body, inspector_header, inspector_name,
    kind_row, section, setting_row, time_signature, toggle_box,
};

/// What the inspector needs from outside the editor.
#[derive(Debug, Default)]
pub struct InspectorContext<'a> {
    /// The project's output buses but the master, offered by Audio Bus Outputs: (name, label).
    pub buses: &'a [(String, String)],
    /// What a Video Input and an Audio Input can read: (`source` setting, label).
    pub video_sources: &'a [(String, String)],
    pub audio_sources: &'a [(String, String)],
    /// The Named Outputs a Named Input can read, as ([`named_choice`] of its track and name,
    /// label).
    pub named_outputs: &'a [(String, String)],
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
    /// The wire modulating it: its color, and where it comes from ("Split · low").
    wire: Option<(Color32, String)>,
}

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
        // What the last compile found: the node's notes and warnings.
        let compiled = self.compiled(key).cloned();
        let mut toggled: Option<(usize, bool)> = None;
        let mut disconnect: Option<usize> = None;
        let mut bypass = false;
        let annotation = kind.spec.category.is_annotation();
        // What a note points at, by title; it is changed on the canvas, with the note's tab.
        let attached = self
            .attached_to(key)
            .and_then(|t| self.node(t))
            .map(|n| self.node_title(n));
        let mut detach = false;
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
                    // Annotations render nothing, so there is nothing to bypass.
                    if annotation {
                        return;
                    }
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
            // Name: shown on the node instead of the type, renamed in place like every name.
            // Unnamed, the node is titled with its type; committing the type's name unnames it,
            // as Restore default name does. The type stays under it, whatever the name.
            let current = node.label.as_deref().unwrap_or(kind.label()).to_owned();
            let edit = inspector_name(ui, node_name_id(), &current);
            edit.response
                .on_hover_text(tr("editor.inspector.name_help"))
                .context_menu(|ui| {
                    let rename = egui::Button::new(tr("resources.rename")).shortcut_text("F2");
                    if ui.add(rename).clicked() {
                        start_rename(ui.ctx(), node_name_id());
                        ui.close();
                    }
                    let restore = egui::Button::new(tr("editor.inspector.restore_name"));
                    if ui.add_enabled(node.label.is_some(), restore).clicked() {
                        node.label = None;
                        ui.close();
                    }
                });
            if let Some(name) = edit.committed {
                node.label = (name != kind.label()).then_some(name);
            }
            ui.label(
                RichText::new(kind.label())
                    .size(SMALL_SIZE)
                    .color(theme.surfaces.text_faint),
            )
            .on_hover_text(tr("editor.inspector.type_help"));
            about(ui, kind.description());
            if let Some(compiled) = &compiled {
                diagnostics(ui, compiled, theme);
            }
        });
        inspector_body(ui, |ui| {
            let title = if kind.kind == NOTE {
                tr("editor.inspector.note_text")
            } else {
                tr("editor.inspector.description")
            };
            section(ui, title, |ui| {
                description_field(ui, &mut node.description, annotation);
                if kind.kind == NOTE {
                    detach = attach_row(ui, attached.as_deref());
                }
            });
            if !kind.spec.meters.is_empty() {
                section(ui, tr("inspector.meters"), |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    node_meters(ui, &kind, ctx.meters, &node.id);
                });
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
                        let text = |name: &str| match node.params.get(name) {
                            Some(ParamValue::Text(text)) => text.clone(),
                            _ => String::new(),
                        };
                        // A Named Input's track and name are picked together, on the name's row.
                        let named = node.kind == NAMED_INPUT;
                        if named && spec.name == TRACK_PARAM {
                            continue;
                        }
                        let tracks = match (node.kind.as_str(), spec.name) {
                            (AUDIO_BUS_OUTPUT, BUS_PARAM) => Some(Names {
                                choices: ctx.buses,
                                missing: "editor.param.no_such_bus",
                                empty: "editor.param.no_buses",
                            }),
                            (VIDEO_INPUT, SOURCE_PARAM) => Some(Names {
                                choices: ctx.video_sources,
                                missing: "editor.param.no_such_source",
                                empty: "editor.param.no_sources",
                            }),
                            (AUDIO_INPUT, SOURCE_PARAM) => Some(Names {
                                choices: ctx.audio_sources,
                                missing: "editor.param.no_such_source",
                                empty: "editor.param.no_sources",
                            }),
                            (NAMED_INPUT, NAME_PARAM) => Some(Names {
                                choices: ctx.named_outputs,
                                missing: "editor.param.no_such_output",
                                empty: "editor.param.no_named_outputs",
                            }),
                            _ => None,
                        };
                        // A numerator and the denominator after it are one time signature
                        // field, unless a signal is wired to either: then each shows its
                        // modulation as usual.
                        let combined = |first: usize| {
                            signature_at(kind.spec.params, first)
                                && pins[first].wire.is_none()
                                && pins[first + 1].wire.is_none()
                        };
                        if index > 0 && combined(index - 1) {
                            continue;
                        }
                        if combined(index) {
                            let used = spec
                                .is_used(|name| choice_value(kind.spec.params, &node.params, name));
                            let mut exposes = [&pins[index], &pins[index + 1]].map(|p| p.exposed);
                            if !used && !exposes.contains(&true) {
                                continue;
                            }
                            let specs = [spec, &kind.spec.params[index + 1]];
                            ui.push_id(node.id.clone(), |ui| {
                                if !used {
                                    ui.set_opacity(0.5);
                                }
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    signature_row(ui, &kind, specs, &mut node.params, &mut exposes);
                                    if !used && let Some(when) = spec.when {
                                        ui.weak(unused_reason(&kind, &when));
                                    }
                                });
                            });
                            for (offset, exposed) in exposes.into_iter().enumerate() {
                                if exposed != pins[index + offset].exposed {
                                    toggled = Some((index + offset, exposed));
                                }
                            }
                            continue;
                        }
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
                        if named {
                            let joined = named_choice(&text(TRACK_PARAM), &text(NAME_PARAM));
                            node.params
                                .insert(NAME_PARAM.to_owned(), ParamValue::Text(joined));
                        }
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
                        if named {
                            let joined = match node.params.get(NAME_PARAM) {
                                Some(ParamValue::Text(text)) => text.clone(),
                                _ => String::new(),
                            };
                            let (track, name) = joined.split_once('\u{0}').unwrap_or(("", ""));
                            for (param, value) in [(TRACK_PARAM, track), (NAME_PARAM, name)] {
                                node.params
                                    .insert(param.to_owned(), ParamValue::Text(value.to_owned()));
                            }
                        }
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
            } else if !shared_settings && !annotation {
                ui.weak(tr("editor.inspector.no_settings"));
            }
        });
        if detach {
            self.attach(key, None);
        }
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
/// The node's description, typed into a field several lines high. Emptied, the node has none.
fn description_field(ui: &mut Ui, description: &mut Option<String>, annotation: bool) {
    let mut text = description.clone().unwrap_or_default();
    let hint = if annotation {
        tr("editor.inspector.annotation_hint")
    } else {
        tr("editor.inspector.description_hint")
    };
    let response = ui.add(
        egui::TextEdit::multiline(&mut text)
            .id(egui::Id::new("node-description"))
            .hint_text(hint)
            .desired_rows(if annotation { 5 } else { 3 })
            .desired_width(f32::INFINITY),
    );
    if response.changed() {
        *description = (!text.trim().is_empty()).then_some(text);
    }
}

/// What a note points at (the title of `attached`), with a Detach button; or, loose, how to
/// attach it: by dragging its tab on the canvas. Returns whether Detach was clicked.
fn attach_row(ui: &mut Ui, attached: Option<&str>) -> bool {
    let theme = Theme::of(ui.ctx());
    let Some(title) = attached else {
        ui.label(
            RichText::new(tr("editor.inspector.attach_hint"))
                .size(SMALL_SIZE)
                .color(theme.surfaces.text_faint),
        );
        return false;
    };
    let mut detach = false;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(tr_args("editor.inspector.attached", &[("node", title)]))
                .font(medium(13.0)),
        )
        .on_hover_text(tr("editor.inspector.attached_help"));
        detach = ui.small_button(tr("editor.inspector.detach")).clicked();
    });
    detach
}

/// The id of the active node's name in the inspector: [`start_rename`] with it renames the node.
pub(super) fn node_name_id() -> egui::Id {
    egui::Id::new("node-name")
}

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

/// Values a text parameter picks from, each with its label, and the lang keys for a value that
/// isn't among them and for none at all.
#[derive(Debug, Clone, Copy)]
struct Names<'a> {
    choices: &'a [(String, String)],
    missing: &'static str,
    empty: &'static str,
}

/// The value a Named Input's drop-down picks for Named Output `name` of graph track `track`:
/// both settings in one, split again once picked. Empty for nothing picked.
pub fn named_choice(track: &str, name: &str) -> String {
    if track.is_empty() && name.is_empty() {
        String::new()
    } else {
        format!("{track}\u{0}{name}")
    }
}

/// What one parameter row shows and edits.
struct ParamRow<'a, 'u> {
    ui: &'u mut Ui,
    kind: &'a NodeType,
    spec: &'a ParamSpec,
    params: &'a mut BTreeMap<String, ParamValue>,
    /// For a bus output's bus, an input's source or a Named Input's output: the values to
    /// pick from.
    tracks: Option<Names<'a>>,
    /// Whether the number is rounded to whole numbers.
    integer: Option<&'a mut bool>,
    /// Whether the parameter's pin shows on the node, for parameters that can be modulated.
    expose: Option<&'a mut bool>,
    /// The modulation of a connected parameter, the wire's color and where it comes from.
    modulation: Option<(&'a mut Modulation, Color32, &'a str)>,
    /// A modulated parameter's value at the playhead.
    live: Option<f64>,
    /// The slider range the user set, if any.
    slider: &'a mut Option<(f64, f64)>,
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
            let timed = spec.timing != Timing::None;
            let mut measure = spec
                .measure_of(params)
                .map(|(unit, reading)| (unit.to_owned(), reading));
            let count = *n;
            let chip_id = ui.id().with((spec.name, "unit"));
            // A timed value's rounding is its unit menu's Free/Sync switch.
            let (sync, integer) = if timed {
                (integer, None)
            } else {
                (None, integer)
            };
            let suffix = |ui: &mut Ui| {
                if let Some((unit, reading)) = measure.as_mut() {
                    let rate = (spec.timing == Timing::Rate).then_some(reading);
                    unit_chip(ui, chip_id, unit, count, rate, sync);
                }
            };
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
                ui, spec.name, label, n, range, modulated, slider, leading, suffix, trailing,
            );
            disconnect = response.disconnect;
            if let Some((unit, reading)) = measure {
                spec.set_measure(params, &unit, reading);
            }
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

/// Whether parameters `first` and `first + 1` are a time signature's numerator and denominator.
fn signature_at(specs: &[ParamSpec], first: usize) -> bool {
    specs.get(first).is_some_and(|s| s.name == "numerator")
        && specs
            .get(first + 1)
            .is_some_and(|s| s.name == "denominator")
}

/// A numerator and denominator as one time signature row: the two pin toggles (for parameters a
/// signal can modulate) and the name, then the field and its reset button. Values equal to the
/// default are not stored, as in [`param_row`].
fn signature_row(
    ui: &mut Ui,
    kind: &NodeType,
    specs: [&ParamSpec; 2],
    params: &mut BTreeMap<String, ParamValue>,
    exposes: &mut [bool; 2],
) {
    let number = |spec: &ParamSpec, value: Option<&ParamValue>| match value {
        Some(ParamValue::Number(n)) => *n,
        _ => match spec.default_value() {
            ParamValue::Number(n) => n,
            _ => 1.0,
        },
    };
    let defaults = specs.map(|spec| number(spec, None));
    let mut values = specs.map(|spec| number(spec, params.get(spec.name)));
    let max = specs
        .iter()
        .map(|spec| match spec.kind {
            ParamKind::Number { limit_max, .. } => limit_max,
            _ => 64.0,
        })
        .fold(1.0, f64::max);
    let label = tr("param.time_signature.label");
    let mut reset = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let theme = Theme::of(ui.ctx());
        for (spec, exposed) in specs.iter().zip(exposes.iter_mut()) {
            if !spec.modulatable {
                continue;
            }
            let hover = tr_args(
                if *exposed {
                    "editor.param.pin_hide_named"
                } else {
                    "editor.param.pin_show_named"
                },
                &[("name", kind.param_label(spec.name))],
            );
            if toggle_box(
                ui,
                *exposed,
                theme.accent,
                &hover,
                |painter, rect, color, on| {
                    diamond(painter, rect, color, on);
                },
            )
            .on_hover_text(&hover)
            .clicked()
            {
                *exposed = !*exposed;
            }
        }
        if specs.iter().all(|s| s.locked) {
            locked_diamond(ui).on_hover_text(tr_args(
                "editor.param.locked",
                &[("reason", kind.param_locked(specs[0]).trim_end_matches('.'))],
            ));
        }
        ui.label(RichText::new(label).font(crate::theme::medium(13.0)))
            .on_hover_text(tr("param.time_signature.help"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let shown = format!("{}/{}", defaults[0], defaults[1]);
            reset = reset_button(ui, label, &shown, values != defaults);
            let [numerator, denominator] = &mut values;
            time_signature(ui, numerator, denominator, max)
                .on_hover_text(tr("param.time_signature.help"));
        });
    });
    if reset {
        values = defaults;
    }
    for ((spec, value), default) in specs.iter().zip(values).zip(defaults) {
        let value = value.round();
        if value == default {
            params.remove(spec.name);
        } else {
            params.insert(spec.name.to_owned(), ParamValue::Number(value));
        }
    }
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
        // Audio Bus Outputs pick one of the project's buses, inputs what they read.
        (ParamKind::Text { .. }, ParamValue::Text(s)) if tracks.is_some() => {
            let names = tracks.unwrap();
            let label = names
                .choices
                .iter()
                .find(|(value, _)| value == s)
                .map(|(_, label)| label.clone());
            let missing = label.is_none();
            let shown = label.unwrap_or_else(|| {
                let shown = match s.split_once('\u{0}') {
                    Some((track, name)) => {
                        tr_args("source.graph_output", &[("track", track), ("output", name)])
                    }
                    None => s
                        .split_once(':')
                        .map_or(s.as_str(), |(_, name)| name)
                        .to_owned(),
                };
                tr_args(names.missing, &[("track", &shown)])
            });
            let combo = egui::ComboBox::from_id_salt(spec.name)
                .selected_text(RichText::new(shown).color(if missing {
                    Theme::of(ui.ctx()).error
                } else {
                    ui.visuals().text_color()
                }))
                .width(width)
                .show_ui(ui, |ui| {
                    if names.choices.is_empty() {
                        ui.weak(tr(names.empty));
                    }
                    for (value, label) in names.choices {
                        ui.selectable_value(s, value.clone(), label);
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
