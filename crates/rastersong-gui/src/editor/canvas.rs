//! Drawing and interaction for the node canvas.
//!
//! - Scroll wheel zooms around the pointer; middle- or right-drag pans.
//! - Left-drag on empty space box-selects (Shift adds); click a node to select it, Shift-click to
//!   add or remove it; drag nodes to move them.
//! - Drag from an output to an input (or the reverse) to connect; drag a connected input to
//!   pick its wire up again. Dropping a wire on empty space opens the node search, connected.
//! - Right-click empty space to add a node there; right-click a node for its menu.
//! - Delete removes the selection, Ctrl+D duplicates it, F frames the whole graph.

use std::collections::{BTreeSet, HashMap};

use eframe::egui::epaint::CubicBezierShape;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Key, PointerButton, Pos2, Rect, Response, Sense,
    Stroke, StrokeKind, Ui, pos2, vec2,
};
use rastersong_engine::{
    Category, Failure, Kind, NodeCost, NodeStats, OutputLevel, ParamLevel, SPLIT, Severity, Tag,
    UNBOUNDED_WARMUP,
};

use super::search::{NodeMenu, SearchMenu};
use super::{GraphEditor, NodeKey};
use rastersong_lang::{tr, tr_args};

use crate::effects::Glow;
use crate::theme::{Theme, WireStyle};

const HEADER: f32 = 24.0;
const ROW: f32 = 20.0;
const PIN_RADIUS: f32 = 4.5;
/// How close (in screen pixels) the pointer must be to grab a pin.
const PIN_GRAB: f32 = 10.0;
const MIN_WIDTH: f32 = 110.0;
const PAD: f32 = 10.0;
const FONT: f32 = 13.0;
const LABEL_FONT: f32 = 11.5;
const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 3.0;
/// Pointer movement (screen pixels) that turns a right-click into a pan.
const DRAG_THRESHOLD: f32 = 4.0;

/// What the canvas needs from outside the editor each frame.
#[derive(Debug, Default)]
pub struct CanvasContext<'a> {
    /// Output levels at the playhead, for wire widths.
    pub levels: &'a [OutputLevel],
    /// Values of modulated parameters at the playhead, for pin tooltips.
    pub params: &'a [ParamLevel],
    /// Why the graph can't render, shown along the bottom of the canvas.
    pub failure: Option<&'a Failure>,
    pub wire_style: WireStyle,
    /// Whether to draw each node's latency and warmup under it.
    pub show_stats: bool,
    /// How long each node took to process the frame at the playhead.
    pub costs: &'a [NodeCost],
    /// Whether to show node processing times and tint the slow nodes.
    pub show_performance: bool,
    /// What the Look tool needs, while it is the tool in use.
    pub look: Option<super::look::LookContext<'a>>,
}

/// How deep [`GraphEditor::output_color`] follows inherited colours upstream.
const MAX_COLOR_DEPTH: usize = 64;

/// One end of a wire being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pin {
    Out(NodeKey, usize),
    In(NodeKey, usize),
}

/// What the pointer is doing on the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) enum Interaction {
    #[default]
    Idle,
    Pan,
    MoveNodes,
    /// Box selection from a graph-space corner. `additive` keeps the existing selection.
    BoxSelect {
        start: Pos2,
        additive: bool,
    },
    /// A wire dragged from a pin.
    Wire {
        from: Pin,
    },
    /// The right button is down: a click opens a menu, a drag pans.
    RightButton {
        start: Pos2,
        panning: bool,
    },
}

/// Where a node and its pins are, in graph space.
#[derive(Debug, Clone)]
pub(super) struct Geometry {
    pub key: NodeKey,
    pub rect: Rect,
    pub title: String,
    pub category: Option<Category>,
    pub inputs: Vec<InputPin>,
    pub outputs: Vec<(Pos2, &'static str)>,
}

/// An input pin: one of the node's inputs, or an exposed parameter.
#[derive(Debug, Clone, Copy)]
pub(super) struct InputPin {
    pub pos: Pos2,
    pub name: &'static str,
    pub required: bool,
    /// The input's index, or the parameter's [`super::param_port`].
    pub port: usize,
    pub param: bool,
}

/// Wire thickness for an output level: thin for silence, thick for a full-scale signal.
pub fn wire_width(rms: Option<f32>) -> f32 {
    match rms {
        Some(rms) => 1.0 + 6.0 * rms.max(0.0).sqrt().min(1.0),
        None => 1.5,
    }
}

impl GraphEditor {
    /// Lays out every node (graph space). Sizes follow the text they hold.
    pub(super) fn geometry(&self, ui: &Ui) -> Vec<Geometry> {
        let measure = |text: &str, size: f32| {
            ui.painter()
                .layout_no_wrap(
                    text.to_owned(),
                    FontId::proportional(size),
                    Color32::PLACEHOLDER,
                )
                .size()
                .x
        };
        self.draw_order()
            .into_iter()
            .map(|i| {
                let node = &self.nodes[i];
                let kind = self.registry.get(&node.kind);
                // A linked node is named by the project, whatever label it may have been given.
                let title = self
                    .node_is_linked(node)
                    .then(|| self.linked_title(node))
                    .flatten()
                    .or_else(|| node.label.clone())
                    .or_else(|| self.linked_title(node))
                    .unwrap_or_else(|| {
                        kind.map_or_else(
                            || tr_args("editor.node.unknown_kind", &[("kind", &node.kind)]),
                            |k| k.label().to_owned(),
                        )
                    });
                // (name, required, port, parameter): inputs, then exposed parameters.
                let mut inputs: Vec<(&'static str, bool, usize, bool)> = kind
                    .map(|k| {
                        k.spec
                            .inputs
                            .iter()
                            .enumerate()
                            .map(|(i, s)| (s.name, s.required || i == 0, i, false))
                            .collect()
                    })
                    .unwrap_or_default();
                // Channel nodes show one port per channel, named from the signal.
                if let Some((count, tag)) = self.channel_ports(node.key, true) {
                    inputs.truncate(count);
                    for (i, input) in inputs.iter_mut().enumerate() {
                        input.0 = super::channel_label(tag, i);
                    }
                }
                if let Some(k) = kind {
                    for index in self.exposed_params(node) {
                        let name = k.spec.params[index].name;
                        inputs.push((name, false, super::param_port(index), true));
                    }
                }
                let mut outputs: Vec<&'static str> = kind
                    .map(|k| super::editor_outputs(k).iter().map(|o| o.name).collect())
                    .unwrap_or_default();
                if let Some((count, tag)) = self.channel_ports(node.key, false) {
                    outputs.truncate(count);
                    for (i, output) in outputs.iter_mut().enumerate() {
                        *output = super::channel_label(tag, i);
                    }
                }
                let labelled_outputs = outputs.len() > 1;

                let widest = |names: &mut dyn Iterator<Item = &str>| {
                    names.map(|n| measure(n, LABEL_FONT)).fold(0.0, f32::max)
                };
                let in_width = widest(&mut inputs.iter().map(|i| i.0));
                let out_width = if labelled_outputs {
                    widest(&mut outputs.iter().copied())
                } else {
                    0.0
                };
                // Room for the warning badge after the title.
                let badge = if self.diagnostics(node.key).is_empty() {
                    0.0
                } else {
                    14.0 + PAD * 0.5
                };
                let width = (measure(&title, FONT) + 2.0 * PAD + badge)
                    .max(in_width + out_width + 4.0 * PAD + 12.0)
                    .max(MIN_WIDTH);
                let rows = inputs.len().max(outputs.len()).max(1);
                let rect =
                    Rect::from_min_size(node.pos, vec2(width, HEADER + rows as f32 * ROW + 6.0));
                let row_y = |i: usize| node.pos.y + HEADER + ROW * (i as f32 + 0.5) + 3.0;
                Geometry {
                    key: node.key,
                    rect,
                    title,
                    category: kind.map(|k| k.spec.category),
                    inputs: inputs
                        .iter()
                        .enumerate()
                        .map(|(i, &(name, required, port, param))| InputPin {
                            pos: pos2(rect.left(), row_y(i)),
                            name,
                            required,
                            port,
                            param,
                        })
                        .collect(),
                    outputs: outputs
                        .iter()
                        .enumerate()
                        .map(|(i, &name)| {
                            (
                                pos2(rect.right(), row_y(i)),
                                if labelled_outputs { name } else { "" },
                            )
                        })
                        .collect(),
                }
            })
            .collect()
    }

    /// The colours of output `port` of `key`, from the tag the last compile gave it. Before a
    /// node is compiled (or when it doesn't feed the output), from its output's tag rule:
    /// whatever a rule leaves open is looked up through the node's main input, upstream until
    /// it's known. A wire with no kind takes its node's category colour.
    pub(super) fn output_color(&self, key: NodeKey, port: usize, theme: &Theme) -> WireColor {
        let category = theme.category(self.kind_of(key).map(|k| k.spec.category));
        if let Some(layout) = self.compiled(key).and_then(|s| s.outputs.get(port)) {
            let (base, part) = theme.tag_colors(layout.tag);
            return WireColor {
                base: base.unwrap_or(category),
                part,
            };
        }
        let mut base = None;
        let mut part = None;
        let mut fallback = theme.unknown_category;
        let mut at = (key, port);
        // A Split output passed on the way, whose part depends on what it splits, and the kind
        // found upstream of it.
        let mut split_port = None;
        let mut split_base = None;
        for _ in 0..MAX_COLOR_DEPTH {
            let Some(kind) = self.kind_of(at.0) else {
                break;
            };
            if base.is_none() {
                fallback = theme.category(Some(kind.spec.category));
            }
            if kind.kind == SPLIT && part.is_none() && split_port.is_none() {
                split_port = Some(at.1);
            }
            let (rule_base, rule_part) = theme.rule_colors(kind.output_tag(at.1));
            base = base.or(rule_base);
            part = part.or(rule_part);
            if split_port.is_some() {
                split_base = split_base.or(rule_base);
            }
            if base.is_some() && part.is_some() && (split_port.is_none() || split_base.is_some()) {
                break;
            }
            match self.wires.iter().find(|w| w.to == (at.0, 0)) {
                Some(wire) => at = wire.from,
                None => break,
            }
        }
        // Uncompiled, a split is taken to be of RGB video, or of stereo when it splits audio.
        if let (None, Some(index)) = (part, split_port) {
            let (kind, channels) = if split_base == Some(theme.ports.audio) {
                (Kind::Audio, 2)
            } else {
                (Kind::Video, 3)
            };
            let tag = Tag {
                kind,
                ..Tag::UNKNOWN
            }
            .fit(channels);
            part = Some(theme.part_color(tag.channel_part(index)));
        }
        WireColor {
            base: base.unwrap_or(fallback),
            part: part.flatten(),
        }
    }

    /// [`Self::output_color`] for every output of every node.
    fn output_colors(&self, theme: &Theme) -> HashMap<(NodeKey, usize), WireColor> {
        let mut colors = HashMap::new();
        for node in &self.nodes {
            let outputs = self.kind_of(node.key).map_or(0, |k| k.spec.outputs.len());
            for port in 0..outputs {
                colors.insert((node.key, port), self.output_color(node.key, port, theme));
            }
        }
        colors
    }

    /// Draws the canvas and handles its input.
    pub fn show(&mut self, ui: &mut Ui, ctx: &CanvasContext) -> Response {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let visuals = ui.visuals().clone();
        let theme = Theme::for_visuals(&visuals);
        painter.rect_filled(rect, CornerRadius::ZERO, theme.canvas_bg);

        let geometry = self.geometry(ui);
        if self.fit_pending && rect.width() > 50.0 {
            self.fit(rect, &geometry);
            self.fit_pending = false;
        }
        self.draw_grid(&painter, rect, theme);

        let menus_open = self.search.is_some() || self.node_menu.is_some();
        if !menus_open {
            self.handle_input(ui, rect, &response, &geometry);
            self.handle_clipboard(ui, rect);
        }

        let view = self.view;
        let to_screen = move |p: Pos2| graph_to_screen(view, rect, p);
        let levels = GraphEditor::level_map(ctx.levels);
        let pointer = ui.input(|i| i.pointer.hover_pos());

        let output_colors = self.output_colors(theme);
        self.smooth_costs(ctx.costs);

        // Wires, behind the nodes.
        let pin_pos = |pin: Pin| -> Option<Pos2> {
            match pin {
                Pin::Out(key, i) => geometry
                    .iter()
                    .find(|g| g.key == key)?
                    .outputs
                    .get(i)
                    .map(|p| p.0),
                Pin::In(key, port) => geometry
                    .iter()
                    .find(|g| g.key == key)?
                    .inputs
                    .iter()
                    .find(|pin| pin.port == port)
                    .map(|pin| pin.pos),
            }
        };
        for wire in &self.wires {
            let (Some(a), Some(b)) = (
                pin_pos(Pin::Out(wire.from.0, wire.from.1)),
                pin_pos(Pin::In(wire.to.0, wire.to.1)),
            ) else {
                continue;
            };
            let source = self.node(wire.from.0);
            let level = source.and_then(|n| levels.get(&(n.id.as_str(), wire.from.1)).copied());
            let color = output_colors
                .get(&wire.from)
                .copied()
                .unwrap_or(WireColor::plain(theme.unknown_category));
            draw_wire(
                &painter,
                to_screen(a),
                to_screen(b),
                wire_width(level) * view.zoom.clamp(0.6, 1.6),
                color,
                ctx.wire_style,
            );
        }

        // Nodes.
        let failed = ctx
            .failure
            .and_then(|f| f.node.as_deref())
            .and_then(|id| self.key_of(id));
        let hovered_pin = pointer.and_then(|p| self.pin_at(&geometry, rect, p));
        for g in &geometry {
            if ctx.show_stats
                && let Some(stats) = self.compiled(g.key)
            {
                self.draw_node_stats(&painter, &visuals, theme, g, to_screen, stats);
            }
            self.draw_node(
                &painter,
                &visuals,
                theme,
                g,
                to_screen,
                failed == Some(g.key),
                hovered_pin,
                &output_colors,
            );
            if ctx.show_performance {
                self.draw_performance(&painter, theme, g, to_screen, visuals.weak_text_color());
            }
        }
        // Compile notes and warnings: a badge on the node, the messages in its tooltip. Notes
        // (a signal used as something it wasn't made as, often on purpose) get a quiet badge.
        for g in &geometry {
            let warnings = self.diagnostics(g.key);
            let Some(severity) = warnings.iter().map(|d| d.severity).max() else {
                continue;
            };
            if self.view.zoom <= 0.35 {
                continue;
            }
            let badge = self.badge_rect(g, to_screen);
            draw_badge(&painter, theme, badge, severity);
            let response = ui.interact(
                badge.intersect(rect),
                ui.id().with(("node-warning", g.key)),
                Sense::hover(),
            );
            response.on_hover_ui(|ui| {
                for warning in warnings {
                    let mark = match warning.severity {
                        Severity::Note => "ℹ",
                        Severity::Warning => "⚠",
                    };
                    ui.label(tr_args(
                        "editor.node.warning_line",
                        &[("mark", mark), ("message", &warning.message)],
                    ));
                }
            });
        }

        let readings = super::tooltips::Readings {
            levels: ctx.levels,
            params: ctx.params,
            look: ctx.look.as_ref(),
        };
        self.hover_tooltips(ui, rect, &geometry, &to_screen, hovered_pin, &readings);

        // The wire being dragged.
        if let (Interaction::Wire { from }, Some(pointer)) = (self.interaction, pointer)
            && let Some(start) = pin_pos(from)
        {
            let start = to_screen(start);
            let (a, b) = match from {
                Pin::Out(..) => (start, pointer),
                Pin::In(..) => (pointer, start),
            };
            let color = WireColor::plain(theme.accent);
            draw_wire(&painter, a, b, 2.0, color, WireStyle::Solid);
        }

        // Box selection.
        if let (Interaction::BoxSelect { start, .. }, Some(pointer)) = (self.interaction, pointer) {
            let area = Rect::from_two_pos(to_screen(start), pointer);
            painter.rect_filled(
                area,
                CornerRadius::same(2),
                theme.accent.gamma_multiply(0.12),
            );
            painter.rect_stroke(
                area,
                CornerRadius::same(2),
                Stroke::new(1.0, theme.accent),
                StrokeKind::Inside,
            );
        }

        if let Some(failure) = ctx.failure {
            self.error_bar(ui, rect, failure, &geometry, theme);
        }
        if self.nodes.is_empty() {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                tr("editor.canvas.empty_hint"),
                FontId::proportional(14.0),
                visuals.weak_text_color(),
            );
        }

        self.show_search(ui, &geometry);
        self.show_node_menu(ui);
        self.last_canvas = rect;
        self.last_geometry = geometry;
        response
    }

    fn handle_input(&mut self, ui: &Ui, rect: Rect, response: &Response, geometry: &[Geometry]) {
        let view = self.view;
        let to_graph = move |s: Pos2| ((s - rect.min - view.offset) / view.zoom).to_pos2();
        let (pointer, delta, modifiers) =
            ui.input(|i| (i.pointer.hover_pos(), i.pointer.delta(), i.modifiers));
        let hovered = response.hovered();

        // Zoom around the pointer.
        if hovered && let Some(p) = pointer {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (scroll * 0.0015).exp() * pinch;
            if factor != 1.0 {
                let zoom = (self.view.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
                let anchor = p - rect.min;
                self.view.offset = anchor - (anchor - self.view.offset) * (zoom / self.view.zoom);
                self.view.zoom = zoom;
            }
        }

        let pressed = |button| ui.input(|i| i.pointer.button_pressed(button));
        let released = |button| ui.input(|i| i.pointer.button_released(button));

        // Presses start an interaction, but only over the canvas.
        if hovered && let Some(p) = pointer {
            if pressed(PointerButton::Primary) {
                self.interaction = if modifiers.alt
                    && self.pin_at(geometry, rect, p).is_none()
                    && let Some(key) = self.node_at(geometry, rect, p)
                {
                    // Alt+click bypasses the node (or the whole selection it belongs to).
                    let nodes = if self.selected.contains(&key) {
                        self.selected.clone()
                    } else {
                        BTreeSet::from([key])
                    };
                    self.toggle_bypass(&nodes);
                    Interaction::Idle
                } else {
                    self.press(p, rect, geometry, modifiers.shift || modifiers.command)
                };
            } else if pressed(PointerButton::Middle) {
                self.interaction = Interaction::Pan;
            } else if pressed(PointerButton::Secondary) {
                self.interaction = Interaction::RightButton {
                    start: p,
                    panning: false,
                };
            }
        }

        match self.interaction {
            Interaction::Pan => self.view.offset += delta,
            Interaction::MoveNodes => {
                let moved = delta / self.view.zoom;
                for node in self
                    .nodes
                    .iter_mut()
                    .filter(|n| self.selected.contains(&n.key))
                {
                    node.pos += moved;
                }
            }
            Interaction::RightButton { start, panning } => {
                let panning =
                    panning || pointer.is_some_and(|p| p.distance(start) > DRAG_THRESHOLD);
                if panning {
                    self.view.offset += delta;
                }
                self.interaction = Interaction::RightButton { start, panning };
            }
            _ => {}
        }

        if released(PointerButton::Primary) {
            match self.interaction {
                Interaction::Wire { from } => {
                    let target = pointer.and_then(|p| self.pin_at(geometry, rect, p));
                    match (from, target) {
                        (Pin::Out(n, o), Some(Pin::In(m, i)))
                        | (Pin::In(m, i), Some(Pin::Out(n, o))) => {
                            self.connect_new((n, o), (m, i));
                        }
                        // Dropped on empty space: offer a node to connect to it.
                        (_, None) if pointer.is_some_and(|p| rect.contains(p)) => {
                            let p = pointer.unwrap();
                            self.search = Some(SearchMenu::new(p, to_graph(p), Some(from)));
                        }
                        _ => {}
                    }
                }
                Interaction::BoxSelect { start, additive } => {
                    if let Some(p) = pointer {
                        let area = Rect::from_two_pos(start, to_graph(p));
                        let inside: BTreeSet<NodeKey> = geometry
                            .iter()
                            .filter(|g| g.rect.intersects(area))
                            .map(|g| g.key)
                            .collect();
                        if !additive {
                            self.selected.clear();
                        }
                        self.selected.extend(inside);
                    }
                }
                _ => {}
            }
            if !matches!(self.interaction, Interaction::RightButton { .. }) {
                self.interaction = Interaction::Idle;
            }
        }
        if released(PointerButton::Middle) && self.interaction == Interaction::Pan {
            self.interaction = Interaction::Idle;
        }
        if released(PointerButton::Secondary)
            && let Interaction::RightButton { panning, .. } = self.interaction
        {
            if !panning && let Some(p) = pointer {
                match self.node_at(geometry, rect, p) {
                    Some(key) => {
                        if !self.selected.contains(&key) {
                            self.set_active(Some(key));
                        }
                        self.node_menu = Some(NodeMenu {
                            screen: p,
                            node: key,
                        });
                    }
                    None => self.search = Some(SearchMenu::new(p, to_graph(p), None)),
                }
            }
            self.interaction = Interaction::Idle;
        }

        // Keys act when the pointer is over the canvas and no text field has focus.
        if hovered && !ui.ctx().egui_wants_keyboard_input() {
            let (delete, repair, duplicate, invert, frame, select_all, escape) = ui.input(|i| {
                (
                    i.key_pressed(Key::Delete),
                    i.key_pressed(Key::Backspace),
                    i.modifiers.command && i.key_pressed(Key::D),
                    i.modifiers.shift,
                    i.key_pressed(Key::F),
                    i.modifiers.command && i.key_pressed(Key::A),
                    i.key_pressed(Key::Escape),
                )
            });
            if delete {
                self.delete_selection();
            }
            if repair {
                self.delete_selection_and_repair();
            }
            if duplicate {
                self.duplicate_selection(invert);
            }
            if frame {
                self.fit(rect, geometry);
            }
            if select_all {
                self.select_all();
            }
            if escape {
                self.interaction = Interaction::Idle;
                self.selected.clear();
            }
        }
    }

    /// Copy, cut and paste, whenever no text field has focus. The platform turns Ctrl+C, Ctrl+X and
    /// Ctrl+V into these events.
    fn handle_clipboard(&mut self, ui: &Ui, rect: Rect) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::Copy => {
                    if let Some(text) = self.copy_selection() {
                        ui.ctx().copy_text(text);
                    }
                }
                egui::Event::Cut => {
                    if let Some(text) = self.copy_selection() {
                        ui.ctx().copy_text(text);
                        self.delete_selection();
                    }
                }
                egui::Event::Paste(text) => {
                    let at = match ui.input(|i| i.pointer.hover_pos()) {
                        Some(p) if rect.contains(p) => {
                            ((p - rect.min - self.view.offset) / self.view.zoom).to_pos2()
                        }
                        _ => self.view_center(),
                    };
                    let invert = ui.input(|i| i.modifiers.shift);
                    self.paste(&text, at, self.keep_connections != invert);
                }
                _ => {}
            }
        }
    }

    /// What a left press at `p` starts.
    fn press(&mut self, p: Pos2, rect: Rect, geometry: &[Geometry], additive: bool) -> Interaction {
        let view = self.view;
        let to_graph = move |s: Pos2| ((s - rect.min - view.offset) / view.zoom).to_pos2();
        if let Some(pin) = self.pin_at(geometry, rect, p) {
            return match pin {
                // Grabbing a connected input picks its wire up to move it elsewhere.
                Pin::In(key, i) => match self.disconnect_input((key, i)) {
                    Some(wire) => Interaction::Wire {
                        from: Pin::Out(wire.from.0, wire.from.1),
                    },
                    None => Interaction::Wire { from: pin },
                },
                Pin::Out(..) => Interaction::Wire { from: pin },
            };
        }
        if let Some(key) = self.node_at(geometry, rect, p) {
            if additive {
                if !self.selected.remove(&key) {
                    self.selected.insert(key);
                }
            } else if !self.selected.contains(&key) {
                self.selected = BTreeSet::from([key]);
            }
            self.active = Some(key);
            self.raise(key);
            return Interaction::MoveNodes;
        }
        Interaction::BoxSelect {
            start: to_graph(p),
            additive,
        }
    }

    fn pin_at(&self, geometry: &[Geometry], rect: Rect, p: Pos2) -> Option<Pin> {
        let to_screen = |q: Pos2| graph_to_screen(self.view, rect, q);
        let mut best: Option<(f32, Pin)> = None;
        for g in geometry {
            let pins = g
                .inputs
                .iter()
                .map(|pin| (pin.pos, Pin::In(g.key, pin.port)))
                .chain(
                    g.outputs
                        .iter()
                        .enumerate()
                        .map(|(i, &(q, _))| (q, Pin::Out(g.key, i))),
                );
            for (q, pin) in pins {
                let distance = to_screen(q).distance(p);
                if distance <= PIN_GRAB && best.is_none_or(|(d, _)| distance < d) {
                    best = Some((distance, pin));
                }
            }
        }
        best.map(|(_, pin)| pin)
    }

    /// The topmost node under `p`.
    fn node_at(&self, geometry: &[Geometry], rect: Rect, p: Pos2) -> Option<NodeKey> {
        let graph = ((p - rect.min - self.view.offset) / self.view.zoom).to_pos2();
        geometry
            .iter()
            .rev()
            .find(|g| g.rect.contains(graph))
            .map(|g| g.key)
    }

    /// Zooms and pans so the whole graph fits the canvas.
    fn fit(&mut self, rect: Rect, geometry: &[Geometry]) {
        let Some(bounds) = geometry.iter().map(|g| g.rect).reduce(|a, b| a.union(b)) else {
            self.view = super::View::default();
            return;
        };
        let margin = 40.0;
        let zoom = ((rect.width() - 2.0 * margin) / bounds.width())
            .min((rect.height() - 2.0 * margin) / bounds.height())
            .clamp(0.65, 1.0);
        self.view.zoom = zoom;
        self.view.offset = rect.size() / 2.0 - bounds.center().to_vec2() * zoom;
    }

    fn draw_grid(&self, painter: &egui::Painter, rect: Rect, theme: &Theme) {
        let spacing = 24.0 * self.view.zoom;
        if spacing < 8.0 {
            return;
        }
        let color = theme.grid_minor;
        let origin = rect.min + self.view.offset;
        let start = origin - ((origin - rect.min) / spacing).floor() * spacing;
        let mut x = start.x;
        while x < rect.right() {
            let mut y = start.y;
            while y < rect.bottom() {
                painter.circle_filled(pos2(x, y), 1.0, color);
                y += spacing;
            }
            x += spacing;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_node(
        &self,
        painter: &egui::Painter,
        visuals: &egui::Visuals,
        theme: &Theme,
        g: &Geometry,
        to_screen: impl Fn(Pos2) -> Pos2,
        failed: bool,
        hovered_pin: Option<Pin>,
        output_colors: &HashMap<(NodeKey, usize), WireColor>,
    ) {
        let zoom = self.view.zoom;
        let rect = Rect::from_min_max(to_screen(g.rect.min), to_screen(g.rect.max));
        let rounding = CornerRadius::same((6.0 * zoom).round().clamp(1.0, 12.0) as u8);
        let color = theme.category(g.category);

        painter.rect_filled(
            rect.translate(vec2(0.0, 3.0 * zoom)),
            rounding,
            theme.node_shadow,
        );
        painter.rect_filled(rect, rounding, theme.node_body);
        let bypassed = self.node(g.key).is_some_and(|n| n.bypass);
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER * zoom));
        painter.rect_filled(
            header,
            CornerRadius {
                nw: rounding.nw,
                ne: rounding.ne,
                sw: 0,
                se: 0,
            },
            color.gamma_multiply(0.35),
        );
        painter.rect_filled(
            Rect::from_min_size(header.left_bottom(), vec2(header.width(), zoom.max(1.0))),
            CornerRadius::ZERO,
            color.gamma_multiply(0.8),
        );

        let outline = if failed {
            Stroke::new(2.0, theme.error)
        } else if self.active == Some(g.key) {
            Stroke::new(2.0, theme.accent)
        } else if self.selected.contains(&g.key) {
            Stroke::new(1.5, theme.accent.gamma_multiply(0.7))
        } else if bypassed {
            Stroke::new(2.0, visuals.weak_text_color())
        } else {
            Stroke::new(1.0, theme.node_outline)
        };
        painter.rect_stroke(rect, rounding, outline, StrokeKind::Outside);

        let text = visuals.text_color();
        if zoom > 0.35 {
            painter.text(
                header.left_center() + vec2(PAD * zoom, 0.0),
                Align2::LEFT_CENTER,
                g.title.clone(),
                FontId::proportional(FONT * zoom),
                if bypassed {
                    text.gamma_multiply(0.6)
                } else {
                    text
                },
            );
            if bypassed {
                painter.text(
                    rect.left_top() - vec2(-PAD * zoom * 0.5, 4.0 * zoom),
                    Align2::LEFT_BOTTOM,
                    tr("editor.node.bypassed"),
                    FontId::proportional(LABEL_FONT * zoom * 1.1),
                    text.gamma_multiply(0.75),
                );
            }
        }
        let labels = zoom > 0.45;
        // With several inputs, the main one (which sets the output's length and layout) gets a ring.
        let several_inputs = g.inputs.iter().filter(|pin| !pin.param).count() > 1;
        for pin in &g.inputs {
            let p = to_screen(pin.pos);
            let hovered = hovered_pin == Some(Pin::In(g.key, pin.port));
            if pin.param {
                draw_param_pin(painter, theme, p, hovered, zoom);
            } else {
                let fill = if pin.required {
                    theme.pin_required
                } else {
                    theme.pin_optional
                };
                if several_inputs && pin.port == 0 {
                    let ring = PIN_RADIUS * zoom.clamp(0.7, 1.6) + 2.5 * zoom.min(1.0);
                    painter.circle_stroke(p, ring, Stroke::new(1.2, theme.pin_required));
                }
                draw_pin(painter, theme, p, fill, hovered, zoom);
            }
            if labels {
                let color = if pin.required {
                    text
                } else {
                    visuals.weak_text_color()
                };
                painter.text(
                    p + vec2(PAD * zoom, 0.0),
                    Align2::LEFT_CENTER,
                    pin.name,
                    FontId::proportional(LABEL_FONT * zoom),
                    color,
                );
            }
        }
        for (i, &(p, name)) in g.outputs.iter().enumerate() {
            let p = to_screen(p);
            let hovered = hovered_pin == Some(Pin::Out(g.key, i));
            let fill = output_colors.get(&(g.key, i)).map_or(color, |c| c.solid());
            draw_pin(painter, theme, p, fill, hovered, zoom);
            if labels && !name.is_empty() {
                painter.text(
                    p - vec2(PAD * zoom, 0.0),
                    Align2::RIGHT_CENTER,
                    name,
                    FontId::proportional(LABEL_FONT * zoom),
                    text,
                );
            }
        }
    }

    /// Where a node's warning badge goes, in screen space: the right end of its header.
    fn badge_rect(&self, g: &Geometry, to_screen: impl Fn(Pos2) -> Pos2) -> Rect {
        let zoom = self.view.zoom;
        let size = 14.0 * zoom;
        let center = to_screen(pos2(g.rect.right(), g.rect.top() + HEADER * 0.5))
            - vec2(PAD * zoom * 0.5 + size * 0.5, 0.0);
        Rect::from_center_size(center, vec2(size, size))
    }

    /// The node's latency and warmup in small text under it, in the warning colour when the warmup
    /// hit the limit.
    fn draw_node_stats(
        &self,
        painter: &egui::Painter,
        visuals: &egui::Visuals,
        theme: &Theme,
        g: &Geometry,
        to_screen: impl Fn(Pos2) -> Pos2,
        stats: &NodeStats,
    ) {
        if self.view.zoom <= 0.45 || (stats.latency_frames <= 0.0 && stats.warmup_frames == 0) {
            return;
        }
        let mut parts = Vec::new();
        if stats.latency_frames > 0.0 {
            parts.push(tr_args(
                "editor.node.latency",
                &[("frames", &format_frames(stats.latency_frames))],
            ));
        }
        if stats.warmup_frames > 0 {
            parts.push(if stats.warmup_frames == UNBOUNDED_WARMUP {
                tr("editor.node.warmup_never").to_owned()
            } else {
                tr_args(
                    "editor.node.warmup",
                    &[("frames", &stats.warmup_frames.to_string())],
                )
            });
        }
        let mut text = parts.join(" · ");
        let color = if stats.warmup_frames > self.max_warmup_frames {
            text = tr_args(
                "editor.node.warmup_over_limit",
                &[
                    ("text", &text),
                    ("limit", &self.max_warmup_frames.to_string()),
                ],
            );
            theme.warning
        } else {
            visuals.weak_text_color()
        };
        let rect = Rect::from_min_max(to_screen(g.rect.min), to_screen(g.rect.max));
        painter.text(
            rect.left_bottom() + vec2(PAD * self.view.zoom * 0.5, 5.0 * self.view.zoom),
            Align2::LEFT_TOP,
            text,
            FontId::proportional(LABEL_FONT * self.view.zoom),
            color,
        );
    }

    fn error_bar(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        failure: &Failure,
        geometry: &[Geometry],
        theme: &Theme,
    ) {
        let height = 30.0;
        let bar = Rect::from_min_max(pos2(rect.left(), rect.bottom() - height), rect.max);
        let response = ui.interact(bar, ui.id().with("graph-error"), Sense::click());
        let fill = if response.hovered() {
            theme.error_bar_hover
        } else {
            theme.error_bar
        };
        ui.painter().rect_filled(bar, CornerRadius::ZERO, fill);
        let node = failure.node.as_deref().and_then(|id| self.key_of(id));
        // Name the node as the user sees it rather than by its id.
        let message = match (
            failure.node.as_deref(),
            node.and_then(|k| geometry.iter().find(|g| g.key == k)),
        ) {
            (Some(_), Some(g)) => tr_args(
                "editor.error_bar.node_message",
                &[("node", &g.title), ("detail", &failure.detail)],
            ),
            _ => failure.message.clone(),
        };
        let text = egui::RichText::new(tr_args("editor.error_bar", &[("message", &message)]))
            .color(theme.error_bar_text);
        ui.put(
            bar.shrink2(vec2(10.0, 0.0)),
            egui::Label::new(text).truncate(),
        );
        if node.is_some() {
            response.clone().on_hover_text(tr("editor.error_bar.hover"));
        }
        if response.clicked()
            && let Some(key) = node
            && let Some(g) = geometry.iter().find(|g| g.key == key)
        {
            self.set_active(Some(key));
            self.view.offset = rect.size() / 2.0 - g.rect.center().to_vec2() * self.view.zoom;
        }
    }
}

/// Frames as short text: whole numbers plainly, fractions to two places.
fn format_frames(frames: f64) -> String {
    if (frames - frames.round()).abs() < 0.005 {
        format!("{}", frames.round())
    } else {
        format!("{frames:.2}")
    }
}

fn draw_pin(
    painter: &egui::Painter,
    theme: &Theme,
    p: Pos2,
    fill: Color32,
    hovered: bool,
    zoom: f32,
) {
    let radius = PIN_RADIUS * zoom.clamp(0.7, 1.6);
    if hovered {
        painter.circle_stroke(p, radius + 3.0, Stroke::new(1.5, theme.accent));
    }
    painter.circle(p, radius, fill, Stroke::new(1.0, theme.pin_outline));
}

/// A node's diagnostic badge: an "i" on a quiet disc for notes, an exclamation mark on a
/// warning-coloured one for warnings.
fn draw_badge(painter: &egui::Painter, theme: &Theme, rect: Rect, severity: Severity) {
    let (fill, stroke, mark, text) = match severity {
        Severity::Note => (theme.node_body, theme.text_dim, "i", theme.text_dim),
        Severity::Warning => (theme.warning, theme.pin_outline, "!", theme.block_text),
    };
    painter.circle(
        rect.center(),
        rect.width() * 0.5,
        fill,
        Stroke::new(1.0, stroke),
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        mark,
        FontId::proportional(rect.height() * 0.85),
        text,
    );
}

/// A parameter's pin: a diamond, so parameters read apart from inputs.
fn draw_param_pin(painter: &egui::Painter, theme: &Theme, p: Pos2, hovered: bool, zoom: f32) {
    let r = PIN_RADIUS * 1.25 * zoom.clamp(0.7, 1.6);
    if hovered {
        painter.circle_stroke(p, r + 3.0, Stroke::new(1.5, theme.accent));
    }
    painter.add(egui::Shape::convex_polygon(
        vec![
            p + vec2(0.0, -r),
            p + vec2(r, 0.0),
            p + vec2(0.0, r),
            p + vec2(-r, 0.0),
        ],
        theme.accent,
        Stroke::new(1.0, theme.pin_outline),
    ));
}

/// A wire's colours: the kind of signal (video or audio), and the part of it the wire carries
/// (a colour channel or frequency band), if any.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct WireColor {
    pub base: Color32,
    pub part: Option<Color32>,
}

impl WireColor {
    fn plain(color: Color32) -> Self {
        Self {
            base: color,
            part: None,
        }
    }

    /// The single colour that best identifies the wire: its part, or else its kind.
    pub fn solid(self) -> Color32 {
        self.part.unwrap_or(self.base)
    }
}

/// Rings of the gradient style, from the edge (base colour) to the centre (part colour).
const GRADIENT_STEPS: usize = 6;
/// The thinnest the crisp middle of a wire gets in the styles that draw one, so a quiet wire still
/// shows its own colour.
const MIN_CORE: f32 = 2.0;

/// The control points of the curve a wire is drawn along.
pub(super) fn wire_points(from: Pos2, to: Pos2) -> [Pos2; 4] {
    let reach = ((to.x - from.x).abs() * 0.5).max(40.0);
    [from, from + vec2(reach, 0.0), to - vec2(reach, 0.0), to]
}

fn draw_wire(
    painter: &egui::Painter,
    from: Pos2,
    to: Pos2,
    width: f32,
    color: WireColor,
    style: WireStyle,
) {
    let points = wire_points(from, to);
    let stroke = |width: f32, color: Color32| {
        painter.add(CubicBezierShape::from_points_stroke(
            points,
            false,
            Color32::TRANSPARENT,
            Stroke::new(width, color),
        ));
    };
    match (style, color.part) {
        (WireStyle::Outline, Some(part)) => {
            let (core, border) = outline_widths(width);
            stroke(core + 2.0 * border, part);
            stroke(core, color.base);
        }
        (WireStyle::Gradient, Some(part)) => {
            // Concentric strokes, widest first: base at the edge blending to the part at the centre.
            let (outer, inner) = gradient_widths(width);
            for step in 0..GRADIENT_STEPS {
                let t = step as f32 / (GRADIENT_STEPS - 1) as f32;
                stroke(
                    outer + (inner - outer) * t,
                    color.base.lerp_to_gamma(part, t),
                );
            }
        }
        (WireStyle::Glow, _) => {
            let core = width.max(MIN_CORE);
            let glow = Glow::new(color.part.unwrap_or(color.base), glow_radius(core), 0.55);
            glow.stroke(core, &stroke);
            stroke(core, color.base);
        }
        // Solid, or a whole signal with no part to show.
        _ => stroke(width, color.solid().gamma_multiply(0.9)),
    }
}

/// The widths of the outline style: the crisp middle, and the border on each side of it. The
/// border grows with the wire, so thin and thick wires keep the same proportions; it never goes
/// under a pixel, so the outline always shows, and the middle never under [`MIN_CORE`], so the
/// base colour does too.
fn outline_widths(width: f32) -> (f32, f32) {
    let core = width.max(MIN_CORE);
    (core, (core * 0.4).max(1.0))
}

/// The widths of the gradient style: the outermost ring, and the innermost.
fn gradient_widths(width: f32) -> (f32, f32) {
    let outer = width.max(MIN_CORE) * 1.5 + 1.5;
    (outer, (outer * 0.25).max(1.0))
}

/// How far the glow of a wire with a middle `core` wide reaches past it.
fn glow_radius(core: f32) -> f32 {
    3.0 + core * 0.6
}

/// Graph space to screen space.
pub(super) fn graph_to_screen(view: super::View, canvas: Rect, p: Pos2) -> Pos2 {
    canvas.min + view.offset + p.to_vec2() * view.zoom
}

#[cfg(test)]
mod tests {
    use eframe::egui::Vec2;

    use super::*;

    #[test]
    fn wire_width_follows_level() {
        assert_eq!(wire_width(None), 1.5);
        assert_eq!(wire_width(Some(0.0)), 1.0);
        assert!(wire_width(Some(0.1)) < wire_width(Some(0.5)));
        assert_eq!(wire_width(Some(4.0)), 7.0, "capped");
    }

    #[test]
    fn wire_styles_keep_both_colours_visible_at_any_thickness() {
        for width in [1.0, 1.5, 3.0, 7.0] {
            let (core, border) = outline_widths(width);
            assert!(core >= MIN_CORE && core >= width, "the base colour shows");
            assert!(border >= 1.0, "the outline shows");
            assert!(
                border <= core * 0.5,
                "and doesn't overpower the base: {width}"
            );

            let (outer, inner) = gradient_widths(width);
            assert!(outer > inner && inner >= 1.0, "{width}");
        }
        // Thicker wires get thicker outlines.
        assert!(outline_widths(7.0).1 > outline_widths(1.0).1);
    }

    #[test]
    fn wires_take_their_colour_from_what_they_carry() {
        let graph = rastersong_engine::GraphDesc::from_json(include_str!(
            "../../../../examples/graphs/am_bands.json"
        ))
        .unwrap();
        let editor = GraphEditor::new(&graph);
        let theme = &Theme::DARK;
        let color = |editor: &GraphEditor, id: &str| {
            editor.output_color(editor.key_of(id).unwrap(), 0, theme)
        };
        let p = &theme.ports;
        let wire = |base, part| WireColor { base, part };
        assert_eq!(color(&editor, "split"), wire(p.video, Some(p.red)));
        assert_eq!(color(&editor, "bands"), wire(p.audio, Some(p.low)));
        // Effects follow their main input: the red carrier stays red video through modulation.
        assert_eq!(color(&editor, "am_red"), wire(p.video, Some(p.red)));
        assert_eq!(color(&editor, "combine"), wire(p.video, None));
        assert_eq!(color(&editor, "audio"), wire(p.audio, None));

        // Converting the red channel to audio keeps it red, as audio.
        let mut editor = editor;
        let split = editor.key_of("split").unwrap();
        let to_audio = editor.add_node("to_audio", Pos2::ZERO).unwrap();
        editor.connect((split, 0), (to_audio, 0));
        assert_eq!(color(&editor, "to_audio"), wire(p.audio, Some(p.red)));
    }

    #[test]
    fn view_maps_graph_to_screen() {
        let view = super::super::View {
            offset: Vec2::new(10.0, 20.0),
            zoom: 2.0,
        };
        let canvas = Rect::from_min_size(pos2(100.0, 100.0), vec2(500.0, 500.0));
        assert_eq!(
            graph_to_screen(view, canvas, pos2(5.0, 5.0)),
            pos2(120.0, 130.0)
        );
    }
}
