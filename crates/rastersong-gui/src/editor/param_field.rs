//! The number field for node parameters: a slider track plus a value box.
//!
//! The track covers the parameter's usual range until told otherwise. Typing a value outside it
//! (anything up to the node's hard limits) widens the stored range for good, so the user is never
//! boxed in, Substance Designer style. The range can be set, or reset to the usual one, from the
//! track's right-click menu.
//!
//! A modulated parameter also shows the span its signal covers (outlined over the track), a
//! ghost handle at its live value, and a knob for the modulation amount. The knob sits in the
//! gutter left of the track, under the parameter's expose toggle; unmodulated fields keep the
//! gutter empty, so every track and value box lines up whatever is connected.
//!
//! Every control can be reset to its default with Alt+click or from its right-click menu.

use eframe::egui::{self, Color32, CornerRadius, Rect, Response, Sense, Stroke, Ui, pos2, vec2};
use rastersong_engine::{ModMode, ModScale, Modulation, ParamKind, ParamSpec};

use crate::theme::Theme;
use crate::value_box::ValueBox;

/// A number parameter as the field shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberRange {
    /// The value Alt+click and "Reset to default" restore.
    pub default: f64,
    /// The usual range, which the track covers by default.
    pub soft: (f64, f64),
    /// The values the node accepts.
    pub limits: (f64, f64),
}

impl NumberRange {}

/// Whether a track showing `range` maps logarithmically.
fn is_logarithmic(range: (f64, f64)) -> bool {
    range.0 > 0.0 && range.1 / range.0 >= 1000.0
}

/// Position `0..=1` of `value` along a track showing `range`.
pub fn to_fraction(value: f64, range: (f64, f64), logarithmic: bool) -> f64 {
    let (lo, hi) = range;
    if hi <= lo {
        return 0.0;
    }
    let t = if logarithmic && lo > 0.0 {
        (value.max(lo) / lo).ln() / (hi / lo).ln()
    } else {
        (value - lo) / (hi - lo)
    };
    t.clamp(0.0, 1.0)
}

/// The value at position `t` (`0..=1`) along a track showing `range`, rounded to a precision that
/// suits the range so dragged values come out tidy.
pub fn from_fraction(t: f64, range: (f64, f64), logarithmic: bool) -> f64 {
    let (lo, hi) = range;
    let t = t.clamp(0.0, 1.0);
    if logarithmic && lo > 0.0 {
        let value = lo * (hi / lo).powf(t);
        // Three significant digits.
        round_to_power_of_ten(value, value.log10().floor() as i32 - 2)
    } else {
        let value = lo + (hi - lo) * t;
        let exponent = ((hi - lo) / 1000.0).log10().floor() as i32;
        round_to_power_of_ten(value, exponent).clamp(lo, hi)
    }
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
}

/// What the user did with a field.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FieldResponse {
    /// The value or the modulation changed.
    pub changed: bool,
    /// The user asked to disconnect the modulating signal.
    pub disconnect: bool,
}

/// Width of the gutter left of the controls: the amount knob of a modulated parameter, or the
/// space kept for one, and the parameter's expose toggle above it.
pub const GUTTER_WIDTH: f32 = 22.0;

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
            let reset = egui::Button::new("Reset to default").shortcut_text("Alt+click");
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

/// Shows the field: the gutter (holding the amount knob of a modulated parameter), a slider
/// track and a value box. Every part has a fixed width, so the field is always
/// `GUTTER_WIDTH + track_width + value_width` plus spacing wide, whatever it shows; a field that
/// grew with its text would widen the panel holding it, which widens the field again.
pub fn param_field(
    ui: &mut Ui,
    id_salt: &str,
    value: &mut f64,
    range: NumberRange,
    track_width: f32,
    value_width: f32,
    mut modulated: Option<Modulated>,
) -> FieldResponse {
    let id = ui.make_persistent_id(id_salt);
    // The track's range is stored per field. It starts as the usual range, grows to include any
    // value typed beyond it, and can be set or reset from the track's right-click menu.
    let range_id = id.with("range");
    let custom: Option<(f64, f64)> = ui.data_mut(|d| d.get_persisted(range_id));
    let mut shown = custom.unwrap_or(range.soft);
    if *value < shown.0 || *value > shown.1 {
        let v = value.clamp(range.limits.0, range.limits.1);
        shown = (shown.0.min(v), shown.1.max(v));
        ui.data_mut(|d| d.insert_persisted(range_id, shown));
    }
    let before = (*value, modulated.as_ref().map(|m| *m.modulation));
    let mut response = FieldResponse::default();

    match modulated.as_mut() {
        Some(m) => {
            let knob = amount_knob(ui, m, *value, shown);
            response.disconnect = knob.disconnect;
        }
        None => {
            // Allocated, not just added as space, so it's followed by the same item spacing.
            ui.allocate_space(vec2(GUTTER_WIDTH, ui.spacing().interact_size.y));
        }
    }

    let height = ui.spacing().interact_size.y;
    let (rect, track) = ui.allocate_exact_size(vec2(track_width, height), Sense::click_and_drag());
    let track = track
        .on_hover_text("Click or drag to set. Alt+click or right-click to reset to the default.");
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
            let reset = egui::Button::new("Reset to default").shortcut_text("Alt+click");
            if ui.add_enabled(*value != range.default, reset).clicked() {
                reset_value = true;
                ui.close();
            }
            ui.separator();
            ui.label("Slider range");
            ui.horizontal(|ui| {
                ui.label("Min");
                ui.add(
                    ValueBox::new(&mut new_range.0)
                        .range(range.limits.0..=range.limits.1)
                        .speed(((shown.1 - shown.0) / 300.0).max(1e-6))
                        .max_decimals(3),
                );
                ui.label("Max");
                ui.add(
                    ValueBox::new(&mut new_range.1)
                        .range(range.limits.0..=range.limits.1)
                        .speed(((shown.1 - shown.0) / 300.0).max(1e-6))
                        .max_decimals(3),
                );
            });
            if ui
                .add_enabled(shown != range.soft, egui::Button::new("Reset range"))
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
        ui.data_mut(|d| d.remove::<(f64, f64)>(range_id));
        shown = range.soft;
    } else if new_range != shown && new_range.0 < new_range.1 {
        shown = new_range;
        *value = value.clamp(shown.0, shown.1);
        ui.data_mut(|d| d.insert_persisted(range_id, shown));
    }
    let logarithmic = is_logarithmic(shown);
    if (track.clicked() || track.dragged())
        && !ui.input(|i| i.modifiers.alt)
        && let Some(p) = track.interact_pointer_pos()
    {
        let t = f64::from((p.x - rect.left()) / rect.width());
        *value = from_fraction(t, shown, logarithmic);
    }

    let fraction = |v: f64| to_fraction(v, shown, logarithmic) as f32;
    let rail = Rect::from_center_size(rect.center(), vec2(rect.width() - 8.0, 4.0));
    let x = |v: f64| rail.left() + rail.width() * fraction(v);
    paint_rail(ui, rail, x(*value));
    if let Some(m) = &modulated {
        let (lo, hi) = m.spec.modulated_range(*value, *m.modulation);
        paint_range(ui, rail, x(lo), x(hi), m.color);
    }
    paint_handle(ui, &track, pos2(x(*value), rect.center().y), rect.height());
    if let Some(live) = modulated
        .as_ref()
        .and_then(|m| m.live.map(|v| (v, m.color)))
    {
        // Fading copies behind it, filled in between frames so a jump reads as a smear.
        // Positions are tracked as fractions of the rail, so the smear is even on screen.
        let gap = f64::from(SMEAR_PX / rail.width().max(1.0));
        let at = fraction(live.0);
        for (f, visibility) in crate::effects::trail(ui, id.with("ghost"), f64::from(at), gap) {
            let gx = rail.left() + rail.width() * f as f32;
            paint_ghost(
                ui,
                pos2(gx, rect.center().y),
                rect.height(),
                live.1.gamma_multiply(visibility * 0.1),
            );
        }
        paint_ghost(ui, pos2(x(live.0), rect.center().y), rect.height(), live.1);
    }

    let speed = if logarithmic {
        value.abs().max(range.soft.0) * 0.01
    } else {
        (shown.1 - shown.0) / 300.0
    };
    let value_box = ui
        .add(
            ValueBox::new(value)
                .range(range.limits.0..=range.limits.1)
                .speed(speed)
                .max_decimals(3)
                .size(vec2(value_width, height)),
        )
        .on_hover_text(
            "Drag, or click to type. Values beyond the slider are allowed. \
             Alt+click or right-click to reset.",
        );
    if reset_gesture(ui, &value_box, *value != range.default) {
        *value = range.default;
        value_box.surrender_focus();
    }
    response.changed = before != (*value, modulated.as_ref().map(|m| *m.modulation));
    response
}

/// How far the knob turns for a full amount: the usual range for linear parameters, this many
/// octaves for octave-scaled ones.
const KNOB_OCTAVES: f64 = 4.0;

/// The amount a full turn of the knob stands for: the field's slider range, so the knob is as
/// fine or coarse as the slider.
fn knob_span(spec: &ParamSpec, track: (f64, f64)) -> f64 {
    match (spec.scale, spec.kind) {
        (ModScale::Octaves, _) => KNOB_OCTAVES,
        (_, ParamKind::Number { .. }) => (track.1 - track.0).max(1e-9),
        _ => 1.0,
    }
}

/// The amount as text: `±0.5` both ways, `+0.5` or `-0.5` one way, in octaves where they apply.
pub fn amount_text(spec: &ParamSpec, modulation: Modulation) -> String {
    let unit = match spec.scale {
        ModScale::Octaves => " oct",
        ModScale::Linear if spec.unit.is_empty() => "",
        ModScale::Linear => spec.unit,
    };
    let sign = match modulation.mode {
        ModMode::Bipolar => "±",
        ModMode::Unipolar if modulation.amount >= 0.0 => "+",
        ModMode::Unipolar => "",
    };
    let space = if unit.is_empty() || unit == " oct" {
        ""
    } else {
        " "
    };
    format!("{sign}{:.3}{space}{unit}", modulation.amount).replace(".000", "")
}

#[derive(Debug, Default)]
struct KnobResponse {
    disconnect: bool,
}

/// The amount knob: an arc showing the amount in the wire's colour. Drag to change it,
/// double-click to reset it, right-click for the modulator's settings.
fn amount_knob(ui: &mut Ui, m: &mut Modulated, base: f64, track: (f64, f64)) -> KnobResponse {
    let mut result = KnobResponse::default();
    let size = vec2(GUTTER_WIDTH, ui.spacing().interact_size.y);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let span = knob_span(m.spec, track);
    let modulation = &mut *m.modulation;
    if response.dragged_by(egui::PointerButton::Primary) {
        // Right and up turn it up. Shift for fine control.
        let delta = response.drag_delta();
        let fine = if ui.input(|i| i.modifiers.shift) {
            0.1
        } else {
            1.0
        };
        modulation.amount += f64::from(delta.x - delta.y) / 150.0 * span * fine;
        if modulation.mode == ModMode::Bipolar {
            modulation.amount = modulation.amount.max(0.0);
        }
    }
    let default_amount = m.spec.default_modulation_amount(base);
    if response.double_clicked() || alt_clicked(ui, &response) {
        modulation.amount = default_amount;
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }

    // The knob: a ring, and an arc from the top for the amount (both sides when bipolar).
    let painter = ui.painter();
    let center = rect.center();
    let radius = (rect.height().min(rect.width()) / 2.0 - 2.0).max(4.0);
    let visuals = ui.style().interact(&response);
    painter.circle(
        center,
        radius,
        ui.visuals().widgets.inactive.bg_fill,
        Stroke::new(1.0, visuals.bg_stroke.color),
    );
    let turn = (modulation.amount / span).clamp(-1.0, 1.0) as f32 * KNOB_SWEEP;
    let arc = |from: f32, to: f32| {
        let steps = 16;
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
            center + radius * 0.6 * vec2(turn.sin(), -turn.cos()),
        ],
        Stroke::new(1.5, visuals.fg_stroke.color),
    );

    let response = response.on_hover_text(format!(
        "Modulation {}. Drag to change, double-click or Alt+click to reset, right-click for options.",
        amount_text(m.spec, *modulation)
    ));
    // Clicks inside don't close it, so its fields can be typed into.
    let menu = egui::Popup::context_menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    menu.show(|ui| {
        ui.set_min_width(190.0);
        ui.label(egui::RichText::new("Modulation").strong());
        ui.separator();
        let both = modulation.mode == ModMode::Bipolar;
        if ui
            .radio(both, "Both ways")
            .on_hover_text("The signal moves the value up and down around it")
            .clicked()
        {
            modulation.mode = ModMode::Bipolar;
            modulation.amount = modulation.amount.abs();
        }
        if ui
            .radio(!both, "One way")
            .on_hover_text(
                "The signal's strength moves the value one way; a negative amount turns it down",
            )
            .clicked()
        {
            modulation.mode = ModMode::Unipolar;
        }
        ui.horizontal(|ui| {
            ui.label("Amount");
            let unit = match m.spec.scale {
                ModScale::Octaves => " oct".to_owned(),
                ModScale::Linear if m.spec.unit.is_empty() => String::new(),
                ModScale::Linear => format!(" {}", m.spec.unit),
            };
            let range = match modulation.mode {
                ModMode::Bipolar => 0.0..=f64::INFINITY,
                ModMode::Unipolar => f64::NEG_INFINITY..=f64::INFINITY,
            };
            ui.add(
                ValueBox::new(&mut modulation.amount)
                    .range(range)
                    .speed(span / 300.0)
                    .suffix(&unit)
                    .max_decimals(3),
            );
        });
        if ui
            .add_enabled(
                modulation.amount != default_amount,
                egui::Button::new("Reset amount").shortcut_text("Alt+click"),
            )
            .clicked()
        {
            modulation.amount = default_amount;
            ui.close();
        }
        ui.separator();
        if ui.button("Disconnect signal").clicked() {
            result.disconnect = true;
            ui.close();
        }
    });
    result
}

/// Screen distance between the interpolated copies of a ghost handle.
const SMEAR_PX: f32 = 2.0;

/// How far the knob's arc turns at full amount, either side of the top (radians).
const KNOB_SWEEP: f32 = 2.4;

fn paint_rail(ui: &Ui, rail: Rect, value_x: f32) {
    let theme = Theme::of(ui.ctx());
    let painter = ui.painter();
    painter.rect_filled(
        rail,
        CornerRadius::same(2),
        ui.visuals().widgets.inactive.bg_fill,
    );
    painter.rect_filled(
        Rect::from_min_max(rail.min, pos2(value_x, rail.max.y)),
        CornerRadius::same(2),
        theme.accent.gamma_multiply(0.8),
    );
}

/// The span a modulating signal covers: outlined over the rail in the wire's colour, with a tick
/// at each end.
fn paint_range(ui: &Ui, rail: Rect, lo: f32, hi: f32, color: Color32) {
    let painter = ui.painter();
    let band = Rect::from_min_max(pos2(lo, rail.top() - 3.0), pos2(hi, rail.bottom() + 3.0));
    painter.rect_filled(band, CornerRadius::same(2), color.gamma_multiply(0.18));
    painter.rect_stroke(
        band,
        CornerRadius::same(2),
        Stroke::new(1.0, color),
        egui::StrokeKind::Middle,
    );
    for x in [lo, hi] {
        painter.line_segment(
            [pos2(x, band.top() - 3.0), pos2(x, band.bottom() + 3.0)],
            Stroke::new(1.5, color),
        );
    }
}

fn paint_handle(ui: &Ui, response: &egui::Response, at: egui::Pos2, height: f32) {
    let visuals = ui.style().interact(response);
    ui.painter().circle(
        at,
        height * 0.32,
        visuals.bg_fill,
        Stroke::new(1.0, visuals.fg_stroke.color),
    );
}

/// The live value: a see-through handle in the wire's colour, over the real one.
fn paint_ghost(ui: &Ui, at: egui::Pos2, height: f32, color: Color32) {
    ui.painter().circle(
        at,
        height * 0.32,
        color.gamma_multiply(0.45),
        Stroke::new(1.5, color),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUTOFF: NumberRange = NumberRange {
        default: 40.0,
        soft: (0.01, 100_000.0),
        limits: (1e-6, 1e9),
    };
    const DEPTH: NumberRange = NumberRange {
        default: 0.0,
        soft: (-10.0, 10.0),
        limits: (f64::NEG_INFINITY, f64::INFINITY),
    };

    #[test]
    fn positions_and_values_map_both_ways() {
        let range = DEPTH.soft;
        assert_eq!(to_fraction(0.0, range, false), 0.5);
        assert_eq!(from_fraction(0.75, range, false), 5.0);
        assert_eq!(from_fraction(2.0, range, false), 10.0, "clamped");

        assert!(is_logarithmic(CUTOFF.soft) && !is_logarithmic(DEPTH.soft));
        let range = CUTOFF.soft;
        let middle = from_fraction(0.5, range, true);
        assert!((middle - 31.6).abs() < 0.05, "{middle}");
        assert!((to_fraction(middle, range, true) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn amounts_read_with_their_direction_and_unit() {
        use rastersong_engine::{ModMode, Modulation, ParamSpec};
        let feedback = ParamSpec::number("feedback", "Feedback", 0.0, 0.0, 1.0, "");
        let cutoff = ParamSpec::number("cutoff", "Cutoff", 40.0, 1.0, 1e5, "").octaves();
        let bits = ParamSpec::number("bits", "Bits", 4.0, 1.0, 24.0, "").unit("bits");
        let m = |amount, mode| Modulation { amount, mode };
        assert_eq!(amount_text(&feedback, m(0.25, ModMode::Bipolar)), "±0.250");
        assert_eq!(amount_text(&feedback, m(-0.5, ModMode::Unipolar)), "-0.500");
        assert_eq!(amount_text(&cutoff, m(1.0, ModMode::Bipolar)), "±1 oct");
        assert_eq!(amount_text(&bits, m(2.0, ModMode::Unipolar)), "+2 bits");
    }

    #[test]
    fn dragged_values_are_tidy() {
        let value = from_fraction(0.123_456, (0.0, 1.0), false);
        assert_eq!(value, 0.123);
        let value = from_fraction(0.123_456, (0.0, 1000.0), false);
        assert_eq!(value, 123.0);
    }
}
