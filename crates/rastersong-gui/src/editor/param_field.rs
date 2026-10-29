//! The number field for node parameters: a slider track plus a value box.
//!
//! The track covers the parameter's usual range until told otherwise. Typing a value outside it
//! (anything up to the node's hard limits) widens the stored range for good, so the user is never
//! boxed in, Substance Designer style. The range can be set, or reset to the usual one, from the
//! track's right-click menu.
//!
//! A modulated parameter is set in a card: the span its signal covers is outlined over the track,
//! a ghost handle shows its live value, and under the track sit where the signal comes from, a
//! knob and a box for the amount, the direction, and how far the signal moves the value.
//!
//! Every control can be reset to its default with Alt+click or from its right-click menu.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Rect, Response, RichText, Sense, Stroke, Ui, Vec2, pos2,
    vec2,
};
use rastersong_engine::{MODULATION_AMOUNT_LIMITS, ModMode, Modulation, ParamSpec, range_span};
use rastersong_lang::{tr, tr_args};

use crate::theme::{CARD_RADIUS, SMALL_SIZE, Theme};
use crate::value_box::ValueBox;
use crate::widgets::{Icon, IconButton, rule, segmented, swatch};

/// A number parameter as the field shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberRange {
    /// The value Alt+click and "Reset to default" restore.
    pub default: f64,
    /// The usual range, which the track covers by default.
    pub soft: (f64, f64),
    /// The values the node accepts.
    pub limits: (f64, f64),
    /// Only whole numbers are offered: the handle jumps between them and typed values round.
    pub whole: bool,
}

impl NumberRange {}

/// Position `0..=1` of `value` along a track showing `range`.
pub fn to_fraction(value: f64, range: (f64, f64)) -> f64 {
    let (lo, hi) = range;
    if hi <= lo {
        return 0.0;
    }
    ((value - lo) / (hi - lo)).clamp(0.0, 1.0)
}

/// The value at position `t` (`0..=1`) along a track showing `range`, rounded to a precision that
/// suits the range so dragged values come out tidy.
pub fn from_fraction(t: f64, range: (f64, f64)) -> f64 {
    let (lo, hi) = range;
    let t = t.clamp(0.0, 1.0);
    let value = lo + (hi - lo) * t;
    let exponent = ((hi - lo) / 1000.0).log10().floor() as i32;
    round_to_power_of_ten(value, exponent).clamp(lo, hi)
}

/// `value` rounded to a multiple of `10^exponent`. Dividing by a whole power of ten for negative
/// exponents keeps results like 0.123 exact instead of 0.12300000000000001.
fn round_to_power_of_ten(value: f64, exponent: i32) -> f64 {
    if exponent < 0 {
        let scale = 10f64.powi(-exponent);
        (value * scale).round() / scale
    } else {
        let step = 10f64.powi(exponent);
        (value / step).round() * step
    }
}

/// A signal modulating the parameter.
#[derive(Debug)]
pub struct Modulated<'a> {
    pub spec: &'a ParamSpec,
    pub modulation: &'a mut Modulation,
    /// The connected wire's colour.
    pub color: Color32,
    /// The value at the playhead, from the last rendered frame.
    pub live: Option<f64>,
    /// Where the signal comes from: "Three-Band Split · low".
    pub source: &'a str,
}

/// What the user did with a field, and where its parts went.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldResponse {
    /// The value or the modulation changed.
    pub changed: bool,
    /// The user asked to disconnect the modulating signal.
    pub disconnect: bool,
    pub track: Rect,
    pub value_box: Rect,
    /// The amount knob of a modulated parameter.
    pub knob: Option<Rect>,
}

/// Width of the value box, so it doesn't resize as the digits change.
pub const VALUE_WIDTH: f32 = 64.0;
/// Height of the slider track.
const TRACK_HEIGHT: f32 = 24.0;
/// How far the rail is inset from each end of the track: half the handle.
pub const RAIL_INSET: f32 = 7.0;
/// Size of the amount knob.
const KNOB_SIZE: f32 = 28.0;
/// Width of the amount box.
const AMOUNT_WIDTH: f32 = 62.0;
/// Side of the small buttons in a parameter's rows: reset and disconnect.
pub const SMALL_BUTTON: f32 = 24.0;

/// Whether the user asked to reset a control: Alt+click on it.
pub fn alt_clicked(ui: &Ui, response: &Response) -> bool {
    response.clicked() && ui.input(|i| i.modifiers.alt)
}

/// Adds a right-click menu with "Reset to default" to `response` (greyed out when there's
/// nothing to reset). Returns true the frame the user picks it.
pub fn reset_menu(response: &Response, differs: bool) -> bool {
    let mut chosen = false;
    // Its own popup id: a combo box's popup is keyed on the same response, and sharing the id
    // made left clicks open nothing and right clicks open the combo.
    egui::Popup::context_menu(response)
        .id(response.id.with("reset-menu"))
        .show(|ui| {
            let reset = egui::Button::new(tr("editor.param.reset"))
                .shortcut_text(tr("editor.shortcut.alt_click"));
            if ui.add_enabled(differs, reset).clicked() {
                chosen = true;
                ui.close();
            }
        });
    chosen
}

/// Both ways to reset a control: Alt+click, and its right-click menu.
pub fn reset_gesture(ui: &Ui, response: &Response, differs: bool) -> bool {
    alt_clicked(ui, response) | reset_menu(response, differs)
}

/// The button that resets parameter `name` to `default` (as text), greyed out when it's there
/// already. Returns true when clicked.
pub fn reset_button(ui: &mut Ui, name: &str, default: &str, differs: bool) -> bool {
    let label = tr_args(
        "editor.param.reset_named",
        &[("param", name), ("value", default)],
    );
    ui.add_enabled(
        differs,
        IconButton::new(Icon::Reset, &label).size(Vec2::splat(SMALL_BUTTON)),
    )
    .on_hover_text(&label)
    .clicked()
}

/// A number as the slider's ends show it: up to two decimals, without trailing zeros.
pub fn short_number(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Shows the field, top-down: `leading` (the pin toggle and name) with `trailing` (the int
/// toggle), the value box and the reset button on its right; then the slider track across. A
/// modulated parameter is set in a card, and goes on with the slider's ends and the live value,
/// where the signal comes from, the amount knob and box with the direction, and how far the
/// signal moves the value. `name` names the reset button. Every part has a fixed size or takes
/// the width it is given, so nothing it shows can widen the panel holding it.
#[allow(clippy::too_many_arguments)]
pub fn param_field(
    ui: &mut Ui,
    id_salt: &str,
    name: &str,
    value: &mut f64,
    range: NumberRange,
    mut modulated: Option<Modulated>,
    custom: &mut Option<(f64, f64)>,
    leading: impl FnOnce(&mut Ui),
    trailing: impl FnOnce(&mut Ui),
) -> FieldResponse {
    let theme = Theme::of(ui.ctx());
    let frame = if modulated.is_some() {
        egui::Frame::new()
            .fill(theme.surfaces.inner_card)
            .stroke(Stroke::new(1.0, theme.surfaces.border))
            .corner_radius(CARD_RADIUS)
            .inner_margin(10)
    } else {
        egui::Frame::NONE
    };
    frame
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            field(
                ui,
                id_salt,
                name,
                value,
                range,
                modulated.as_mut(),
                custom,
                leading,
                trailing,
            )
        })
        .inner
}

#[allow(clippy::too_many_arguments)]
fn field(
    ui: &mut Ui,
    id_salt: &str,
    name: &str,
    value: &mut f64,
    range: NumberRange,
    mut modulated: Option<&mut Modulated>,
    custom: &mut Option<(f64, f64)>,
    leading: impl FnOnce(&mut Ui),
    trailing: impl FnOnce(&mut Ui),
) -> FieldResponse {
    let id = ui.make_persistent_id(id_salt);
    // The track's range is the node's to keep (`custom`, saved in the graph, where modulation
    // is held to it). It starts as the usual range, grows to include any value typed beyond
    // it, and can be set or reset from the track's right-click menu.
    let mut shown = custom.unwrap_or(range.soft);
    if *value < shown.0 || *value > shown.1 {
        let v = value.clamp(range.limits.0, range.limits.1);
        shown = (shown.0.min(v), shown.1.max(v));
        *custom = Some(shown);
    }
    let before = (*value, modulated.as_ref().map(|m| *m.modulation));
    let height = ui.spacing().interact_size.y;

    let mut value_box = Rect::NOTHING;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        leading(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let default = short_number(range.default);
            if reset_button(ui, name, &default, *value != range.default) {
                *value = range.default;
            }
            let speed = (shown.1 - shown.0) / 300.0;
            let response = ui
                .add(
                    ValueBox::new(value)
                        .range(range.limits.0..=range.limits.1)
                        .speed(speed)
                        .max_decimals(if range.whole { 0 } else { 3 })
                        .whole(range.whole)
                        .size(vec2(VALUE_WIDTH, height)),
                )
                .on_hover_text(tr("editor.param.value_help"));
            if reset_gesture(ui, &response, *value != range.default) {
                *value = range.default;
                response.surrender_focus();
            }
            value_box = response.rect;
            trailing(ui);
        });
    });

    let size = vec2(ui.available_width(), TRACK_HEIGHT);
    let (rect, track) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let track = track.on_hover_text(tr("editor.param.track_help"));
    if alt_clicked(ui, &track) {
        *value = range.default;
    }
    let mut new_range = shown;
    let mut reset_range = false;
    let mut reset_value = false;
    egui::Popup::context_menu(&track)
        .id(id.with("track-menu"))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(170.0);
            let reset = egui::Button::new(tr("editor.param.reset"))
                .shortcut_text(tr("editor.shortcut.alt_click"));
            if ui.add_enabled(*value != range.default, reset).clicked() {
                reset_value = true;
                ui.close();
            }
            ui.separator();
            ui.label(tr("editor.param.slider_range"));
            ui.horizontal(|ui| {
                ui.label(tr("editor.param.min"));
                ui.add(
                    ValueBox::new(&mut new_range.0)
                        .range(range.limits.0..=range.limits.1)
                        .speed(((shown.1 - shown.0) / 300.0).max(1e-6))
                        .max_decimals(3),
                );
                ui.label(tr("editor.param.max"));
                ui.add(
                    ValueBox::new(&mut new_range.1)
                        .range(range.limits.0..=range.limits.1)
                        .speed(((shown.1 - shown.0) / 300.0).max(1e-6))
                        .max_decimals(3),
                );
            });
            if ui
                .add_enabled(
                    shown != range.soft,
                    egui::Button::new(tr("editor.param.reset_range")),
                )
                .clicked()
            {
                reset_range = true;
                ui.close();
            }
        });
    if reset_value {
        *value = range.default;
    }
    if reset_range {
        *custom = None;
        shown = range.soft;
    } else if new_range != shown && new_range.0 < new_range.1 {
        shown = new_range;
        *value = value.clamp(shown.0, shown.1);
        *custom = Some(shown);
    }
    // The handle runs along the rail, inset from the track's ends; the value under the pointer is
    // the one whose handle is drawn there.
    let rail = Rect::from_center_size(rect.center(), vec2(rect.width() - 2.0 * RAIL_INSET, 4.0));
    if (track.clicked() || track.dragged())
        && !ui.input(|i| i.modifiers.alt)
        && let Some(p) = track.interact_pointer_pos()
    {
        let t = f64::from((p.x - rail.left()) / rail.width());
        *value = from_fraction(t, shown);
        if range.whole {
            *value = value.round().clamp(range.limits.0, range.limits.1);
        }
    }

    let fraction = |v: f64| to_fraction(v, shown) as f32;
    let x = |v: f64| rail.left() + rail.width() * fraction(v);
    let mut response = FieldResponse {
        changed: false,
        disconnect: false,
        track: rect,
        value_box,
        knob: None,
    };
    match &modulated {
        Some(m) => {
            paint_rail(ui, rail, None);
            let (lo, hi) = m.spec.modulated_range(*value, *m.modulation, shown);
            paint_range(ui, rail, x(lo), x(hi), m.color);
        }
        None => paint_rail(ui, rail, Some(x(*value))),
    }
    paint_handle(ui, &track, pos2(x(*value), rect.center().y));
    if let Some((live, color)) = modulated
        .as_ref()
        .and_then(|m| m.live.map(|v| (v, m.color)))
    {
        // Fading copies behind it, filled in between frames so a jump reads as a smear.
        // Positions are tracked as fractions of the rail, so the smear is even on screen.
        let gap = f64::from(SMEAR_PX / rail.width().max(1.0));
        let at = fraction(live);
        for (f, visibility) in crate::effects::trail(ui, id.with("ghost"), f64::from(at), gap) {
            let gx = rail.left() + rail.width() * f as f32;
            paint_ghost(
                ui,
                pos2(gx, rect.center().y),
                color.gamma_multiply(visibility * 0.1),
            );
        }
        paint_ghost(ui, pos2(x(live), rect.center().y), color);
    }

    if let Some(m) = modulated.as_mut() {
        modulation_rows(ui, m, shown, &mut response);
    }
    response.changed = before != (*value, modulated.as_ref().map(|m| *m.modulation));
    response
}

/// Under a modulated parameter's slider: its ends and the live value, then, past a rule, where
/// the signal comes from, the amount knob and box with the direction, and how far a full signal
/// moves the value.
fn modulation_rows(
    ui: &mut Ui,
    m: &mut Modulated,
    range: (f64, f64),
    response: &mut FieldResponse,
) {
    let theme = Theme::of(ui.ctx());
    let (ends, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
    let font = egui::FontId::monospace(SMALL_SIZE);
    let color = theme.surfaces.text_faint;
    let painter = ui.painter();
    let left = ends.left_center() + vec2(RAIL_INSET, 0.0);
    let right = ends.right_center() - vec2(RAIL_INSET, 0.0);
    painter.text(
        left,
        Align2::LEFT_CENTER,
        short_number(range.0),
        font.clone(),
        color,
    );
    painter.text(
        right,
        Align2::RIGHT_CENTER,
        short_number(range.1),
        font.clone(),
        color,
    );
    if let Some(live) = m.live {
        let text = tr_args(
            "editor.modulation.live",
            &[("value", &format!("{live:.2}"))],
        );
        painter.text(ends.center(), Align2::CENTER_CENTER, text, font, color);
    }
    rule(ui, theme.surfaces.border);
    // The board pads the rows under the rule by 8, a little more than the card's spacing.
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        swatch(ui, 14.0, m.color);
        ui.label(
            RichText::new(tr("editor.modulation.from"))
                .size(12.0)
                .color(theme.text_dim),
        );
        ui.label(RichText::new(m.source).font(crate::theme::medium(12.0)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = tr("editor.modulation.disconnect");
            if ui
                .add(IconButton::new(Icon::Close, label).size(Vec2::splat(SMALL_BUTTON)))
                .on_hover_text(label)
                .clicked()
            {
                response.disconnect = true;
            }
        });
    });

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        response.knob = Some(amount_knob(ui, m, range));
        let default_amount = m.spec.default_modulation_amount();
        let amount = ui
            .add(
                ValueBox::new(&mut m.modulation.amount)
                    .range(AMOUNT_LIMITS.0..=AMOUNT_LIMITS.1)
                    .speed(0.3)
                    .suffix(" %")
                    .max_decimals(1)
                    .size(vec2(AMOUNT_WIDTH, ui.spacing().interact_size.y)),
            )
            .on_hover_text(tr("editor.modulation.amount.help"));
        if reset_gesture(ui, &amount, m.modulation.amount != default_amount) {
            m.modulation.amount = default_amount;
            amount.surrender_focus();
        }
        let modes = [
            (
                tr("editor.modulation.one_way"),
                tr("editor.modulation.one_way.help"),
            ),
            (
                tr("editor.modulation.both_ways"),
                tr("editor.modulation.both_ways.help"),
            ),
        ];
        let on = usize::from(m.modulation.mode == ModMode::Bipolar);
        match segmented(ui, &modes, on) {
            Some(0) => m.modulation.mode = ModMode::Unipolar,
            Some(_) => m.modulation.mode = ModMode::Bipolar,
            None => {}
        }
    });

    // The same amount in the parameter's own terms, for when you know the distance you want.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let words = |text: &str, color| RichText::new(text).size(11.5).color(color);
        ui.label(words(
            match m.modulation.mode {
                ModMode::Bipolar => tr("editor.modulation.either_side"),
                ModMode::Unipolar => tr("editor.modulation.moves_by"),
            },
            theme.text_dim,
        ));
        let unit = if m.spec.unit.is_empty() {
            String::new()
        } else {
            format!(" {}", m.spec.unit)
        };
        let mut sweep = m.spec.modulation_sweep(*m.modulation, range);
        let span = range_span(range);
        if ui
            .add(
                ValueBox::new(&mut sweep)
                    .range(-span..=span)
                    .speed(span / 300.0)
                    .suffix(&unit)
                    .max_decimals(3),
            )
            .on_hover_text(tr("editor.modulation.sweep.help"))
            .changed()
        {
            m.modulation.amount = m.spec.modulation_amount_for_sweep(sweep, range);
        }
        ui.label(words(
            tr("editor.modulation.held"),
            theme.surfaces.text_faint,
        ));
    });
}

/// The amounts the knob and both boxes accept, in percent of the slider's range.
const AMOUNT_LIMITS: (f64, f64) = MODULATION_AMOUNT_LIMITS;

/// The amount a full turn of the knob stands for, in percent: the whole span.
const KNOB_PERCENT: f64 = 100.0;

/// The sign a modulation's amount is shown with: `±` both ways, `+` one way up, nothing for a
/// negative amount, which carries its own minus.
fn amount_sign(modulation: Modulation) -> &'static str {
    match modulation.mode {
        ModMode::Bipolar => "±",
        ModMode::Unipolar if modulation.amount >= 0.0 => "+",
        ModMode::Unipolar => "",
    }
}

/// The amount as text: a percentage of the parameter's span, `±25%` both ways, `+25%` or `-25%`
/// one way.
pub fn amount_text(modulation: Modulation) -> String {
    format!("{}{:.1}%", amount_sign(modulation), modulation.amount).replace(".0%", "%")
}

/// What the amount comes to in the parameter's own terms, for a tooltip: how far a full signal
/// moves the value, `±3.3 bits` or `+2.5 Hz`. Both ways, that is either side of the value.
pub fn amount_effect(spec: &ParamSpec, modulation: Modulation, range: (f64, f64)) -> String {
    let sweep = spec.modulation_sweep(modulation, range);
    let unit = spec.unit;
    let space = if unit.is_empty() { "" } else { " " };
    let value = format!("{:.2}", sweep.abs()).replace(".00", "");
    let value = value.trim_end_matches('0').trim_end_matches('.');
    let sign = match modulation.mode {
        ModMode::Bipolar => "±",
        ModMode::Unipolar if sweep >= 0.0 => "+",
        ModMode::Unipolar => "-",
    };
    format!("{sign}{value}{space}{unit}")
}

/// The amount knob: an arc showing the amount in the wire's colour. Drag to change it,
/// double-click or Alt+click to reset it. Returns where it went.
fn amount_knob(ui: &mut Ui, m: &mut Modulated, range: (f64, f64)) -> Rect {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(KNOB_SIZE), Sense::click_and_drag());
    let modulation = &mut *m.modulation;
    if response.dragged_by(egui::PointerButton::Primary) {
        // Right and up turn it up. Shift for fine control.
        let delta = response.drag_delta();
        let fine = if ui.input(|i| i.modifiers.shift) {
            0.1
        } else {
            1.0
        };
        modulation.amount = (modulation.amount
            + f64::from(delta.x - delta.y) / 150.0 * KNOB_PERCENT * fine)
            .clamp(AMOUNT_LIMITS.0, AMOUNT_LIMITS.1);
    }
    let default_amount = m.spec.default_modulation_amount();
    if response.double_clicked() || alt_clicked(ui, &response) {
        modulation.amount = default_amount;
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }

    // The knob: a well with a ring, an arc from the top for the amount (both sides when
    // bipolar), and a pointer.
    let theme = Theme::of(ui.ctx());
    let painter = ui.painter();
    let center = rect.center();
    let radius = KNOB_SIZE / 2.0 - 3.0;
    let ring = if response.hovered() || response.dragged() {
        theme.surfaces.border_hover
    } else {
        theme.node_outline
    };
    painter.circle(
        center,
        radius,
        theme.surfaces.ground,
        Stroke::new(2.0, ring),
    );
    let turn = (modulation.amount / KNOB_PERCENT).clamp(-1.0, 1.0) as f32 * KNOB_SWEEP;
    let arc = |from: f32, to: f32| {
        let steps = 24;
        (0..=steps)
            .map(|k| {
                let a = from + (to - from) * k as f32 / steps as f32;
                // Angle 0 is straight up, clockwise positive.
                center + radius * vec2(a.sin(), -a.cos())
            })
            .collect::<Vec<_>>()
    };
    let stroke = Stroke::new(2.5, m.color);
    let points = match modulation.mode {
        ModMode::Bipolar => arc(-turn.abs(), turn.abs()),
        ModMode::Unipolar => arc(0.0, turn),
    };
    painter.add(egui::Shape::line(points, stroke));
    painter.line_segment(
        [
            center,
            center + (radius - 1.0) * vec2(turn.sin(), -turn.cos()),
        ],
        Stroke::new(2.0, theme.surfaces.text),
    );

    response.on_hover_text(tr_args(
        "editor.modulation.knob_help",
        &[
            ("amount", &amount_text(*modulation)),
            ("effect", &amount_effect(m.spec, *modulation, range)),
        ],
    ));
    rect
}

/// Screen distance between the interpolated copies of a ghost handle.
const SMEAR_PX: f32 = 2.0;

/// How far the knob's arc turns at full amount, either side of the top (radians).
const KNOB_SWEEP: f32 = 2.4;

/// Radius of the slider's handle.
const HANDLE_RADIUS: f32 = 7.0;

/// The rail, filled in the accent up to `value_x` when given.
fn paint_rail(ui: &Ui, rail: Rect, value_x: Option<f32>) {
    let theme = Theme::of(ui.ctx());
    let painter = ui.painter();
    painter.rect_filled(rail, CornerRadius::same(2), theme.surfaces.border);
    if let Some(x) = value_x {
        painter.rect_filled(
            Rect::from_min_max(rail.min, pos2(x, rail.max.y)),
            CornerRadius::same(2),
            theme.accent.gamma_multiply(0.8),
        );
    }
}

/// The span a modulating signal covers: a band over the rail in the wire's colour.
fn paint_range(ui: &Ui, rail: Rect, lo: f32, hi: f32, color: Color32) {
    let band = Rect::from_min_max(pos2(lo, rail.top() - 1.0), pos2(hi, rail.bottom() + 1.0));
    ui.painter().rect(
        band,
        CornerRadius::same(3),
        color.gamma_multiply(0.35),
        Stroke::new(1.0, color),
        egui::StrokeKind::Inside,
    );
}

/// The handle: light, with a dark edge and a thin ring that turns the accent under the pointer.
fn paint_handle(ui: &Ui, response: &egui::Response, at: egui::Pos2) {
    let theme = Theme::of(ui.ctx());
    let ring = if response.hovered() || response.dragged() {
        theme.accent
    } else {
        theme.tick
    };
    let painter = ui.painter();
    painter.circle_filled(at, HANDLE_RADIUS + 1.0, ring);
    painter.circle(
        at,
        HANDLE_RADIUS,
        theme.surfaces.text,
        Stroke::new(2.0, theme.surfaces.ground),
    );
}

/// The live value: a see-through handle in the wire's colour, over the real one.
fn paint_ghost(ui: &Ui, at: egui::Pos2, color: Color32) {
    ui.painter().circle(
        at,
        HANDLE_RADIUS,
        color.gamma_multiply(0.45),
        Stroke::new(1.0, color),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUTOFF: NumberRange = NumberRange {
        default: 40.0,
        soft: (0.01, 100_000.0),
        limits: (1e-6, 1e9),
        whole: false,
    };
    const DEPTH: NumberRange = NumberRange {
        default: 0.0,
        soft: (-10.0, 10.0),
        limits: (f64::NEG_INFINITY, f64::INFINITY),
        whole: false,
    };

    #[test]
    fn positions_and_values_map_both_ways() {
        let range = DEPTH.soft;
        assert_eq!(to_fraction(0.0, range), 0.5);
        assert_eq!(from_fraction(0.75, range), 5.0);
        assert_eq!(from_fraction(2.0, range), 10.0, "clamped");

        let range = CUTOFF.soft;
        let middle = from_fraction(0.5, range);
        assert!((middle - 50_000.0).abs() < 50.0, "{middle}");
        assert!((to_fraction(middle, range) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn amounts_read_with_their_direction_and_unit() {
        use rastersong_engine::{ModMode, Modulation, ParamSpec};
        let feedback = ParamSpec::number("feedback", 0.0, 0.0, 1.0);
        let bits = ParamSpec::number("bits", 4.0, 1.0, 24.0).unit("bits");
        let m = |amount, mode| Modulation { amount, mode };
        assert_eq!(amount_text(m(25.0, ModMode::Bipolar)), "±25%");
        assert_eq!(amount_text(m(-12.5, ModMode::Unipolar)), "-12.5%");
        assert_eq!(amount_text(m(100.0, ModMode::Unipolar)), "+100%");
        // One rule in both modes: the amount is how far a full signal moves the value, as a share
        // of the range.
        assert_eq!(
            amount_effect(&feedback, m(50.0, ModMode::Bipolar), (0.0, 1.0)),
            "±0.5"
        );
        assert_eq!(
            amount_effect(&feedback, m(-50.0, ModMode::Unipolar), (0.0, 1.0)),
            "-0.5"
        );
        assert_eq!(
            amount_effect(&bits, m(10.0, ModMode::Unipolar), (1.0, 24.0)),
            "+2.3 bits"
        );
        // The slider's own range, not the node's usual one, is what the percentage is of.
        assert_eq!(
            amount_effect(&bits, m(50.0, ModMode::Bipolar), (1.0, 5.0)),
            "±2 bits"
        );
    }

    /// The amount knob is the percentage box: dragged far either way it stops at the ends.
    #[test]
    fn the_knob_stops_at_the_ends_of_the_percentage() {
        use eframe::egui::{Event, Modifiers, PointerButton, pos2};
        use egui_kittest::Harness;
        use rastersong_engine::{ModMode, Modulation, ParamSpec};

        let spec = ParamSpec::number("time", 1.0, 0.0, 10.0);
        let state = (
            5.0f64,
            Modulation {
                amount: 25.0,
                mode: ModMode::Unipolar,
            },
            None::<(f64, f64)>,
        );
        let knob = std::rc::Rc::new(std::cell::Cell::new(Rect::NOTHING));
        let seen = knob.clone();
        let mut harness = Harness::new_ui_state(
            move |ui, (value, modulation, custom)| {
                // The app's fonts are only there from the frame after they're set.
                if ui.ctx().cumulative_frame_nr() == 0 {
                    crate::theme::apply_style(ui.ctx());
                    return;
                }
                let range = NumberRange {
                    default: 1.0,
                    soft: (0.0, 10.0),
                    limits: (0.0, 10.0),
                    whole: false,
                };
                let modulated = Modulated {
                    spec: &spec,
                    modulation,
                    color: Color32::LIGHT_BLUE,
                    live: None,
                    source: "LFO · out",
                };
                let field = param_field(
                    ui,
                    "t",
                    "time",
                    value,
                    range,
                    Some(modulated),
                    custom,
                    |_| {},
                    |_| {},
                );
                seen.set(field.knob.unwrap());
            },
            state,
        );
        harness.run();
        let knob = knob.get().center();
        let drag = |harness: &mut Harness<'_, _>, dy: f32| {
            harness.event(Event::PointerMoved(knob));
            harness.run_steps(1);
            harness.event(Event::PointerButton {
                pos: knob,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            });
            harness.run_steps(1);
            for step in 1..=10 {
                harness.event(Event::PointerMoved(pos2(
                    knob.x,
                    knob.y + dy * step as f32 / 10.0,
                )));
                harness.run_steps(1);
            }
            harness.event(Event::PointerButton {
                pos: knob,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            });
            harness.run_steps(1);
        };
        drag(&mut harness, -2000.0);
        assert_eq!(harness.state().1.amount, 100.0, "up stops at 100%");
        drag(&mut harness, 8000.0);
        assert_eq!(harness.state().1.amount, -100.0, "down stops at -100%");
    }

    #[test]
    fn dragged_values_are_tidy() {
        let value = from_fraction(0.123_456, (0.0, 1.0));
        assert_eq!(value, 0.123);
        let value = from_fraction(0.123_456, (0.0, 1000.0));
        assert_eq!(value, 123.0);
    }
}
