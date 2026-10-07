//! The Look tool's popup: what a connection carries at the playhead, as a picture for video-like
//! signals and a scope for audio-like ones.

use std::sync::Arc;

use eframe::egui::{self, Color32, Id, Rect, Sense, TextureHandle, TextureOptions, Ui, vec2};
use rastersong_engine::{Tap, TapOutcome, TapRequest};
use rastersong_lang::tr;

use crate::theme::Theme;

/// How long the pointer rests on a connection before it is looked at, in seconds. Moving across
/// wires asks for nothing.
const DWELL: f64 = 0.15;
/// The soonest a new frame is asked for while the playhead moves, in seconds.
const REFRESH: f64 = 0.2;
/// The popup's width, and the height of a scope.
pub const WIDTH: f32 = 240.0;
const SCOPE_HEIGHT: f32 = 72.0;
/// The shape of a picture before there is one to measure.
const DEFAULT_ASPECT: f32 = 16.0 / 9.0;

/// What the Look tool needs from the app: where the playhead is and how to ask the engine.
pub struct LookContext<'a> {
    pub frame: usize,
    pub tap: &'a dyn Fn(&TapRequest) -> TapOutcome,
}

/// What the popup shows.
#[derive(Clone)]
pub enum LookView {
    /// Nothing yet: the pointer has not rested, or the engine has not answered.
    Waiting {
        audio: bool,
    },
    Picture {
        texture: TextureHandle,
        aspect: f32,
    },
    Scope {
        samples: Arc<[f32]>,
        channels: usize,
    },
    /// The connection is not part of what is rendered.
    NotRendered,
    Failed(String),
}

/// What the tool has worked out about the connection under the pointer.
#[derive(Clone)]
struct LookState {
    target: (String, usize),
    /// When the pointer arrived on the target.
    since: f64,
    /// The frame being shown, and when it was asked for.
    frame: usize,
    asked: f64,
    /// The last thing the engine answered, shown while the next answer is on its way.
    last: Option<Arc<Tap>>,
    texture: Option<(TapRequest, TextureHandle)>,
}

/// Looks at output `output` of node `node`. `audio` is whether the connection is known to carry
/// audio, so the popup has the right shape before the engine has answered.
pub fn look(ui: &Ui, node: &str, output: usize, audio: bool, ctx: &LookContext) -> LookView {
    let id = Id::new("look-state");
    let now = ui.input(|i| i.time);
    let target = (node.to_owned(), output);
    let mut state = ui
        .data(|d| d.get_temp::<LookState>(id))
        .filter(|s| s.target == target)
        .unwrap_or_else(|| LookState {
            target,
            since: now,
            frame: ctx.frame,
            asked: now,
            last: None,
            texture: None,
        });

    let view = if now - state.since < DWELL {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(DWELL));
        LookView::Waiting { audio }
    } else {
        if state.frame != ctx.frame && now - state.asked >= REFRESH {
            state.frame = ctx.frame;
            state.asked = now;
        }
        let request = TapRequest {
            frame: state.frame,
            node: node.to_owned(),
            output,
        };
        match (ctx.tap)(&request) {
            TapOutcome::Ready(tap) => {
                state.last = Some(tap);
                view_of(ui, &mut state, audio)
            }
            TapOutcome::Pending => {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
                view_of(ui, &mut state, audio)
            }
            TapOutcome::NotRendered => LookView::NotRendered,
            TapOutcome::Failed(message) => LookView::Failed(message),
        }
    };
    ui.data_mut(|d| d.insert_temp(id, state));
    view
}

/// The view of the last answer, or waiting if there is none.
fn view_of(ui: &Ui, state: &mut LookState, audio: bool) -> LookView {
    let Some(tap) = state.last.clone() else {
        return LookView::Waiting { audio };
    };
    if let Some(picture) = &tap.picture {
        let current = state
            .texture
            .as_ref()
            .is_some_and(|(request, _)| *request == tap.request);
        if !current {
            let image = egui::ColorImage::from_rgb(
                [picture.width as usize, picture.height as usize],
                &picture.rgb,
            );
            let texture = ui
                .ctx()
                .load_texture("look-picture", image, TextureOptions::NEAREST);
            state.texture = Some((tap.request.clone(), texture));
        }
        if let Some((_, texture)) = &state.texture {
            return LookView::Picture {
                texture: texture.clone(),
                aspect: picture.width as f32 / picture.height.max(1) as f32,
            };
        }
    }
    match &tap.samples {
        Some(samples) => LookView::Scope {
            samples: samples.clone(),
            channels: tap.layout.samples_per_pixel.max(1) as usize,
        },
        None => LookView::Waiting { audio },
    }
}

impl LookView {
    /// Draws the popup's picture or scope in a space of fixed size for its kind.
    pub fn show(&self, ui: &mut Ui) {
        match self {
            Self::Picture { texture, aspect } => {
                let size = vec2(WIDTH, WIDTH / aspect.max(0.1));
                ui.add(egui::Image::new((texture.id(), size)));
            }
            Self::Scope { samples, channels } => scope(ui, samples, *channels),
            Self::Waiting { audio } => {
                let height = if *audio {
                    SCOPE_HEIGHT
                } else {
                    WIDTH / DEFAULT_ASPECT
                };
                let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, height), Sense::hover());
                ui.painter()
                    .rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    tr("editor.tip.look_waiting"),
                    egui::FontId::proportional(12.0),
                    ui.visuals().weak_text_color(),
                );
            }
            Self::NotRendered => {
                ui.label(egui::RichText::new(tr("editor.tip.look_not_rendered")).weak());
            }
            Self::Failed(message) => {
                ui.colored_label(Theme::of(ui.ctx()).error, message);
            }
        }
    }
}

/// The minimum and maximum of channel `channel` of `samples` for each of `columns` columns.
fn envelope(samples: &[f32], channels: usize, channel: usize, columns: usize) -> Vec<(f32, f32)> {
    let frames = samples.len() / channels.max(1);
    (0..columns)
        .map(|column| {
            let from = column * frames / columns.max(1);
            let to = ((column + 1) * frames / columns.max(1))
                .max(from + 1)
                .min(frames);
            samples
                .iter()
                .skip(from * channels + channel)
                .step_by(channels.max(1))
                .take(to.saturating_sub(from))
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &x| {
                    (lo.min(x), hi.max(x))
                })
        })
        .filter(|(lo, hi)| lo <= hi)
        .collect()
}

/// A scope: each channel's waveform over the frame, on a `-1..=1` scale.
fn scope(ui: &mut Ui, samples: &[f32], channels: usize) {
    let theme = Theme::of(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, SCOPE_HEIGHT), Sense::hover());
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
        let color: Color32 = theme.accent;
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
                egui::Stroke::new(1.0, color),
            );
            line.push(egui::pos2(px, y((lo + hi) * 0.5)));
        }
        // Joined up, so a signal with fewer samples than columns is a line and not dots.
        painter.add(egui::Shape::line(line, egui::Stroke::new(1.0, color)));
    }
}

impl std::fmt::Debug for LookContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LookContext")
            .field("frame", &self.frame)
            .finish_non_exhaustive()
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
