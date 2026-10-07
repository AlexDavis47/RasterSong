//! A scope: the waveform of a block of samples.

use eframe::egui::{self, Rect, Sense, Ui, Vec2, vec2};

use crate::theme::Theme;

/// The minimum and maximum of channel `channel` of `samples` for each of `columns` columns.
fn envelope(samples: &[f32], channels: usize, channel: usize, columns: usize) -> Vec<(f32, f32)> {
    let channels = channels.max(1);
    let frames = samples.len() / channels;
    (0..columns)
        .map(|column| {
            let from = column * frames / columns.max(1);
            let to = ((column + 1) * frames / columns.max(1))
                .max(from + 1)
                .min(frames);
            samples
                .iter()
                .skip(from * channels + channel)
                .step_by(channels)
                .take(to.saturating_sub(from))
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &x| {
                    (lo.min(x), hi.max(x))
                })
        })
        .filter(|(lo, hi)| lo <= hi)
        .collect()
}

/// Draws `samples` (`channels` interleaved, at most two shown, one lane each) on a `-1..=1`
/// scale in a box of `size`. Values outside the scale are clipped to the box.
pub fn scope(ui: &mut Ui, samples: &[f32], channels: usize, size: Vec2) {
    let theme = Theme::of(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    let channels = channels.clamp(1, 2);
    let lane = rect.height() / channels as f32;
    for channel in 0..channels {
        let lane_rect = Rect::from_min_size(
            egui::pos2(rect.left(), rect.top() + lane * channel as f32),
            vec2(rect.width(), lane),
        );
        let mid = lane_rect.center().y;
        painter.line_segment(
            [
                egui::pos2(lane_rect.left(), mid),
                egui::pos2(lane_rect.right(), mid),
            ],
            egui::Stroke::new(1.0, theme.text_dim.gamma_multiply(0.5)),
        );
        let columns = rect.width() as usize;
        let y = |v: f32| mid - v.clamp(-1.0, 1.0) * lane * 0.5;
        let mut line = Vec::with_capacity(columns);
        for (x, (lo, hi)) in envelope(samples, channels, channel, columns)
            .into_iter()
            .enumerate()
        {
            let px = rect.left() + x as f32 + 0.5;
            painter.line_segment(
                [
                    egui::pos2(px, y(hi)),
                    egui::pos2(px, y(lo).max(y(hi) + 1.0)),
                ],
                egui::Stroke::new(1.0, theme.accent),
            );
            line.push(egui::pos2(px, y((lo + hi) * 0.5)));
        }
        // Joined up, so a signal with fewer samples than columns is a line and not dots.
        painter.add(egui::Shape::line(
            line,
            egui::Stroke::new(1.0, theme.accent),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_has_a_range_per_column_and_follows_the_channel() {
        // Stereo: left rises 0..3, right is constant 9.
        let samples = [0.0, 9.0, 1.0, 9.0, 2.0, 9.0, 3.0, 9.0];
        let left = envelope(&samples, 2, 0, 2);
        assert_eq!(left, [(0.0, 1.0), (2.0, 3.0)]);
        let right = envelope(&samples, 2, 1, 4);
        assert!(right.iter().all(|&(lo, hi)| lo == 9.0 && hi == 9.0));
        // More columns than samples never makes an empty column.
        assert_eq!(envelope(&[0.5], 1, 0, 8).len(), 8);
    }
}
