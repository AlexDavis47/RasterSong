//! Drawing and interaction for the node canvas.
//!
//! - Scroll wheel zooms around the pointer; middle- or right-drag pans.
//! - Left-drag on empty space box-selects (Shift adds); click a node to select it, Shift-click to
//!   add or remove it; drag nodes to move them.
//! - Drag from an output to an input (or the reverse) to connect; drag a connected input to
//!   pick its wire up again. Dropping a wire on empty space opens the node search, connected.
//! - Right-click empty space to add a node there; right-click a node for its menu.
//! - Delete removes the selection, Ctrl+D duplicates it, F frames the whole graph.

use std::collections::BTreeSet;

use eframe::egui::epaint::CubicBezierShape;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Key, PointerButton, Pos2, Rect, Response, Sense,
    Stroke, StrokeKind, Ui, pos2, vec2,
};
use rastersong_engine::{Category, Failure, OutputLevel};

use super::search::{NodeMenu, SearchMenu};
use super::{GraphEditor, NodeKey};
use crate::theme::Theme;

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
    /// Why the graph can't render, shown along the bottom of the canvas.
    pub failure: Option<&'a Failure>,
}

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
    pub inputs: Vec<(Pos2, &'static str, bool)>,
    pub outputs: Vec<(Pos2, &'static str)>,
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
                let title = node.label.clone().unwrap_or_else(|| {
                    kind.map_or_else(
                        || format!("{} (unknown)", node.kind),
                        |k| k.spec.label.to_owned(),
                    )
                });
                let inputs: Vec<(&'static str, bool)> = kind
                    .map(|k| {
                        k.inputs
                            .iter()
                            .enumerate()
                            .map(|(i, s)| (s.name, s.required || i == 0))
                            .collect()
                    })
                    .unwrap_or_default();
                let outputs: Vec<&'static str> = kind
                    .map(|k| super::editor_outputs(k).to_vec())
                    .unwrap_or_default();
                let labelled_outputs = outputs.len() > 1;

                let widest = |names: &mut dyn Iterator<Item = &str>| {
                    names.map(|n| measure(n, LABEL_FONT)).fold(0.0, f32::max)
                };
                let in_width = widest(&mut inputs.iter().map(|(n, _)| *n));
                let out_width = if labelled_outputs {
                    widest(&mut outputs.iter().copied())
                } else {
                    0.0
                };
                let width = (measure(&title, FONT) + 2.0 * PAD)
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
                        .map(|(i, &(name, required))| (pos2(rect.left(), row_y(i)), name, required))
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

        // Wires, behind the nodes.
        let pin_pos = |pin: Pin| -> Option<Pos2> {
            match pin {
                Pin::Out(key, i) => geometry
                    .iter()
                    .find(|g| g.key == key)?
                    .outputs
                    .get(i)
                    .map(|p| p.0),
                Pin::In(key, i) => geometry
                    .iter()
                    .find(|g| g.key == key)?
                    .inputs
                    .get(i)
                    .map(|p| p.0),
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
            let category = geometry
                .iter()
                .find(|g| g.key == wire.from.0)
                .and_then(|g| g.category);
            let color = theme.category(category).gamma_multiply(0.85);
            draw_wire(
                &painter,
                to_screen(a),
                to_screen(b),
                wire_width(level) * view.zoom.clamp(0.6, 1.6),
                color,
            );
        }

        // Nodes.
        let failed = ctx
            .failure
            .and_then(|f| f.node.as_deref())
            .and_then(|id| self.key_of(id));
        let hovered_pin = pointer.and_then(|p| self.pin_at(&geometry, rect, p));
        for g in &geometry {
            self.draw_node(
                &painter,
                &visuals,
                theme,
                g,
                to_screen,
                failed == Some(g.key),
                hovered_pin,
            );
        }

        // The wire being dragged.
        if let (Interaction::Wire { from }, Some(pointer)) = (self.interaction, pointer)
            && let Some(start) = pin_pos(from)
        {
            let start = to_screen(start);
            let (a, b) = match from {
                Pin::Out(..) => (start, pointer),
                Pin::In(..) => (pointer, start),
            };
            draw_wire(&painter, a, b, 2.0, theme.accent);
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
                "Right-click to add a node",
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
                self.interaction =
                    self.press(p, rect, geometry, modifiers.shift || modifiers.command);
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
                            self.connect((n, o), (m, i));
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
            let (delete, duplicate, frame, select_all, escape) = ui.input(|i| {
                (
                    i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                    i.modifiers.command && i.key_pressed(Key::D),
                    i.key_pressed(Key::F),
                    i.modifiers.command && i.key_pressed(Key::A),
                    i.key_pressed(Key::Escape),
                )
            });
            if delete {
                self.delete_selection();
            }
            if duplicate {
                self.duplicate_selection();
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
                    self.paste(&text, at);
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
                .enumerate()
                .map(|(i, &(q, ..))| (q, Pin::In(g.key, i)))
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
        } else {
            Stroke::new(1.0, theme.node_outline)
        };
        painter.rect_stroke(rect, rounding, outline, StrokeKind::Outside);

        let text = visuals.text_color();
        if zoom > 0.35 {
            painter.text(
                header.left_center() + vec2(PAD * zoom, 0.0),
                Align2::LEFT_CENTER,
                &g.title,
                FontId::proportional(FONT * zoom),
                text,
            );
        }
        let labels = zoom > 0.45;
        for (i, &(p, name, required)) in g.inputs.iter().enumerate() {
            let p = to_screen(p);
            let hovered = hovered_pin == Some(Pin::In(g.key, i));
            let fill = if required {
                theme.pin_required
            } else {
                theme.pin_optional
            };
            draw_pin(painter, theme, p, fill, hovered, zoom);
            if labels {
                let color = if required {
                    text
                } else {
                    visuals.weak_text_color()
                };
                painter.text(
                    p + vec2(PAD * zoom, 0.0),
                    Align2::LEFT_CENTER,
                    name,
                    FontId::proportional(LABEL_FONT * zoom),
                    color,
                );
            }
        }
        for (i, &(p, name)) in g.outputs.iter().enumerate() {
            let p = to_screen(p);
            let hovered = hovered_pin == Some(Pin::Out(g.key, i));
            draw_pin(painter, theme, p, color, hovered, zoom);
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
            (Some(id), Some(g)) => {
                let detail = failure
                    .message
                    .strip_prefix(&format!("node `{id}`: "))
                    .unwrap_or(&failure.message);
                format!("{}: {detail}", g.title)
            }
            _ => failure.message.clone(),
        };
        let text = egui::RichText::new(format!("⚠  {message}")).color(theme.error_bar_text);
        ui.put(
            bar.shrink2(vec2(10.0, 0.0)),
            egui::Label::new(text).truncate(),
        );
        if node.is_some() {
            response.clone().on_hover_text("Click to show the node");
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

fn draw_wire(painter: &egui::Painter, from: Pos2, to: Pos2, width: f32, color: Color32) {
    let reach = ((to.x - from.x).abs() * 0.5).max(40.0);
    let curve = CubicBezierShape::from_points_stroke(
        [from, from + vec2(reach, 0.0), to - vec2(reach, 0.0), to],
        false,
        Color32::TRANSPARENT,
        Stroke::new(width, color),
    );
    painter.add(curve);
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
