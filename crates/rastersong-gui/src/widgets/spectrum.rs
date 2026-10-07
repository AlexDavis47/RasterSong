//! A spectrum analyzer: the frequency content of a block of samples on a logarithmic axis.

use eframe::egui::{self, Align2, FontId, Id, Sense, Ui, Vec2, pos2};
use rastersong_engine::Fft;
use rastersong_lang::tr_args;

use crate::theme::Theme;

/// The lowest frequency shown, in Hz.
const LOWEST: f64 = 20.0;
/// The level range shown, in decibels relative to a full-scale sine.
const FLOOR_DB: f32 = -90.0;
/// The most samples one transform looks at, and the fewest worth looking at.
const MAX_POINTS: usize = 4096;
const MIN_POINTS: usize = 32;
/// How much of the way to a new reading the display moves each time: smoothing between updates.
const SMOOTHING: f32 = 0.45;
/// Levels with a line and a label, in decibels.
const LEVEL_LINES: [f32; 4] = [-20.0, -40.0, -60.0, -80.0];
/// Frequencies with a line and a label, in Hz.
const FREQUENCY_LINES: [f64; 3] = [100.0, 1_000.0, 10_000.0];

/// The mono mix of interleaved `channels`.
fn mono(samples: &[f32], channels: usize) -> Vec<f32> {
    let channels = channels.max(1);
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn db(magnitude: f32) -> f32 {
    (20.0 * magnitude.max(1e-9).log10()).max(FLOOR_DB)
}

/// The position of `frequency` along a logarithmic axis from `low` to `high`, `0..=1`.
fn position(frequency: f64, low: f64, high: f64) -> f64 {
    ((frequency / low).ln() / (high / low).ln()).clamp(0.0, 1.0)
}

/// The level in decibels for each of `columns` columns of a log-frequency axis from `low` to
/// `high` Hz, from the bins of a transform of `points` samples at `rate`. A column takes the
/// loudest bin it covers, or the level interpolated between the two nearest bins when it is
/// narrower than a bin.
fn columns(bins: &[f32], points: usize, rate: f64, low: f64, high: f64, count: usize) -> Vec<f32> {
    let per_bin = rate / points as f64;
    let at = |frequency: f64| frequency / per_bin;
    (0..count)
        .map(|c| {
            let edge = |i: usize| low * (high / low).powf(i as f64 / count as f64);
            let (from, to) = (at(edge(c)), at(edge(c + 1)));
            let level = if to - from >= 1.0 {
                let (a, b) = (
                    from.floor() as usize,
                    (to.ceil() as usize).min(bins.len() - 1),
                );
                bins[a.min(b)..=b].iter().copied().fold(0.0, f32::max)
            } else {
                let mid = (from + to) * 0.5;
                let i = (mid.floor() as usize).min(bins.len() - 1);
                let next = (i + 1).min(bins.len() - 1);
                let frac = (mid - i as f64) as f32;
                bins[i] + (bins[next] - bins[i]) * frac
            };
            db(level)
        })
        .collect()
}

/// What the widget keeps between frames, under its id.
#[derive(Clone)]
struct State {
    fft: Fft,
    /// The displayed level of each column, smoothed over updates.
    smoothed: Vec<f32>,
    /// The samples the last reading was made from, so the smoothing only advances on new data.
    reading: Vec<f32>,
}

/// Draws the spectrum of `samples` (`channels` interleaved, mixed to mono) taken at `rate`
/// samples a second, in a box of `size`. `id` keeps the display's smoothing: the same widget
/// must pass the same id each frame.
pub fn spectrum(ui: &mut Ui, id: Id, samples: &[f32], channels: usize, rate: f64, size: Vec2) {
    let theme = Theme::of(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    let mix = mono(samples, channels);
    if mix.len() < MIN_POINTS || rate <= 0.0 || !rate.is_finite() {
        return;
    }
    let points = (1usize << mix.len().ilog2()).min(MAX_POINTS);
    let high = rate / 2.0;
    let low = LOWEST.max(rate / points as f64).min(high / 2.0);
    let count = rect.width().max(1.0) as usize;

    let key = id.with("spectrum");
    let mut state = ui
        .data(|d| d.get_temp::<State>(key))
        .filter(|s| s.fft.size() == points)
        .unwrap_or_else(|| State {
            fft: Fft::new(points),
            smoothed: Vec::new(),
            reading: Vec::new(),
        });
    // The latest `points` samples are the ones heard last.
    let window = &mix[mix.len() - points..];
    if state.reading != window || state.smoothed.len() != count {
        let mut bins = Vec::new();
        state.fft.magnitudes(window, &mut bins);
        let levels = columns(&bins, points, rate, low, high, count);
        if state.smoothed.len() == count {
            for (shown, new) in state.smoothed.iter_mut().zip(&levels) {
                *shown += (new - *shown) * SMOOTHING;
            }
            // Still moving toward the reading: keep drawing until it arrives.
            ui.ctx().request_repaint();
        } else {
            state.smoothed = levels;
        }
        state.reading = window.to_vec();
    }

    let y = |level: f32| rect.bottom() - rect.height() * (level - FLOOR_DB) / -FLOOR_DB;
    let font = FontId::proportional(9.5);
    for level in LEVEL_LINES {
        painter.line_segment(
            [pos2(rect.left(), y(level)), pos2(rect.right(), y(level))],
            egui::Stroke::new(1.0, theme.text_dim.gamma_multiply(0.25)),
        );
        painter.text(
            pos2(rect.left() + 3.0, y(level)),
            Align2::LEFT_BOTTOM,
            format!("{level:.0}"),
            font.clone(),
            theme.text_dim,
        );
    }
    for frequency in FREQUENCY_LINES
        .into_iter()
        .filter(|f| (low..high).contains(f))
    {
        let x = rect.left() + rect.width() * position(frequency, low, high) as f32;
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            egui::Stroke::new(1.0, theme.text_dim.gamma_multiply(0.25)),
        );
        let label = if frequency >= 1_000.0 {
            tr_args(
                "spectrum.khz",
                &[("value", &format!("{}", frequency / 1_000.0))],
            )
        } else {
            tr_args("spectrum.hz", &[("value", &format!("{frequency}"))])
        };
        painter.text(
            pos2(x + 3.0, rect.bottom() - 2.0),
            Align2::LEFT_BOTTOM,
            label,
            font.clone(),
            theme.text_dim,
        );
    }

    let points: Vec<egui::Pos2> = state
        .smoothed
        .iter()
        .enumerate()
        .map(|(i, &level)| pos2(rect.left() + i as f32 + 0.5, y(level)))
        .collect();
    // Filled column by column: the outline is not convex.
    for point in &points {
        painter.line_segment(
            [*point, pos2(point.x, rect.bottom())],
            egui::Stroke::new(1.0, theme.accent.gamma_multiply(0.25)),
        );
    }
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(1.2, theme.accent),
    ));
    ui.data_mut(|d| d.insert_temp(key, state));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_shows_in_the_column_at_its_frequency() {
        let rate = 8_000.0;
        let points = 1024;
        let mut fft = Fft::new(points);
        let tone: Vec<f32> = (0..points)
            .map(|i| (std::f64::consts::TAU * 1_000.0 * i as f64 / rate).sin() as f32)
            .collect();
        let mut bins = Vec::new();
        fft.magnitudes(&tone, &mut bins);
        let (low, high, count) = (LOWEST, rate / 2.0, 200);
        let levels = columns(&bins, points, rate, low, high, count);
        let loudest = levels
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        let expected = (position(1_000.0, low, high) * count as f64) as usize;
        assert!(loudest.abs_diff(expected) <= 2, "{loudest} vs {expected}");
        assert!(levels[loudest] > -3.0, "{}", levels[loudest]);
        assert!(levels[10] < -40.0);
    }

    #[test]
    fn stereo_mixes_down_and_silence_sits_on_the_floor() {
        assert_eq!(mono(&[1.0, 3.0, 2.0, 4.0], 2), [2.0, 3.0]);
        assert_eq!(db(0.0), FLOOR_DB);
        assert!((db(1.0)).abs() < 1e-4);
    }
}
