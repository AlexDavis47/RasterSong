//! Tooltips for wires and pins: what a signal is and what it is doing at the playhead.

use eframe::egui::{self, Color32, Id, Margin, Pos2, Rect, Sense, Ui, Vec2, vec2};
use rastersong_engine::{Layout, OutputLevel, ParamLevel};
use rastersong_lang::{tr, tr_args};

use super::canvas::{Geometry, Pin, wire_points};
use super::inspect::{InspectContext, InspectMode, Visual, show_view};
use super::{GraphEditor, NodeKey, Wire, modulation};
use crate::theme::{self, Theme};
use crate::widgets::{DRUM_WIDTH, drum, kbd_text, section_label, signal_meter};

/// How close (in screen pixels) the pointer must be to a wire for its tooltip.
const WIRE_GRAB: f32 = 6.0;
/// Points the wire's curve is sampled at to find the distance to it.
const WIRE_SAMPLES: usize = 32;

/// The levels the tooltips read, as the canvas was given them.
#[derive(Debug, Clone, Copy, Default)]
pub struct Readings<'a> {
    pub levels: &'a [OutputLevel],
    pub params: &'a [ParamLevel],
    /// How inspecting works, when the app gave the editor a way to ask the engine.
    pub inspect: Option<&'a InspectContext<'a>>,
}

/// How far `p` is from the cubic curve through `points`.
fn distance_to_curve(points: [Pos2; 4], p: Pos2) -> f32 {
    let at = |t: f32| {
        let u = 1.0 - t;
        let weights = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        let mut sum = Pos2::ZERO.to_vec2();
        for (point, w) in points.iter().zip(weights) {
            sum += point.to_vec2() * w;
        }
        sum.to_pos2()
    };
    (0..=WIRE_SAMPLES)
        .map(|i| at(i as f32 / WIRE_SAMPLES as f32).distance(p))
        .fold(f32::INFINITY, f32::min)
}

/// Minimum width of a tooltip, and of the meter in it.
const TIP_WIDTH: f32 = 240.0;
const METER_WIDTH: f32 = 130.0;

/// The wire probe's width: the drum, then the view with its margins.
const PROBE_WIDTH: f32 = DRUM_WIDTH + super::inspect::WIDTH + 24.0;
/// Where the probe opens from the pointer, and how far outside it the pointer may stray.
const PROBE_OFFSET: Vec2 = vec2(12.0, 12.0);
const PROBE_REACH: f32 = 6.0;

/// What the probe is about: a wire or a pin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Target {
    Wire(Wire),
    Pin(Pin),
}

/// The wire probe: a popup about a connection that stays open while the pointer is on it, or on
/// its way to it from where it opened, so its drum can be clicked.
#[derive(Debug, Clone, Copy)]
pub(super) struct Probe {
    target: Target,
    /// Where the pointer was when it opened.
    origin: Pos2,
    /// Where it and its drum were drawn last frame.
    pub(super) rect: Rect,
    pub(super) drum: Rect,
}

impl Probe {
    fn holds(&self, pointer: Pos2) -> bool {
        self.rect.expand(PROBE_REACH).contains(pointer)
    }

    /// Whether `pointer` is between where the probe opened and the probe.
    fn on_the_way(&self, pointer: Pos2) -> bool {
        Rect::from_two_pos(self.origin, self.rect.left_top())
            .expand(PROBE_REACH)
            .contains(pointer)
    }
}

/// The probe's drum, which keeps keyboard focus from frame to frame.
fn drum_id() -> Id {
    Id::new("wire-probe-drum")
}

/// A reading with a sign and a fixed number of characters (`" +0.1234"`), so columns line up.
fn number(x: f32) -> String {
    format!("{x:>+9.4}")
}

impl GraphEditor {
    /// The wire nearest `pointer`, if one is within reach.
    fn wire_near(
        &self,
        geometry: &[Geometry],
        to_screen: &dyn Fn(Pos2) -> Pos2,
        pointer: Pos2,
    ) -> Option<Wire> {
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
        self.wires
            .iter()
            .filter_map(|wire| {
                let a = to_screen(pin_pos(Pin::Out(wire.from.0, wire.from.1))?);
                let b = to_screen(pin_pos(Pin::In(wire.to.0, wire.to.1))?);
                let d = distance_to_curve(wire_points(a, b), pointer);
                (d <= WIRE_GRAB).then_some((d, *wire))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, wire)| wire)
    }

    /// What an output carries at the playhead: its layout and tag, then its level.
    fn signal_tip(&self, key: NodeKey, output: usize, readings: &Readings) -> Tip {
        let mut tip = Tip::default();
        let Some(node) = self.node(key) else {
            return tip;
        };
        if let Some(layout) = self
            .compiled(key)
            .and_then(|stats| stats.outputs.get(output))
        {
            tip.facts = layout_lines(layout);
            tip.scale = Some(crate::widgets::Scale::of(&layout.tag));
            tip.audio = layout.tag.kind == rastersong_engine::Kind::Audio;
        }
        tip.source = Some((node.id.to_string(), output));
        tip.output = Some((key, output));
        let port = self
            .registry
            .get(&node.kind)
            .and_then(|kind| kind.spec.outputs.get(output))
            .map_or("", |o| o.name);
        tip.ends = vec![end(&node.id, port)];
        tip.level = readings
            .levels
            .iter()
            .find(|l| *l.node == *node.id && l.output == output)
            .cloned();
        tip
    }

    /// Shows a tooltip for the wire or pin under the pointer, if there is one.
    pub(super) fn hover_tooltips(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        geometry: &[Geometry],
        to_screen: &dyn Fn(Pos2) -> Pos2,
        hovered_pin: Option<Pin>,
        readings: &Readings,
    ) {
        self.hovered_output = None;
        let idle = self.interaction == super::canvas::Interaction::Idle;
        let probe = self.probe.take().filter(|_| idle);
        // Another window over the canvas takes the pointer: nothing under it is hovered. The
        // probe's own popup doesn't count.
        let probe_layer = egui::LayerId::new(egui::Order::Foreground, ui.id().with("wire-probe"));
        let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|p| {
            ui.ctx()
                .layer_id_at(*p)
                .is_none_or(|layer| layer == ui.layer_id() || layer == probe_layer)
        });
        // On the probe, it stays about what it was opened for, whatever is under it.
        let target = pointer.and_then(|pointer| match probe {
            Some(probe) if probe.holds(pointer) => Some(probe.target),
            _ => self
                .target_at(rect, geometry, to_screen, hovered_pin, pointer)
                .or_else(|| probe.filter(|p| p.on_the_way(pointer)).map(|p| p.target)),
        });
        let close = |ui: &Ui| ui.memory_mut(|m| m.surrender_focus(drum_id()));
        let (Some(pointer), Some(target)) = (pointer, target.filter(|_| idle)) else {
            self.probe_wait = None;
            close(ui);
            return;
        };
        let mut tip = match target {
            Target::Wire(wire) => self.wire_tip(wire, readings),
            Target::Pin(pin) => self.pin_tip(pin, readings),
        };
        if tip.is_empty() {
            close(ui);
            return;
        }
        self.hovered_output = tip.source.clone();
        self.hovered_audio = tip.audio;
        let (Some(inspect), Some((node, output))) = (readings.inspect, tip.source.clone()) else {
            // Nothing to look at: a plain tooltip.
            close(ui);
            let spot = Rect::from_center_size(pointer, egui::vec2(2.0, 2.0));
            ui.interact(spot, ui.id().with("canvas-tooltip"), Sense::hover())
                .on_hover_ui(|ui| tip.show(ui));
            return;
        };
        // Like any tooltip, the probe opens once the pointer has stopped on its target and rested
        // there for the tooltip delay: sliding along a wire doesn't open it.
        if !probe.is_some_and(|p| p.target == target) {
            let now = ui.input(|i| i.time);
            let since = match self.probe_wait {
                Some((waiting, since)) if waiting == target => since,
                _ => {
                    self.probe_wait = Some((target, now));
                    now
                }
            };
            let rested = ui.input(|i| {
                let still = i.pointer.is_still() && !i.is_scrolling();
                still.then(|| {
                    f64::from(
                        i.pointer
                            .time_since_last_movement()
                            .min(i.time_since_last_scroll()),
                    )
                })
            });
            let Some(rested) = rested else {
                ui.ctx().request_repaint();
                close(ui);
                return;
            };
            let left = f64::from(ui.style().interaction.tooltip_delay) - (now - since).min(rested);
            if left > 0.0 {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(left));
                close(ui);
                return;
            }
        }
        tip.mode = inspect.views.of(tip.audio);
        if tip.mode != InspectMode::Readings {
            tip.visual = Some(super::inspect::inspect(ui, &node, output, inspect));
        }
        let theme = Theme::of(ui.ctx());
        tip.color = tip.output.map(|(key, port)| {
            let color = self.output_color(key, port, theme);
            color.part.unwrap_or(color.base)
        });
        let origin = match probe {
            Some(probe) if probe.target == target => probe.origin,
            _ => pointer,
        };
        let area = egui::Area::new(ui.id().with("wire-probe"))
            .order(egui::Order::Foreground)
            .fixed_pos(origin + PROBE_OFFSET)
            .constrain(true)
            .show(ui.ctx(), |ui| tip.show_probe(ui));
        let (turned, drum) = area.inner;
        self.view_turn += turned;
        self.probe = Some(Probe {
            target,
            origin,
            rect: area.response.rect,
            drum,
        });
    }

    /// The pin or wire under `pointer` on the canvas, if there is one and no node is over it.
    fn target_at(
        &self,
        rect: Rect,
        geometry: &[Geometry],
        to_screen: &dyn Fn(Pos2) -> Pos2,
        hovered_pin: Option<Pin>,
        pointer: Pos2,
    ) -> Option<Target> {
        if !rect.contains(pointer) {
            return None;
        }
        if let Some(pin) = hovered_pin {
            return Some(Target::Pin(pin));
        }
        let over_node = geometry.iter().any(|g| {
            Rect::from_two_pos(to_screen(g.rect.min), to_screen(g.rect.max)).contains(pointer)
        });
        if over_node {
            return None;
        }
        self.wire_near(geometry, to_screen, pointer)
            .map(Target::Wire)
    }

    fn name_of(&self, key: NodeKey) -> String {
        self.node(key).map(|n| n.id.to_string()).unwrap_or_default()
    }

    fn wire_tip(&self, wire: Wire, readings: &Readings) -> Tip {
        let port_name = |key: NodeKey, port: usize, input: bool| -> String {
            let Some(node) = self.node(key) else {
                return String::new();
            };
            let Some(kind) = self.registry.get(&node.kind) else {
                return String::new();
            };
            if input {
                match modulation::as_param(port) {
                    Some(index) => kind
                        .spec
                        .params
                        .get(index)
                        .map(|p| format!("@{}", p.name))
                        .unwrap_or_default(),
                    None => kind
                        .spec
                        .inputs
                        .get(port)
                        .map(|i| i.name.to_owned())
                        .unwrap_or_default(),
                }
            } else {
                kind.spec
                    .outputs
                    .get(wire.from.1)
                    .map(|o| o.name.to_owned())
                    .unwrap_or_default()
            }
        };
        let mut tip = self.signal_tip(wire.from.0, wire.from.1, readings);
        let (from, out) = (
            self.name_of(wire.from.0),
            port_name(wire.from.0, wire.from.1, false),
        );
        let (to, into) = (
            self.name_of(wire.to.0),
            port_name(wire.to.0, wire.to.1, true),
        );
        tip.title = Some(tr_args(
            "editor.tip.wire",
            &[("from", &from), ("out", &out), ("to", &to), ("in", &into)],
        ));
        tip.ends = vec![end(&from, &out), end(&to, &into)];
        tip
    }

    fn pin_tip(&self, pin: Pin, readings: &Readings) -> Tip {
        match pin {
            Pin::Out(key, output) => self.signal_tip(key, output, readings),
            Pin::In(key, port) => {
                if let Some(index) = modulation::as_param(port) {
                    let id = self.name_of(key);
                    return Tip {
                        value: readings
                            .params
                            .iter()
                            .find(|p| *p.node == *id && p.index == index)
                            .map(|p| p.value),
                        ..Tip::default()
                    };
                }
                match self.wires.iter().find(|w| w.to == (key, port)) {
                    Some(wire) => self.signal_tip(wire.from.0, wire.from.1, readings),
                    None => Tip {
                        title: Some(tr("editor.tip.unconnected").to_owned()),
                        ..Tip::default()
                    },
                }
            }
        }
    }
}

/// What a tooltip says: a title, facts about the signal, and its readings at the playhead.
#[derive(Default)]
struct Tip {
    title: Option<String>,
    facts: Vec<String>,
    /// The node output the tip is about (by id, and by key), and whether it carries audio.
    source: Option<(String, usize)>,
    output: Option<(NodeKey, usize)>,
    /// The ends of the connection ("Split.low", "Delay.in"), or the output alone for a pin.
    ends: Vec<String>,
    /// The colour of the wire.
    color: Option<Color32>,
    audio: bool,
    /// The picture, scope or spectrum of the signal, and which it is.
    visual: Option<Visual>,
    mode: InspectMode,
    level: Option<OutputLevel>,
    /// How the level is metered, from the signal's tag.
    scale: Option<crate::widgets::Scale>,
    /// A modulated parameter's value now.
    value: Option<f32>,
}

impl Tip {
    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.facts.is_empty()
            && self.level.is_none()
            && self.value.is_none()
    }

    /// Lays the tip out: text first, then a grid whose rows are a label and a fixed-width
    /// value, so changing numbers never move anything.
    fn show(&self, ui: &mut Ui) {
        self.content(ui);
    }

    /// Lays the probe out: the connection's ends and what it carries, the drum of views beside
    /// the view, the meter and the readings, and the keys. Returns the steps the drum was turned,
    /// and where it is.
    fn show_probe(&self, ui: &mut Ui) -> (i32, Rect) {
        let theme = Theme::of(ui.ctx());
        let mut turned = 0;
        let mut drum_rect = Rect::NOTHING;
        egui::Frame::popup(ui.style())
            .inner_margin(0)
            .show(ui, |ui| {
                ui.set_width(PROBE_WIDTH);
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                // Rows are as tall as their text, not as tall as a control; a row that centres
                // things says how tall it is.
                ui.spacing_mut().interact_size.y = 0.0;
                let header = Margin {
                    left: 12,
                    right: 12,
                    top: 10,
                    bottom: 8,
                };
                section(ui, header, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    self.heading(ui, theme);
                    if !self.facts.is_empty() {
                        ui.label(
                            egui::RichText::new(self.facts.join(" · "))
                                .monospace()
                                .size(11.0)
                                .color(theme.text_dim),
                        );
                    }
                });
                divider(ui, theme);
                let row = ui.horizontal_top(|ui| {
                    let items: Vec<_> = InspectMode::ALL
                        .iter()
                        .map(|mode| (mode.icon(), mode.label()))
                        .collect();
                    let selected = InspectMode::ALL
                        .iter()
                        .position(|mode| *mode == self.mode)
                        .unwrap_or(0);
                    let drum = drum(ui, drum_id(), &items, selected);
                    turned = drum.turned;
                    drum_rect = drum.response.rect;
                    section(ui, Margin::symmetric(12, 10), |ui| {
                        ui.set_width(super::inspect::WIDTH);
                        ui.spacing_mut().item_spacing.y = 6.0;
                        if let Some(visual) = &self.visual {
                            show_view(ui, visual, self.mode, self.color.unwrap_or(theme.accent));
                        }
                        ui.label(
                            egui::RichText::new(self.mode.about())
                                .size(11.0)
                                .color(theme.surfaces.text_faint),
                        );
                    });
                    drum.response.rect.right()
                });
                ui.painter().vline(
                    row.inner,
                    row.response.rect.y_range(),
                    egui::Stroke::new(1.0, theme.surfaces.card_border),
                );
                if let Some(level) = &self.level {
                    divider(ui, theme);
                    self.probe_readings(ui, theme, level);
                }
                divider(ui, theme);
                section(ui, Margin::symmetric(12, 8), |ui| {
                    ui.spacing_mut().interact_size.y = 17.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        for (text, key) in [
                            ("inspect.turn", "inspect.key.views"),
                            ("inspect.listen", "inspect.key.listen"),
                        ] {
                            kbd_text(ui, tr(text), tr(key), 11.0, theme.text_dim);
                        }
                    });
                });
            });
        (turned, drum_rect)
    }

    /// The probe's first line: the wire's colour, then its ends with an arrow between.
    fn heading(&self, ui: &mut Ui, theme: &Theme) {
        ui.spacing_mut().interact_size.y = 18.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if let Some(color) = self.color {
                let (rect, _) = ui.allocate_exact_size(vec2(14.0, 3.0), Sense::hover());
                ui.painter().rect_filled(rect, 2.0, color);
            }
            for (i, end) in self.ends.iter().enumerate() {
                if i > 0 {
                    ui.label(
                        egui::RichText::new(tr("editor.tip.arrow"))
                            .color(theme.surfaces.text_faint),
                    );
                }
                ui.label(
                    egui::RichText::new(end)
                        .font(theme::semibold(theme::TITLE_SIZE))
                        .color(theme.surfaces.text),
                );
            }
        });
    }

    /// The meter across, then the mean, min, max and RMS in four columns.
    fn probe_readings(&self, ui: &mut Ui, theme: &Theme, level: &OutputLevel) {
        const GAP: f32 = 6.0;
        let scale = self.scale.unwrap_or(crate::widgets::Scale::Unipolar);
        let meter = Margin {
            left: 12,
            right: 12,
            top: 10,
            bottom: 4,
        };
        section(ui, meter, |ui| {
            ui.spacing_mut().interact_size.y = 16.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (rect, _) = ui.allocate_exact_size(vec2(36.0, 16.0), Sense::hover());
                ui.painter().text(
                    rect.left_center(),
                    egui::Align2::LEFT_CENTER,
                    scale.label(),
                    egui::FontId::proportional(12.0),
                    theme.text_dim,
                );
                signal_meter(ui, scale, level.min, level.max, None);
            });
        });
        let numbers = Margin {
            left: 12,
            right: 12,
            top: 6,
            bottom: 10,
        };
        section(ui, numbers, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                let width = (ui.available_width() - 3.0 * GAP) / 4.0;
                for (name, value) in [
                    ("editor.tip.mean", level.mean),
                    ("editor.tip.min", level.min),
                    ("editor.tip.max", level.max),
                    ("editor.tip.rms", level.rms),
                ] {
                    let column = egui::Layout::top_down(egui::Align::Min);
                    ui.allocate_ui_with_layout(vec2(width, 0.0), column, |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        section_label(ui, tr(name));
                        ui.label(
                            egui::RichText::new(format!("{value:+.3}"))
                                .monospace()
                                .size(12.0)
                                .color(theme.surfaces.text),
                        );
                    });
                }
            });
        });
    }

    fn content(&self, ui: &mut Ui) {
        ui.set_min_width(TIP_WIDTH);
        if let Some(title) = &self.title {
            ui.label(egui::RichText::new(title).strong());
        }
        for fact in &self.facts {
            ui.label(egui::RichText::new(fact).weak());
        }
        if let Some(visual) = &self.visual {
            ui.add_space(2.0);
            show_view(ui, visual, self.mode, Theme::of(ui.ctx()).accent);
        }
        if self.level.is_some() || self.value.is_some() {
            if self.title.is_some() || !self.facts.is_empty() || self.visual.is_some() {
                ui.separator();
            }
            self.readings(ui);
        }
    }

    /// The meter and the numbers, in a grid so they stay in their columns.
    fn readings(&self, ui: &mut Ui) {
        egui::Grid::new("tip-readings")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                if let Some(value) = self.value {
                    row(ui, tr("editor.tip.value"), value);
                }
                if let Some(level) = &self.level {
                    let scale = self.scale.unwrap_or(crate::widgets::Scale::Unipolar);
                    ui.label(scale.label());
                    crate::widgets::signal_meter(
                        ui,
                        scale,
                        level.min,
                        level.max,
                        Some(METER_WIDTH),
                    );
                    ui.end_row();
                    row(ui, tr("editor.tip.mean"), level.mean);
                    row(ui, tr("editor.tip.min"), level.min);
                    row(ui, tr("editor.tip.max"), level.max);
                    row(ui, tr("editor.tip.rms"), level.rms);
                }
            });
    }
}

/// A part of the probe, `margin` inside its edges and as wide as the probe.
fn section(ui: &mut Ui, margin: Margin, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new().inner_margin(margin).show(ui, |ui| {
        ui.vertical(add);
    });
}

/// A line across the probe between its parts.
fn divider(ui: &mut Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    let stroke = egui::Stroke::new(1.0, theme.surfaces.card_border);
    ui.painter().hline(rect.x_range(), rect.center().y, stroke);
}

/// One end of a connection: the node and its port.
fn end(node: &str, port: &str) -> String {
    tr_args("editor.tip.end", &[("node", node), ("port", port)])
}

/// A grid row: the label, then the number in monospace with a fixed width and a sign.
fn row(ui: &mut Ui, label: &str, value: f32) {
    ui.label(label);
    ui.label(egui::RichText::new(number(value)).monospace());
    ui.end_row();
}

/// The lines describing a layout and its tag.
fn layout_lines(layout: &Layout) -> Vec<String> {
    let mut lines = vec![tr_args(
        "editor.tip.layout",
        &[
            ("layout", &layout.to_string()),
            ("samples", &layout.len().to_string()),
        ],
    )];
    let tag = layout.tag.to_string();
    if !tag.is_empty() {
        lines.push(tr_args("editor.tip.tag", &[("tag", &tag)]));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    #[test]
    fn a_point_on_the_wire_is_near_it_and_a_far_one_is_not() {
        let points = wire_points(pos2(0.0, 0.0), pos2(200.0, 100.0));
        // A cubic through these points is symmetric about its midpoint.
        let mid = pos2(100.0, 50.0);
        assert!(distance_to_curve(points, mid) < 1.0);
        assert!(distance_to_curve(points, pos2(100.0, 200.0)) > WIRE_GRAB);
    }
}
