//! The beat lights: one light per beat of the bar, the current beat lit in amber.

use eframe::egui::{Response, Sense, Ui, Vec2, WidgetInfo, WidgetType, vec2};
use rastersong_lang::tr_args;

use crate::theme::Theme;

/// The most lights drawn; in longer bars the beats past this light none.
pub const MAX_BEAT_LIGHTS: u32 = 8;

/// A row of `beats` round lights (at most [`MAX_BEAT_LIGHTS`]) `diameter` wide, `gap` apart, with
/// beat `lit` (from 0) lit, or none.
pub fn beat_lights(ui: &mut Ui, beats: u32, lit: Option<u32>, diameter: f32, gap: f32) -> Response {
    let size = Vec2::splat(diameter);
    let theme = Theme::of(ui.ctx());
    let count = beats.clamp(1, MAX_BEAT_LIGHTS);
    let width = count as f32 * size.x + (count - 1) as f32 * gap;
    let (rect, response) = ui.allocate_exact_size(vec2(width, size.y), Sense::hover());
    response.widget_info(|| {
        let label = match lit {
            Some(beat) => tr_args(
                "tempo.beat_lights",
                &[
                    ("beat", &(beat + 1).to_string()),
                    ("beats", &beats.to_string()),
                ],
            ),
            None => tr_args("tempo.beat_lights.idle", &[("beats", &beats.to_string())]),
        };
        WidgetInfo::labeled(WidgetType::Label, true, label)
    });
    let radius = diameter / 2.0;
    for i in 0..count {
        let light = eframe::egui::Rect::from_min_size(
            rect.min + vec2(i as f32 * (size.x + gap), 0.0),
            size,
        );
        let color = if lit == Some(i) {
            theme.beat
        } else {
            theme.beat.gamma_multiply(0.28)
        };
        ui.painter().circle_filled(light.center(), radius, color);
    }
    response
}
