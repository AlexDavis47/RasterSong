//! A key cap: the name of a key in a small raised box, for the shortcuts a footer or a button
//! names.

use eframe::egui::{self, Align2, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, vec2};

use crate::theme::{SMALL_SIZE, Theme};

const HEIGHT: f32 = 17.0;
const PAD: f32 = 4.0;

/// Draws `key` as a key cap in the row.
pub fn kbd(ui: &mut Ui, key: &str) -> Response {
    let size = vec2(text_width(ui.painter(), key) + 2.0 * PAD, HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    paint_cap(ui.painter(), Theme::of(ui.ctx()), rect, key);
    response
}

/// Draws `key` as a key cap ending at `right_center`, and returns where it went.
pub fn paint_kbd(painter: &Painter, theme: &Theme, right_center: Pos2, key: &str) -> Rect {
    let size = vec2(text_width(painter, key) + 2.0 * PAD, HEIGHT);
    let rect = Align2::RIGHT_CENTER.anchor_size(right_center, size);
    paint_cap(painter, theme, rect, key);
    rect
}

fn text_width(painter: &Painter, key: &str) -> f32 {
    painter
        .layout_no_wrap(
            key.to_owned(),
            FontId::monospace(SMALL_SIZE),
            egui::Color32::PLACEHOLDER,
        )
        .size()
        .x
}

/// The cap's border is thicker along the bottom, like the edge of a key.
fn paint_cap(painter: &Painter, theme: &Theme, rect: Rect, key: &str) {
    let stroke = Stroke::new(1.0, theme.node_outline);
    painter.rect_stroke(rect, 4, stroke, egui::StrokeKind::Inside);
    painter.hline(rect.x_range().shrink(3.0), rect.bottom() - 1.5, stroke);
    painter.text(
        rect.center() - vec2(0.0, 0.5),
        Align2::CENTER_CENTER,
        key,
        FontId::monospace(SMALL_SIZE),
        theme.text_dim,
    );
}
