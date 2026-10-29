//! A key cap: the name of a key in a small raised box, for the shortcuts a footer or a button
//! names.

use eframe::egui::{
    self, Align2, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, WidgetInfo, WidgetType,
    vec2,
};

use super::{Icon, paint_icon};
use crate::theme::{CONTROL_RADIUS, SMALL_SIZE, Theme};

const HEIGHT: f32 = 17.0;
const PAD: f32 = 4.0;

/// Draws `key` as a key cap in the row.
pub fn kbd(ui: &mut Ui, key: &str) -> Response {
    let size = vec2(text_width(ui.painter(), key) + 2.0 * PAD, HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    paint_cap(ui.painter(), Theme::of(ui.ctx()), rect, key);
    response
}

/// A line of `text` with `key` as a key cap where `text` says `{key}` ("{key}+scroll turns
/// it"), so translations can put the key where their grammar wants it.
pub fn kbd_text(ui: &mut Ui, text: &str, key: &str, size: f32, color: egui::Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let (before, after) = text.split_once("{key}").unwrap_or((text, ""));
        let words = |ui: &mut Ui, words: &str| {
            if !words.is_empty() {
                ui.label(egui::RichText::new(words).size(size).color(color));
            }
        };
        words(ui, before);
        kbd(ui, key);
        words(ui, after);
    });
}

/// A button `height` high: `icon`, `label`, and the key that does the same as a key cap after
/// it ("+ Add node [Tab]"). Named `label` for screen readers (and tests).
pub fn key_button(ui: &mut Ui, icon: Icon, label: &str, key: &str, height: f32) -> Response {
    const ICON: f32 = 18.0;
    const GAP: f32 = 6.0;
    let font = egui::TextStyle::Button.resolve(ui.style());
    let text = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, egui::Color32::PLACEHOLDER);
    let key_width = text_width(ui.painter(), key) + 2.0 * PAD;
    let pad = ui.spacing().button_padding.x;
    let width = pad + ICON + GAP + text.size().x + GAP + key_width + pad * 0.6;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let theme = Theme::of(ui.ctx());
    let visuals = ui.style().interact(&response);
    let painter = ui.painter();
    painter.rect(
        rect,
        CONTROL_RADIUS,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let color = visuals.text_color();
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.left() + pad + ICON / 2.0, rect.center().y),
        egui::Vec2::splat(ICON),
    );
    paint_icon(painter, icon_rect, icon, color);
    painter.galley(
        egui::pos2(
            icon_rect.right() + GAP,
            rect.center().y - text.size().y / 2.0,
        ),
        text,
        color,
    );
    paint_kbd(
        painter,
        theme,
        egui::pos2(rect.right() - pad * 0.6, rect.center().y),
        key,
    );
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
