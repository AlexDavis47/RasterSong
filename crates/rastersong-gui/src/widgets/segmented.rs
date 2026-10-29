//! A row of choices of which one is on, set in a sunken track.

use eframe::egui::{self, Align2, Rect, Sense, Stroke, Ui, WidgetInfo, WidgetType, vec2};

use crate::theme::{DENSE_HEIGHT, Theme};

/// Padding between the track and its segments.
const INSET: f32 = 2.0;

/// Draws `options` (each a name and its hover text) side by side with `selected` on. Returns the
/// index clicked, if any; clicking the one that is on returns it too.
pub fn segmented(ui: &mut Ui, options: &[(&str, &str)], selected: usize) -> Option<usize> {
    let theme = Theme::of(ui.ctx());
    let font = crate::theme::medium(12.0);
    let galleys: Vec<_> = options
        .iter()
        .map(|(name, _)| {
            ui.painter()
                .layout_no_wrap((*name).to_owned(), font.clone(), theme.surfaces.text)
        })
        .collect();
    let height = DENSE_HEIGHT - 2.0 * INSET;
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 18.0).collect();
    let total = vec2(
        widths.iter().sum::<f32>() + INSET * (options.len() as f32 + 1.0),
        DENSE_HEIGHT,
    );
    let (track, _) = ui.allocate_exact_size(total, Sense::hover());
    ui.painter().rect(
        track,
        7,
        theme.surfaces.ground,
        Stroke::new(1.0, theme.surfaces.border),
        egui::StrokeKind::Inside,
    );
    let enabled = ui.is_enabled();
    let mut clicked = None;
    let mut x = track.left() + INSET;
    for (i, (((name, help), galley), width)) in options.iter().zip(galleys).zip(widths).enumerate()
    {
        let rect = Rect::from_min_size(egui::pos2(x, track.top() + INSET), vec2(width, height));
        x += width + INSET;
        let response = ui.interact(rect, ui.id().with(("segment", *name)), Sense::click());
        let on = i == selected;
        response
            .widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, enabled, on, *name));
        let fill = if on {
            theme.surfaces.raised_active
        } else if response.hovered() {
            theme.surfaces.raised_hover
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 5, fill);
        let color = if on {
            theme.surfaces.text
        } else {
            theme.text_dim
        };
        let pos = Align2::CENTER_CENTER
            .anchor_size(rect.center(), galley.size())
            .min;
        ui.painter().galley(pos, galley, color);
        if response.on_hover_text(*help).clicked() {
            clicked = Some(i);
        }
    }
    clicked
}
