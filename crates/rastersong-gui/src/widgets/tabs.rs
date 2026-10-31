//! A row of tabs over a card's body: names side by side, the open one underlined in the accent.

use eframe::egui::{self, Align2, Rect, Sense, Stroke, Ui, WidgetInfo, WidgetType, vec2};

use crate::theme::Theme;

/// Height of the tab row.
pub const TABS_HEIGHT: f32 = 30.0;

/// Draws `options` (each a name and its hover text) as tabs from the left, `selected` open, with
/// a divider across the whole row under them. Returns the index clicked, if any.
pub fn tabs(ui: &mut Ui, options: &[(&str, &str)], selected: usize) -> Option<usize> {
    let theme = Theme::of(ui.ctx());
    let font = crate::theme::medium(12.0);
    let id = ui.next_auto_id();
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), TABS_HEIGHT), Sense::hover());
    ui.painter().hline(
        row.x_range(),
        row.bottom() - 0.5,
        Stroke::new(1.0, theme.surfaces.divider),
    );
    let mut clicked = None;
    let mut x = row.left();
    for (i, (name, help)) in options.iter().enumerate() {
        let on = i == selected;
        let color = if on {
            theme.surfaces.text
        } else {
            theme.text_dim
        };
        let galley = ui
            .painter()
            .layout_no_wrap((*name).to_owned(), font.clone(), color);
        let rect = Rect::from_min_size(
            egui::pos2(x, row.top()),
            vec2(galley.size().x + 20.0, row.height()),
        );
        x = rect.right();
        let response = ui.interact(rect, id.with(("tab", i)), Sense::click());
        response.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, on, *name));
        let color = if !on && response.hovered() {
            theme.surfaces.text_soft
        } else {
            color
        };
        let pos = Align2::CENTER_CENTER
            .anchor_size(rect.center(), galley.size())
            .min;
        ui.painter().galley(pos, galley, color);
        if on {
            let line = Rect::from_min_max(
                egui::pos2(rect.left() + 6.0, rect.bottom() - 2.0),
                egui::pos2(rect.right() - 6.0, rect.bottom()),
            );
            ui.painter().rect_filled(line, 1, theme.accent);
        }
        if response.on_hover_text(*help).clicked() {
            clicked = Some(i);
        }
    }
    clicked
}
