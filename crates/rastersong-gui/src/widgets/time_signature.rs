//! The time signature field: how the timeline's tempo, the tempo unit and the Beat node all write
//! a note length.

use eframe::egui::{Response, RichText, Ui};

use crate::theme::Theme;
use crate::value_box::ValueBox;

/// `numerator / denominator` as two whole-number boxes with a slash between them, each from 1 to
/// `max`. The response is changed when either number is.
pub fn time_signature(
    ui: &mut Ui,
    numerator: &mut f64,
    denominator: &mut f64,
    max: f64,
) -> Response {
    let theme = Theme::of(ui.ctx());
    let reversed = ui.layout().prefer_right_to_left();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        // Numerator first on screen, also in a right-to-left row (which adds from the right).
        let (first, second) = if reversed {
            (denominator, numerator)
        } else {
            (numerator, denominator)
        };
        let mut response = ui.add(field(first, max));
        ui.label(RichText::new("/").color(theme.text_dim));
        response |= ui.add(field(second, max));
        response
    })
    .inner
}

/// One of the two numbers.
fn field(value: &mut f64, max: f64) -> ValueBox<'_> {
    ValueBox::new(value)
        .whole(true)
        .range(1.0..=max)
        .speed(0.05)
        .max_decimals(0)
}
