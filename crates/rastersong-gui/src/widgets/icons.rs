//! Small icons drawn with shapes (so they need no font), a button that shows one, and a vertical
//! list of them to choose from.

use eframe::egui::{
    self, Color32, Painter, Rect, Response, Sense, Stroke, Ui, Vec2, Widget, WidgetInfo,
    WidgetType, pos2, vec2,
};

use crate::theme::{CONTROL_RADIUS, DENSE_HEIGHT, Theme};

/// An icon: a way of looking at a signal, or an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Picture,
    Scope,
    Spectrum,
    /// Numbers and meters only.
    Readings,
    Play,
    Pause,
    /// Go to the start.
    ToStart,
    Loop,
    /// Fit the picture to its pane.
    Fit,
    Volume,
    Settings,
    /// Add something: a node.
    Plus,
    /// Back to the default.
    Reset,
    /// Remove or disconnect.
    Close,
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
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![at(0.2, 0.05), at(0.95, 0.5), at(0.2, 0.95)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            for x in [0.3, 0.7] {
                painter.line_segment(
                    [at(x, 0.08), at(x, 0.92)],
                    Stroke::new(rect.width() * 0.2, color),
                );
            }
        }
        Icon::ToStart => {
            painter.line_segment([at(0.15, 0.08), at(0.15, 0.92)], stroke);
            painter.add(egui::Shape::convex_polygon(
                vec![at(0.9, 0.08), at(0.9, 0.92), at(0.35, 0.5)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Loop => {
            // Two quarter-turns chasing each other, each ending in an arrowhead.
            let turn = |from: f32, centre: (f32, f32)| {
                (0..=6)
                    .map(|i| {
                        let a = from + i as f32 / 6.0 * std::f32::consts::FRAC_PI_2;
                        at(centre.0 + 0.33 * a.cos(), centre.1 + 0.33 * a.sin())
                    })
                    .collect::<Vec<_>>()
            };
            let mut top = turn(std::f32::consts::PI, (0.42, 0.42));
            top.push(at(0.9, 0.09));
            painter.add(egui::Shape::line(top, stroke));
            painter.add(egui::Shape::line(
                vec![at(0.74, -0.06), at(0.9, 0.09), at(0.74, 0.24)],
                stroke,
            ));
            let mut bottom = turn(0.0, (0.58, 0.58));
            bottom.push(at(0.1, 0.91));
            painter.add(egui::Shape::line(bottom, stroke));
            painter.add(egui::Shape::line(
                vec![at(0.26, 0.76), at(0.1, 0.91), at(0.26, 1.06)],
                stroke,
            ));
        }
        Icon::Fit => {
            for (x, y, dx, dy) in [
                (0.0, 0.0, 1.0, 1.0),
                (1.0, 0.0, -1.0, 1.0),
                (1.0, 1.0, -1.0, -1.0),
                (0.0, 1.0, 1.0, -1.0),
            ] {
                painter.add(egui::Shape::line(
                    vec![at(x, y + 0.25 * dy), at(x, y), at(x + 0.25 * dx, y)],
                    stroke,
                ));
            }
        }
        Icon::Volume => {
            painter.add(egui::Shape::closed_line(
                vec![
                    at(0.0, 0.35),
                    at(0.25, 0.35),
                    at(0.58, 0.08),
                    at(0.58, 0.92),
                    at(0.25, 0.65),
                    at(0.0, 0.65),
                ],
                stroke,
            ));
            let wave = (0..=6)
                .map(|i| {
                    let a = (i as f32 / 6.0 - 0.5) * 2.2;
                    at(0.62 + 0.32 * a.cos(), 0.5 + 0.32 * a.sin())
                })
                .collect();
            painter.add(egui::Shape::line(wave, stroke));
        }
        Icon::Plus => {
            painter.line_segment([at(0.5, 0.05), at(0.5, 0.95)], stroke);
            painter.line_segment([at(0.05, 0.5), at(0.95, 0.5)], stroke);
        }
        Icon::Reset => {
            // Most of a turn, ending in an arrowhead at the top left.
            let centre = rect.center();
            let r = rect.width() * 0.45;
            let points = (0..=14)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + 1.6 * i as f32 / 14.0) + 0.45;
                    centre - r * vec2(a.cos(), a.sin())
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            painter.add(egui::Shape::line(
                vec![at(0.05, 0.0), at(0.05, 0.35), at(0.4, 0.35)],
                stroke,
            ));
        }
        Icon::Close => {
            painter.line_segment([at(0.15, 0.15), at(0.85, 0.85)], stroke);
            painter.line_segment([at(0.85, 0.15), at(0.15, 0.85)], stroke);
        }
        Icon::Settings => {
            let centre = rect.center();
            let r = rect.width() / 2.0;
            painter.circle_stroke(centre, r * 0.4, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let dir = vec2(a.cos(), a.sin());
                painter.line_segment([centre + dir * r * 0.68, centre + dir * r], stroke);
            }
        }
    }
}

/// A button showing an icon, named `label` for screen readers (and tests). A plain one is drawn
/// only while hovered; [`IconButton::accent`] fills one with the accent, for the main action, and
/// [`IconButton::selected`] outlines one in it, for a toggle that is on.
#[derive(Debug)]
pub struct IconButton<'a> {
    icon: Icon,
    label: &'a str,
    size: Vec2,
    accent: bool,
    selected: bool,
}

impl<'a> IconButton<'a> {
    pub fn new(icon: Icon, label: &'a str) -> Self {
        Self {
            icon,
            label,
            size: Vec2::splat(DENSE_HEIGHT),
            accent: false,
            selected: false,
        }
    }

    pub fn size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }

    pub fn accent(mut self) -> Self {
        self.accent = true;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(self.size, Sense::click());
        let enabled = ui.is_enabled();
        response.widget_info(|| {
            WidgetInfo::selected(WidgetType::Button, enabled, self.selected, self.label)
        });
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let theme = Theme::of(ui.ctx());
        let visuals = ui.style().interact(&response);
        let active = response.hovered() || response.has_focus();
        let (fill, stroke, color) = if self.accent {
            let fill = if active {
                theme.accent.lerp_to_gamma(Color32::WHITE, 0.15)
            } else {
                theme.accent
            };
            (fill, Stroke::NONE, theme.surfaces.ground)
        } else if self.selected {
            (
                visuals.weak_bg_fill,
                Stroke::new(1.0, theme.accent),
                theme.accent,
            )
        } else if active {
            (
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                visuals.fg_stroke.color,
            )
        } else {
            (Color32::TRANSPARENT, Stroke::NONE, theme.text_dim)
        };
        let (fill, color) = if enabled {
            (fill, color)
        } else {
            (fill.gamma_multiply(0.4), color.gamma_multiply(0.4))
        };
        let painter = ui.painter();
        painter.rect(rect, CONTROL_RADIUS, fill, stroke, egui::StrokeKind::Inside);
        let side = 20.0_f32.min(rect.height());
        paint(
            painter,
            Rect::from_center_size(rect.center(), Vec2::splat(side)),
            self.icon,
            color,
        );
        response
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
