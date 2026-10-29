//! The inspector's one layout, whatever is selected: a header with the kind of thing, a coloured
//! tag and the actions that apply to it, then its name and a line about it; under a divider, the
//! sections, each under a capital label.

use eframe::egui::{self, Color32, Margin, Rect, RichText, Sense, Stroke, TextEdit, Ui, vec2};

use crate::theme::{self, SMALL_SIZE, Theme};

/// Space between the inspector's edge and what it holds, across.
const PAD_X: i8 = 14;
/// Space between sections.
const SECTION_GAP: f32 = 16.0;
/// Width of a drop-down in a setting's row.
pub const CHOICE_WIDTH: f32 = 120.0;
/// Height of the header's first row: the kind, the tag and the actions.
const KIND_ROW: f32 = 24.0;

/// Shows the header (`header`, laid out top-down), a divider, and `body` scrolling under it.
pub fn inspector<R>(
    ui: &mut Ui,
    header: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui) -> R,
) -> R {
    inspector_header(ui, header);
    inspector_body(ui, body)
}

/// The inspector's header, then the divider under it. The header, the divider and the body that
/// follows sit flush: their own margins are the space between them.
pub fn inspector_header(ui: &mut Ui, header: impl FnOnce(&mut Ui)) {
    let theme = Theme::of(ui.ctx());
    ui.spacing_mut().item_spacing.y = 0.0;
    let margin = Margin {
        left: PAD_X,
        right: PAD_X,
        top: 12,
        bottom: 10,
    };
    egui::Frame::new().inner_margin(margin).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        header(ui);
    });
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, theme.surfaces.divider),
    );
}

/// The inspector's sections, scrolling under its header.
pub fn inspector_body<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(Margin::symmetric(PAD_X, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = SECTION_GAP;
                    body(ui)
                })
                .inner
        })
        .inner
}

/// The header's first row: `kind` ("Node") as a capital label, `tag` in its colour, and
/// `actions` laid out from the right edge leftwards.
pub fn kind_row(
    ui: &mut Ui,
    kind: &str,
    tag: Option<(&str, Color32)>,
    actions: impl FnOnce(&mut Ui),
) {
    let size = vec2(ui.available_width(), KIND_ROW);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(size);
            ui.spacing_mut().item_spacing.x = 8.0;
            section_label(ui, kind);
            if let Some((text, color)) = tag {
                pill(ui, text, color);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
        },
    );
}

/// `text` in a rounded tag tinted with `color`: a node's category, a track's kind.
pub fn pill(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), egui::FontId::proportional(11.0), color);
    let size = galley.size() + vec2(16.0, 2.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, rect.height() / 2.0, color.gamma_multiply(0.12));
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
    response
}

/// How the name field looks: large semibold text with no frame, as wide as the inspector.
pub fn name_style(edit: TextEdit<'_>) -> TextEdit<'_> {
    edit.font(theme::semibold(15.0))
        // The frame's margin, not `margin`, which a frame replaces.
        .frame(egui::Frame::NONE.inner_margin(Margin::symmetric(0, 7)))
        .desired_width(f32::INFINITY)
}

/// The line under the name: what the thing is, in muted text.
pub fn about(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(12.0)
            .color(Theme::of(ui.ctx()).text_dim),
    );
}

/// A section: its capital label, then `add` top-down.
pub fn section<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        section_label(ui, title);
        add(ui)
    })
    .inner
}

/// A label in small semibold capitals, spaced out: a section's title, a reading's name.
pub fn section_label(ui: &mut Ui, text: &str) -> egui::Response {
    let color = Theme::of(ui.ctx()).surfaces.text_faint;
    section_label_in(ui, text, color)
}

/// A [`section_label`] in `color`: the timeline's section names take their kind's colour.
pub fn section_label_in(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    ui.label(
        RichText::new(text.to_uppercase())
            .font(theme::semibold(SMALL_SIZE))
            .extra_letter_spacing(SMALL_SIZE * 0.08)
            .color(color),
    )
}

/// A setting on one line: `label` (with `help` on hover) on the left, `add` on the right.
pub fn setting_row<R>(ui: &mut Ui, label: &str, help: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let theme = Theme::of(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(theme.surfaces.text_soft))
            .on_hover_text(help);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add)
            .inner
    })
    .inner
}

/// A setting that is on or off: `label` (with `help` on hover) on the left, a check box on the
/// right named `label`. Returns true when clicked, having flipped `on`.
pub fn setting_toggle(ui: &mut Ui, label: &str, help: &str, on: &mut bool) -> bool {
    let accent = Theme::of(ui.ctx()).accent;
    let clicked = setting_row(ui, label, help, |ui| {
        toggle_box(ui, *on, accent, label, |painter, rect, color, on| {
            if on {
                let c = rect.center();
                let points = vec![
                    c + vec2(-5.0, 0.0),
                    c + vec2(-1.5, 3.5),
                    c + vec2(5.0, -3.5),
                ];
                painter.add(egui::Shape::line(points, Stroke::new(1.6, color)));
            }
        })
        .on_hover_text(help)
        .clicked()
    });
    if clicked {
        *on = !*on;
    }
    clicked
}

/// A short line in `color` (a wire's or an output's): `width` across and 3 high.
pub fn swatch(ui: &mut Ui, width: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 3.0), Sense::hover());
    ui.painter().rect_filled(rect, 2.0, color);
}

/// A thin line across `ui`, between the parts of a card.
pub fn rule(ui: &mut Ui, color: Color32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, color));
    rect
}

/// A small outlined toggle, 22 px high: `paint` draws what it shows in `color` when `on`, muted
/// when off. Named `label` for screen readers (and tests).
pub fn toggle_box(
    ui: &mut Ui,
    on: bool,
    color: Color32,
    label: &str,
    paint: impl FnOnce(&egui::Painter, Rect, Color32, bool),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
    let enabled = ui.is_enabled();
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, on, label));
    let theme = Theme::of(ui.ctx());
    let (stroke, ink) = if on {
        (color, color)
    } else if response.hovered() {
        (theme.surfaces.border_hover, theme.surfaces.text_soft)
    } else {
        (theme.node_outline, theme.text_dim)
    };
    ui.painter()
        .rect_stroke(rect, 4, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    paint(ui.painter(), rect, ink, on);
    response
}
