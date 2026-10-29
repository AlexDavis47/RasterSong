//! A panel drawn as a card: rounded and outlined on the window's ground, with an optional title
//! row across its top.

use eframe::egui::{self, Frame, Margin, Rect, Stroke, Ui, UiBuilder, vec2};

use crate::theme::{CARD_RADIUS, Theme};

/// Space between two cards, and between a card and the window's edge.
pub const CARD_GAP: i8 = 6;
/// Half of [`CARD_GAP`]: between two cards, each side of the panel edge gets this much, so a
/// resize handle (which egui puts on the edge) sits in the middle of the gap.
pub const HALF_GAP: i8 = CARD_GAP / 2;
/// Height of a card's title row.
pub const CARD_HEADER_HEIGHT: f32 = 40.0;

/// The frame of a panel that holds a card: the window's ground, with `gaps` around the card.
pub fn card_space(ui: &Ui, gaps: Margin) -> Frame {
    Frame::new()
        .fill(Theme::of(ui.ctx()).surfaces.ground)
        .inner_margin(gaps)
}

/// Shows a resizable `panel` that holds a card, `gaps` around it, with no line drawn on its
/// resize handle, even while it's hovered or dragged: the gap between the cards is the handle,
/// and the cursor shows it.
pub fn card_panel<R>(
    ui: &mut Ui,
    panel: egui::Panel,
    gaps: Margin,
    add: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    // egui strokes the handle with the parent's hovered and active widget strokes, read after
    // the panel's contents are laid out, so they are blanked around the panel only.
    let style = ui.style().clone();
    let mut quiet = (*style).clone();
    quiet.visuals.widgets.hovered.fg_stroke = Stroke::NONE;
    quiet.visuals.widgets.active.fg_stroke = Stroke::NONE;
    ui.set_style(quiet);
    let frame = card_space(ui, gaps);
    let response = panel
        .resizable(true)
        .show_separator_line(false)
        .frame(frame)
        .show(ui, |ui| {
            ui.set_style(style.clone());
            add(ui)
        });
    ui.set_style(style);
    response
}

/// Fills the space left in `ui` with a card and draws `add` in it, edge to edge: a title row
/// ([`card_header`]) spans the card, and [`card_body`] pads what goes under it.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let theme = Theme::of(ui.ctx());
    let rect = ui.available_rect_before_wrap();
    ui.painter()
        .rect_filled(rect, CARD_RADIUS, theme.surfaces.panel);
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect.shrink(1.0)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let inner = add(&mut child);
    ui.allocate_rect(rect, egui::Sense::hover());
    // The outline last, over what the card holds, so the canvas and lanes meet it cleanly.
    paint_card_outline(ui, rect);
    inner
}

/// The card's outline, with the window's ground painted outside its rounded corners so whatever
/// the card holds keeps to them.
fn paint_card_outline(ui: &Ui, rect: Rect) {
    let theme = Theme::of(ui.ctx());
    let painter = ui.painter();
    let r = f32::from(CARD_RADIUS);
    for (corner, toward) in [
        (rect.left_top(), vec2(1.0, 1.0)),
        (rect.right_top(), vec2(-1.0, 1.0)),
        (rect.left_bottom(), vec2(1.0, -1.0)),
        (rect.right_bottom(), vec2(-1.0, -1.0)),
    ] {
        // The square in the corner, less the quarter circle inside it: a fan from the corner
        // along the arc, which egui fills as one.
        let centre = corner + toward * r;
        let steps = 8;
        let mut points = vec![corner];
        for i in 0..=steps {
            let a = std::f32::consts::FRAC_PI_2 * i as f32 / steps as f32;
            points.push(centre - vec2(toward.x * a.cos(), toward.y * a.sin()) * r);
        }
        painter.add(egui::Shape::convex_polygon(
            points,
            theme.surfaces.ground,
            Stroke::NONE,
        ));
    }
    painter.rect_stroke(
        rect,
        CARD_RADIUS,
        Stroke::new(1.0, theme.surfaces.card_border),
        egui::StrokeKind::Inside,
    );
}

/// A card's title row: 40 px high with a divider under it. `add` lays it out left to right.
pub fn card_header<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let theme = Theme::of(ui.ctx());
    let size = vec2(ui.available_width(), CARD_HEADER_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, theme.surfaces.divider),
    );
    let inner = Rect::from_min_max(rect.min + vec2(12.0, 0.0), rect.max - vec2(8.0, 1.0));
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    add(&mut child)
}

/// What a card holds under its title row, `margin` in from the card's edge.
pub fn card_body<R>(ui: &mut Ui, margin: i8, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .inner_margin(Margin::same(margin))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            add(ui)
        })
        .inner
}

/// A card's title, in semibold.
pub fn card_title(ui: &mut Ui, title: &str) -> egui::Response {
    ui.label(egui::RichText::new(title).font(crate::theme::semibold(crate::theme::TITLE_SIZE)))
}
