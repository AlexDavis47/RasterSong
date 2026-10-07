//! Tooltips for wires and pins: what a signal is and what it is doing at the playhead.

use eframe::egui::{self, Pos2, Rect, Sense, Ui};
use rastersong_engine::{Layout, OutputLevel, ParamLevel};
use rastersong_lang::{tr, tr_args};

use super::canvas::{Geometry, Pin, wire_points};
use super::look::{LookContext, LookView};
use super::{GraphEditor, NodeKey, Wire, modulation};

/// How close (in screen pixels) the pointer must be to a wire for its tooltip.
const WIRE_GRAB: f32 = 6.0;
/// Points the wire's curve is sampled at to find the distance to it.
const WIRE_SAMPLES: usize = 32;

/// The levels the tooltips read, as the canvas was given them.
#[derive(Debug, Clone, Copy, Default)]
pub struct Readings<'a> {
    pub levels: &'a [OutputLevel],
    pub params: &'a [ParamLevel],
    /// The Look tool, while it is in use.
    pub look: Option<&'a LookContext<'a>>,
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
        tip.level = readings
            .levels
            .iter()
            .find(|l| *l.node == *node.id && l.output == output)
            .cloned();
        tip
    }

    /// Shows a tooltip for the wire or pin under the pointer, if there is one.
    pub(super) fn hover_tooltips(
        &self,
        ui: &mut Ui,
        rect: Rect,
        geometry: &[Geometry],
        to_screen: &dyn Fn(Pos2) -> Pos2,
        hovered_pin: Option<Pin>,
        readings: &Readings,
    ) {
        let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        if !rect.contains(pointer) || self.interaction != super::canvas::Interaction::Idle {
            return;
        }
        let over_node = geometry.iter().any(|g| {
            Rect::from_two_pos(to_screen(g.rect.min), to_screen(g.rect.max)).contains(pointer)
        });
        let mut tip = if let Some(pin) = hovered_pin {
            self.pin_tip(pin, readings)
        } else if over_node {
            return;
        } else if let Some(wire) = self.wire_near(geometry, to_screen, pointer) {
            self.wire_tip(wire, readings)
        } else {
            return;
        };
        if tip.is_empty() {
            return;
        }
        if let (Some(look), Some((node, output))) = (readings.look, &tip.source) {
            tip.look = Some(super::look::look(ui, node, *output, tip.audio, look));
        }
        let spot = Rect::from_center_size(pointer, egui::vec2(2.0, 2.0));
        ui.interact(spot, ui.id().with("canvas-tooltip"), Sense::hover())
            .on_hover_ui(|ui| tip.show(ui));
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
        tip.title = Some(tr_args(
            "editor.tip.wire",
            &[
                ("from", &self.name_of(wire.from.0)),
                ("out", &port_name(wire.from.0, wire.from.1, false)),
                ("to", &self.name_of(wire.to.0)),
                ("in", &port_name(wire.to.0, wire.to.1, true)),
            ],
        ));
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
    /// The node output the tip is about, and whether it carries audio.
    source: Option<(String, usize)>,
    audio: bool,
    /// The Look tool's picture or scope.
    look: Option<LookView>,
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
        ui.set_min_width(TIP_WIDTH);
        if let Some(title) = &self.title {
            ui.label(egui::RichText::new(title).strong());
        }
        for fact in &self.facts {
            ui.label(egui::RichText::new(fact).weak());
        }
        if let Some(look) = &self.look {
            ui.add_space(2.0);
            look.show(ui);
        }
        if self.level.is_none() && self.value.is_none() {
            return;
        }
        if self.title.is_some() || !self.facts.is_empty() || self.look.is_some() {
            ui.separator();
        }
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
                    crate::widgets::signal_meter(ui, scale, level.min, level.max, METER_WIDTH);
                    ui.end_row();
                    row(ui, tr("editor.tip.mean"), level.mean);
                    row(ui, tr("editor.tip.min"), level.min);
                    row(ui, tr("editor.tip.max"), level.max);
                    row(ui, tr("editor.tip.rms"), level.rms);
                }
            });
    }
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
