//! The number field for node parameters: a slider track plus a value box.
//!
//! The track covers the parameter's usual range, widened to include the current value. Typing a
//! value outside the usual range (anything up to the node's hard limits) widens the track, so the
//! user is never boxed in, Substance Designer style. The track's range is frozen while it's being
//! dragged, so it can't shift under the pointer.

use eframe::egui::{self, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::theme::Theme;

/// A number parameter as the field shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberRange {
    /// The usual range, which the track covers by default.
    pub soft: (f64, f64),
    /// The values the node accepts.
    pub limits: (f64, f64),
}

impl NumberRange {
    /// Whether the track maps logarithmically: positive ranges spanning three decades or more.
    pub fn logarithmic(&self) -> bool {
        self.soft.0 > 0.0 && self.soft.1 / self.soft.0 >= 1000.0
    }

    /// The range the track shows for `value`: the usual range, widened to include it.
    pub fn visible(&self, value: f64) -> (f64, f64) {
        let value = value.clamp(self.limits.0, self.limits.1);
        (self.soft.0.min(value), self.soft.1.max(value))
    }
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

/// Shows the field. Returns true if the value changed.
pub fn param_field(
    ui: &mut Ui,
    id_salt: &str,
    value: &mut f64,
    range: NumberRange,
    suffix: &str,
    track_width: f32,
) -> bool {
    let id = ui.make_persistent_id(id_salt);
    let logarithmic = range.logarithmic();
    let before = *value;

    let height = ui.spacing().interact_size.y;
    let (rect, track) = ui.allocate_exact_size(vec2(track_width, height), Sense::click_and_drag());
    // Freeze the range for the length of a drag.
    let frozen: Option<(f64, f64)> = ui.data(|d| d.get_temp(id));
    let shown = frozen.unwrap_or_else(|| range.visible(*value));
    if track.drag_started() {
        ui.data_mut(|d| d.insert_temp(id, shown));
    }
    if (track.clicked() || track.dragged())
        && let Some(p) = track.interact_pointer_pos()
    {
        let t = f64::from((p.x - rect.left()) / rect.width());
        *value = from_fraction(t, shown, logarithmic);
    }
    if track.drag_stopped() {
        ui.data_mut(|d| d.remove::<(f64, f64)>(id));
    }
    paint_track(
        ui,
        rect,
        &track,
        to_fraction(*value, shown, logarithmic) as f32,
    );

    let speed = if logarithmic {
        value.abs().max(range.soft.0) * 0.01
    } else {
        (shown.1 - shown.0) / 300.0
    };
    ui.add(
        egui::DragValue::new(value)
            .range(range.limits.0..=range.limits.1)
            .speed(speed)
            .suffix(suffix)
            .max_decimals(3),
    )
    .on_hover_text("Drag, or double-click to type. Values beyond the slider are allowed.");
    *value != before
}

fn paint_track(ui: &Ui, rect: Rect, response: &egui::Response, t: f32) {
    let theme = Theme::of(ui.ctx());
    let visuals = ui.style().interact(response);
    let rail = Rect::from_center_size(rect.center(), vec2(rect.width() - 8.0, 4.0));
    let painter = ui.painter();
    painter.rect_filled(
        rail,
        CornerRadius::same(2),
        ui.visuals().widgets.inactive.bg_fill,
    );
    let x = rail.left() + rail.width() * t;
    painter.rect_filled(
        Rect::from_min_max(rail.min, pos2(x, rail.max.y)),
        CornerRadius::same(2),
        theme.accent.gamma_multiply(0.8),
    );
    let radius = rect.height() * 0.32;
    painter.circle(
        pos2(x, rect.center().y),
        radius,
        visuals.bg_fill,
        Stroke::new(1.0, visuals.fg_stroke.color),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUTOFF: NumberRange = NumberRange {
        soft: (0.01, 100_000.0),
        limits: (1e-6, 1e9),
    };
    const DEPTH: NumberRange = NumberRange {
        soft: (-10.0, 10.0),
        limits: (f64::NEG_INFINITY, f64::INFINITY),
    };

    #[test]
    fn the_track_widens_to_include_the_value() {
        assert_eq!(DEPTH.visible(3.0), (-10.0, 10.0));
        assert_eq!(DEPTH.visible(250.0), (-10.0, 250.0));
        assert_eq!(DEPTH.visible(-40.0), (-40.0, 10.0));
        // Never past the limits.
        assert_eq!(CUTOFF.visible(5e9), (0.01, 1e9));
    }

    #[test]
    fn positions_and_values_map_both_ways() {
        let range = DEPTH.visible(0.0);
        assert_eq!(to_fraction(0.0, range, false), 0.5);
        assert_eq!(from_fraction(0.75, range, false), 5.0);
        assert_eq!(from_fraction(2.0, range, false), 10.0, "clamped");

        assert!(CUTOFF.logarithmic() && !DEPTH.logarithmic());
        let range = CUTOFF.soft;
        let middle = from_fraction(0.5, range, true);
        assert!((middle - 31.6).abs() < 0.05, "{middle}");
        assert!((to_fraction(middle, range, true) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn dragged_values_are_tidy() {
        let value = from_fraction(0.123_456, (0.0, 1.0), false);
        assert_eq!(value, 0.123);
        let value = from_fraction(0.123_456, (0.0, 1000.0), false);
        assert_eq!(value, 123.0);
    }
}
