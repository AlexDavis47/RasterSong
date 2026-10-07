//! Level and gain-reduction meters, with peak hold.
//!
//! A meter is a bar and, to its right, a box with the value in decibels. The box has a fixed
//! width and a monospace font, so the number never moves the bar or its neighbours.

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
const HEIGHT: f32 = 16.0;
/// Width of the box holding the decibel value.
const READOUT_WIDTH: f32 = 64.0;
/// The narrowest a bar is drawn when it fills the space it is given.
const MIN_BAR: f32 = 40.0;

/// How a meter is laid out.
#[derive(Debug, Clone, Copy)]
struct Look {
    /// The bar's width, or `None` to fill the space left in the row.
    bar_width: Option<f32>,
    /// Whether a peak mark is held (it needs the meter to be drawn every frame).
    hold: bool,
}

const FILL: Look = Look {
    bar_width: None,
    hold: true,
};

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

/// The bar, then the box with `text`, in one row.
fn bar_and_readout(ui: &mut Ui, look: Look, text: String, fill: impl FnOnce(&egui::Painter, Rect)) {
    let spacing = ui.spacing().item_spacing.x;
    let bar_width = look
        .bar_width
        .unwrap_or_else(|| (ui.available_width() - READOUT_WIDTH - spacing).max(MIN_BAR));
    ui.horizontal(|ui| {
        let (bar, _) = ui.allocate_exact_size(vec2(bar_width, HEIGHT), Sense::hover());
        let painter = ui.painter_at(bar);
        painter.rect_filled(bar, 2.0, ui.visuals().extreme_bg_color);
        fill(&painter, bar);

        let (boxed, _) = ui.allocate_exact_size(vec2(READOUT_WIDTH, HEIGHT), Sense::hover());
        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        ui.painter().rect(
            boxed,
            2.0,
            ui.visuals().extreme_bg_color,
            stroke,
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            boxed.right_center() - vec2(5.0, 0.0),
            Align2::RIGHT_CENTER,
            text,
            FontId::monospace(11.0),
            ui.visuals().text_color(),
        );
    });
}

/// A level meter for a linear peak (`1` is full scale), in decibels, with a held peak mark and a
/// red clip zone above 0 dB.
pub fn level_meter(ui: &mut Ui, id: Id, label: &str, peak: f32) {
    ui.horizontal(|ui| {
        ui.label(label);
        level_bar(ui, id, peak, FILL);
    });
}

/// How a signal's values are shown, chosen from what the signal is said to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// Audio: peak level in decibels.
    Decibels,
    /// Video and other `0..=1` signals: linear, from 0 to 1.
    Unipolar,
    /// Other `-1..=1` signals: linear around a centre line.
    Bipolar,
}

impl Scale {
    /// The scale for a signal's tag: audio is read in decibels, anything else on a linear
    /// scale of its range (a signal with no range is read as `0..=1`).
    pub fn of(tag: &rastersong_engine::Tag) -> Self {
        use rastersong_engine::{Kind, Range};
        match (tag.kind, tag.range) {
            (Kind::Audio, _) => Self::Decibels,
            (_, Range::Bipolar) => Self::Bipolar,
            _ => Self::Unipolar,
        }
    }

    /// The label of the meter row.
    pub fn label(self) -> &'static str {
        match self {
            Self::Decibels => tr("meter.peak"),
            Self::Unipolar | Self::Bipolar => tr("meter.range"),
        }
    }
}

/// A meter of the values from `min` to `max` on `scale`, `width` wide, without peak hold: for
/// tooltips.
pub fn signal_meter(ui: &mut Ui, scale: Scale, min: f32, max: f32, width: f32) {
    let look = Look {
        bar_width: Some(width),
        hold: false,
    };
    if scale == Scale::Decibels {
        level_bar(ui, Id::NULL, min.abs().max(max.abs()), look);
        return;
    }
    let theme = Theme::of(ui.ctx());
    let bipolar = scale == Scale::Bipolar;
    let (low, high) = if bipolar { (-1.0, 1.0) } else { (0.0, 1.0) };
    let outside = min < low - 1e-4 || max > high + 1e-4;
    let text = if bipolar {
        format!("{:.3}", min.abs().max(max.abs()))
    } else {
        format!("{max:.3}")
    };
    bar_and_readout(ui, look, text, |painter, rect| {
        let at = |v: f32| rect.left() + rect.width() * ((v - low) / (high - low)).clamp(0.0, 1.0);
        let (from, to) = if bipolar { (min, max) } else { (0.0, max) };
        let color = if outside {
            theme.error
        } else {
            theme.accent.gamma_multiply(0.8)
        };
        let left = at(from.min(to));
        let right = at(from.max(to)).max(left + 1.5);
        painter.rect_filled(
            Rect::from_min_max(
                egui::pos2(left, rect.top()),
                egui::pos2(right, rect.bottom()),
            ),
            2.0,
            color,
        );
        if bipolar {
            let zero = at(0.0);
            painter.line_segment(
                [
                    egui::pos2(zero, rect.top()),
                    egui::pos2(zero, rect.bottom()),
                ],
                egui::Stroke::new(1.0, theme.text_dim),
            );
        }
    });
}

fn level_bar(ui: &mut Ui, id: Id, peak: f32, look: Look) {
    let theme = Theme::of(ui.ctx());
    let db = to_db(peak);
    let hold = if look.hold {
        held(ui, id, db, LEVEL_RANGE.0)
    } else {
        db
    };
    let text = if db <= LEVEL_RANGE.0 {
        "-∞".to_owned()
    } else {
        tr_args("meter.db", &[("value", &format!("{db:.1}"))])
    };
    bar_and_readout(ui, look, text, |painter, rect| {
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
        if look.hold {
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
        }
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
    ui.horizontal(|ui| {
        ui.label(label);
        bar_and_readout(
            ui,
            FILL,
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
    });
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
