//! Small icons drawn with shapes (so they need no font), and a vertical list of them to choose
//! from.

use eframe::egui::{self, Color32, Painter, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::theme::Theme;

/// An icon for a way of looking at a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Picture,
    Scope,
    Spectrum,
    /// Numbers and meters only.
    Readings,
}

/// The size of one icon cell in a list.
const CELL: f32 = 28.0;

/// Draws `icon` to fill `rect` in `color`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let rect = rect.shrink(rect.width() * 0.2);
    let stroke = Stroke::new(1.5, color);
    let at = |x: f32, y: f32| {
        pos2(
            rect.left() + rect.width() * x,
            rect.top() + rect.height() * y,
        )
    };
    match icon {
        Icon::Picture => {
            painter.rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
            painter.add(egui::Shape::line(
                vec![
                    at(0.0, 0.85),
                    at(0.35, 0.45),
                    at(0.6, 0.7),
                    at(0.8, 0.5),
                    at(1.0, 0.8),
                ],
                stroke,
            ));
            painter.circle_filled(at(0.75, 0.25), rect.width() * 0.08, color);
        }
        Icon::Scope => {
            let points = (0..=16)
                .map(|i| {
                    let x = i as f32 / 16.0;
                    at(x, 0.5 - 0.4 * (x * std::f32::consts::TAU * 1.5).sin())
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
        }
        Icon::Spectrum => {
            for (i, height) in [0.45, 0.8, 0.6, 1.0, 0.35].into_iter().enumerate() {
                let x = 0.1 + i as f32 * 0.2;
                painter.line_segment(
                    [at(x, 1.0), at(x, 1.0 - height)],
                    Stroke::new(rect.width() * 0.13, color),
                );
            }
        }
        Icon::Readings => {
            for (i, width) in [1.0, 0.65, 0.85].into_iter().enumerate() {
                let y = 0.2 + i as f32 * 0.3;
                painter.line_segment([at(0.0, y), at(width, y)], stroke);
            }
        }
    }
}

/// A vertical list of icons with their names as tooltips, `selected` highlighted. Returns the
/// index clicked, if any.
pub fn icon_list(ui: &mut Ui, items: &[(Icon, &str)], selected: usize) -> Option<usize> {
    let theme = Theme::of(ui.ctx());
    let mut clicked = None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (i, (icon, name)) in items.iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(vec2(CELL, CELL), Sense::click());
            let on = i == selected;
            if on {
                ui.painter()
                    .rect_filled(rect, 4.0, theme.accent.gamma_multiply(0.35));
            } else if response.hovered() {
                ui.painter()
                    .rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
            }
            let color = if on {
                ui.visuals().strong_text_color()
            } else {
                theme.text_dim
            };
            paint(ui.painter(), rect, *icon, color);
            if response.on_hover_text(*name).clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}
