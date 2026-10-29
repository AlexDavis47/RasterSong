//! One of a few choices drawn as a card: a radio dot, a name, and a line saying what it is for.

use eframe::egui::{
    self, Response, Sense, Stroke, TextWrapMode, Ui, WidgetInfo, WidgetText, WidgetType, pos2, vec2,
};

use crate::theme::{self, CARD_RADIUS, Theme};

/// Space inside the card, across and down.
const PADDING: egui::Vec2 = vec2(12.0, 10.0);
/// The radio dot's size, and the gap after it.
const DOT: f32 = 16.0;
const DOT_GAP: f32 = 10.0;

/// A card as wide as the space, outlined in the accent while `selected`. Named `name` for screen
/// readers (and tests); `about` wraps under it.
pub fn choice_card(ui: &mut Ui, selected: bool, name: &str, about: &str) -> Response {
    let theme = Theme::of(ui.ctx());
    let width = ui.available_width();
    let text_width = width - 2.0 * PADDING.x - DOT - DOT_GAP;
    let name_galley = WidgetText::from(name).into_galley(
        ui,
        Some(TextWrapMode::Truncate),
        text_width,
        theme::semibold(13.0),
    );
    let about_galley = WidgetText::from(egui::RichText::new(about).size(11.5)).into_galley(
        ui,
        Some(TextWrapMode::Wrap),
        text_width,
        egui::TextStyle::Body,
    );
    let height = 2.0 * PADDING.y + name_galley.size().y + 2.0 + about_galley.size().y;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, enabled, selected, name));

    let (fill, border) = if selected {
        (
            theme.surfaces.inner_card.lerp_to_gamma(theme.accent, 0.1),
            theme.accent,
        )
    } else if response.hovered() {
        (theme.surfaces.inner_card, theme.surfaces.border_hover)
    } else {
        (theme.surfaces.inner_card, theme.surfaces.border)
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CARD_RADIUS,
        fill,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    let dot = pos2(
        rect.left() + PADDING.x + DOT / 2.0,
        rect.top() + PADDING.y + DOT / 2.0,
    );
    if selected {
        painter.circle_filled(dot, DOT / 2.0, theme.accent);
        painter.circle_filled(dot, 3.0, theme.surfaces.ground);
    } else {
        painter.circle(
            dot,
            DOT / 2.0 - 0.5,
            theme.surfaces.ground,
            Stroke::new(1.0, theme.surfaces.border_hover),
        );
    }
    let text_left = rect.left() + PADDING.x + DOT + DOT_GAP;
    let name_height = name_galley.size().y;
    painter.galley(
        pos2(text_left, rect.top() + PADDING.y),
        name_galley,
        theme.surfaces.text,
    );
    painter.galley(
        pos2(text_left, rect.top() + PADDING.y + name_height + 2.0),
        about_galley,
        theme.text_dim,
    );
    response
}
