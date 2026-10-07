//! Level and gain-reduction meters, with peak hold.

use eframe::egui::{self, Align2, Color32, FontId, Id, Rect, Sense, Ui, vec2};
use rastersong_engine::MeterKind;
use rastersong_lang::{tr, tr_args};

use crate::theme::Theme;

/// The decibel range a level meter covers.
const LEVEL_RANGE: (f32, f32) = (-60.0, 6.0);
/// The most gain reduction a gain-reduction meter shows.
const REDUCTION_RANGE: f32 = 24.0;
/// How fast a held peak falls, in decibels per second.
const HOLD_FALL: f32 = 20.0;
const HEIGHT: f32 = 14.0;

fn to_db(linear: f32) -> f32 {
    20.0 * linear.max(1e-6).log10()
}

/// The position of `db` along the level scale, `0..=1`.
fn level_position(db: f32) -> f32 {
    ((db - LEVEL_RANGE.0) / (LEVEL_RANGE.1 - LEVEL_RANGE.0)).clamp(0.0, 1.0)
}

/// A peak held by a meter: it rises with the value and falls slowly.
fn held(ui: &Ui, id: Id, value: f32, floor: f32) -> f32 {
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let mut hold = value;
    ui.data_mut(|d| {
        let slot = d.get_temp_mut_or::<f32>(id, floor);
        hold = value.max(*slot - HOLD_FALL * dt);
        *slot = hold;
    });
    if hold > value {
        // Falling: keep the display moving.
        ui.ctx().request_repaint();
    }
    hold
}

fn frame(ui: &mut Ui, label: &str, text: String, fill: impl FnOnce(&egui::Painter, Rect)) {
    ui.horizontal(|ui| {
        ui.label(label);
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width().max(60.0), HEIGHT), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
        fill(&painter, rect);
        painter.text(
            rect.right_center() - vec2(4.0, 0.0),
            Align2::RIGHT_CENTER,
            text,
            FontId::proportional(10.5),
            ui.visuals().text_color(),
        );
    });
}

/// A level meter for a linear peak (`1` is full scale), in decibels, with a held peak mark and a
/// red clip zone above 0 dB.
pub fn level_meter(ui: &mut Ui, id: Id, label: &str, peak: f32) {
    let theme = Theme::of(ui.ctx());
    let db = to_db(peak);
    let hold = held(ui, id, db, LEVEL_RANGE.0);
    let text = if db <= LEVEL_RANGE.0 {
        "-∞".to_owned()
    } else {
        tr_args("meter.db", &[("value", &format!("{db:.1}"))])
    };
    frame(ui, label, text, |painter, rect| {
        let at = |db: f32| rect.left() + rect.width() * level_position(db);
        let zero = at(0.0);
        let bar = Rect::from_min_max(rect.min, egui::pos2(at(db), rect.bottom()));
        let safe = bar.intersect(Rect::from_min_max(
            rect.min,
            egui::pos2(zero, rect.bottom()),
        ));
        painter.rect_filled(safe, 2.0, theme.accent.gamma_multiply(0.8));
        if db > 0.0 {
            let clip = Rect::from_min_max(
                egui::pos2(zero, rect.top()),
                egui::pos2(at(db), rect.bottom()),
            );
            painter.rect_filled(clip, 0.0, theme.error);
        }
        let mark = at(hold);
        let color = if hold > 0.0 {
            theme.error
        } else {
            Color32::WHITE
        };
        painter.line_segment(
            [
                egui::pos2(mark, rect.top()),
                egui::pos2(mark, rect.bottom()),
            ],
            egui::Stroke::new(1.5, color),
        );
        painter.line_segment(
            [
                egui::pos2(zero, rect.top()),
                egui::pos2(zero, rect.bottom()),
            ],
            egui::Stroke::new(1.0, theme.text_dim),
        );
    });
}

/// A gain-reduction meter for decibels of gain taken away (`0` or more); it fills from the
/// right, as is usual.
pub fn gain_reduction_meter(ui: &mut Ui, id: Id, label: &str, reduction_db: f32) {
    let theme = Theme::of(ui.ctx());
    let reduction = reduction_db.clamp(0.0, REDUCTION_RANGE);
    let hold = held(ui, id, reduction, 0.0);
    frame(
        ui,
        label,
        tr_args("meter.db", &[("value", &format!("-{reduction_db:.1}"))]),
        |painter, rect| {
            let width = |db: f32| rect.width() * (db / REDUCTION_RANGE).clamp(0.0, 1.0);
            let bar = Rect::from_min_max(
                egui::pos2(rect.right() - width(reduction), rect.top()),
                rect.max,
            );
            painter.rect_filled(bar, 2.0, theme.warning.gamma_multiply(0.9));
            let mark = rect.right() - width(hold);
            painter.line_segment(
                [
                    egui::pos2(mark, rect.top()),
                    egui::pos2(mark, rect.bottom()),
                ],
                egui::Stroke::new(1.5, Color32::WHITE),
            );
        },
    );
}

/// The label a kind of meter has in the text.
pub fn meter_label(kind: MeterKind) -> &'static str {
    match kind {
        MeterKind::Level => tr("meter.peak"),
        MeterKind::GainReduction => tr("meter.reduction"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_scale_is_zero_db_and_silence_is_the_floor() {
        assert!(to_db(1.0).abs() < 1e-4);
        assert!((to_db(0.5) + 6.02).abs() < 0.01);
        assert_eq!(level_position(-100.0), 0.0);
        assert_eq!(level_position(LEVEL_RANGE.1), 1.0);
        assert!(level_position(0.0) < 1.0);
    }
}
