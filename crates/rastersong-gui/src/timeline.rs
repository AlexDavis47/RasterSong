//! The timeline: a video track and any number of audio tracks, each with an offset, the rendered
//! frames, and the playhead.

use std::ops::Range;

use eframe::egui::{self, Align2, CornerRadius, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::theme::Theme;

const HEADER_WIDTH: f32 = 250.0;
const RULER_HEIGHT: f32 = 18.0;
const LANE_HEIGHT: f32 = 26.0;

/// One audio track as the timeline shows it.
#[derive(Debug, Clone)]
pub struct TrackView {
    pub name: String,
    /// Length in seconds, once decoded.
    pub duration: Option<f64>,
    pub offset: f64,
    pub muted: bool,
}

/// What the timeline shows.
#[derive(Debug, Clone)]
pub struct TimelineModel<'a> {
    pub frame_count: usize,
    pub frame_rate: f64,
    pub playhead: usize,
    pub cached: &'a [Range<usize>],
    pub video_name: Option<String>,
    pub tracks: Vec<TrackView>,
    pub selected_track: Option<usize>,
}

/// Something the user did to a track.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackAction {
    Select(usize),
    SetOffset(usize, f64),
    ToggleMute(usize),
    Rename(usize, String),
    Remove(usize),
    Add,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimelineResponse {
    pub seek: Option<usize>,
    pub actions: Vec<TrackAction>,
}

/// Converts between seconds and screen x over the lanes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeScale {
    pub left: f32,
    pub width: f32,
    pub duration: f64,
}

impl TimeScale {
    pub fn x(&self, seconds: f64) -> f32 {
        self.left + (seconds / self.duration) as f32 * self.width
    }

    pub fn seconds(&self, x: f32) -> f64 {
        f64::from((x - self.left) / self.width) * self.duration
    }

    /// The frame under `x`, clamped to the video.
    pub fn frame_at(&self, x: f32, frame_rate: f64, frame_count: usize) -> usize {
        let frame = (self.seconds(x) * frame_rate).floor().max(0.0) as usize;
        frame.min(frame_count.saturating_sub(1))
    }
}

/// Spacing for ruler ticks: the smallest "nice" step that keeps labels apart.
fn tick_step(duration: f64, width: f32) -> f64 {
    let min_step = duration * 70.0 / f64::from(width.max(1.0));
    [
        0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0,
    ]
    .into_iter()
    .find(|&s| s >= min_step)
    .unwrap_or(1200.0)
}

pub fn timeline(
    ui: &mut Ui,
    model: &TimelineModel,
    track_names: &mut [String],
) -> TimelineResponse {
    let mut response = TimelineResponse::default();
    let theme = Theme::of(ui.ctx());
    let duration = (model.frame_count as f64 / model.frame_rate).max(1e-6);
    let lanes_left = ui.min_rect().left() + HEADER_WIDTH + 8.0;
    let scale = TimeScale {
        left: lanes_left,
        width: (ui.max_rect().right() - lanes_left - 8.0).max(1.0),
        duration,
    };
    let mut lanes = Rect::NOTHING;

    // Ruler.
    ui.horizontal(|ui| {
        ui.add_space(HEADER_WIDTH + 8.0);
        let (rect, ruler) =
            ui.allocate_exact_size(vec2(scale.width, RULER_HEIGHT), Sense::click_and_drag());
        lanes = lanes.union(rect);
        seek(&ruler, &scale, model, &mut response);
        let painter = ui.painter_at(rect.expand(30.0));
        let step = tick_step(duration, scale.width);
        let mut t = 0.0;
        while t <= duration + 1e-9 {
            let x = scale.x(t);
            painter.line_segment(
                [pos2(x, rect.bottom() - 5.0), pos2(x, rect.bottom())],
                Stroke::new(1.0, theme.tick),
            );
            painter.text(
                pos2(x + 3.0, rect.top()),
                Align2::LEFT_TOP,
                crate::timeline::timecode(t),
                FontId::proportional(10.5),
                theme.tick_label,
            );
            t += step;
        }
    });

    // Video lane.
    ui.horizontal(|ui| {
        header(ui, |ui| {
            ui.label(egui::RichText::new("Video").strong());
            if let Some(name) = &model.video_name {
                ui.add(egui::Label::new(egui::RichText::new(name).weak().small()).truncate());
            }
        });
        let (rect, lane) =
            ui.allocate_exact_size(vec2(scale.width, LANE_HEIGHT), Sense::click_and_drag());
        lanes = lanes.union(rect);
        seek(&lane, &scale, model, &mut response);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same(3), theme.video_block);
        for range in model.cached {
            let x0 = scale.x(range.start as f64 / model.frame_rate);
            let x1 = scale.x(range.end as f64 / model.frame_rate);
            let bar = Rect::from_min_max(
                pos2(x0, rect.bottom() - 4.0),
                pos2(x1.max(x0 + 1.0), rect.bottom()),
            );
            painter.rect_filled(bar, CornerRadius::ZERO, theme.cached);
        }
    });

    // Audio lanes.
    for (i, track) in model.tracks.iter().enumerate() {
        let selected = model.selected_track == Some(i);
        ui.horizontal(|ui| {
            header(ui, |ui| {
                let mute = egui::Button::new(if track.muted { "🔇" } else { "🔊" }).frame(false);
                if ui
                    .add(mute)
                    .on_hover_text(if track.muted {
                        "Unmute"
                    } else {
                        "Mute in playback"
                    })
                    .clicked()
                {
                    response.actions.push(TrackAction::ToggleMute(i));
                }
                let name = &mut track_names[i];
                let edit = egui::TextEdit::singleline(name).desired_width(HEADER_WIDTH - 150.0);
                let edit = ui
                    .add(edit)
                    .on_hover_text("Track name, used by Audio Input nodes");
                if edit.gained_focus() {
                    response.actions.push(TrackAction::Select(i));
                }
                if edit.lost_focus() && *name != track.name {
                    response
                        .actions
                        .push(TrackAction::Rename(i, name.trim().to_owned()));
                }
                let mut offset = track.offset;
                let offset_edit = ui
                    .add(
                        egui::DragValue::new(&mut offset)
                            .speed(0.01)
                            .suffix(" s")
                            .max_decimals(3),
                    )
                    .on_hover_text("Offset: seconds this track starts after the video");
                if offset_edit.changed() {
                    response.actions.push(TrackAction::SetOffset(i, offset));
                }
                if ui
                    .add(egui::Button::new("×").frame(false))
                    .on_hover_text("Remove track")
                    .clicked()
                {
                    response.actions.push(TrackAction::Remove(i));
                }
            });
            let (rect, lane) =
                ui.allocate_exact_size(vec2(scale.width, LANE_HEIGHT), Sense::click_and_drag());
            lanes = lanes.union(rect);
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, CornerRadius::same(3), theme.lane_bg);
            if selected {
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(3),
                    Stroke::new(1.0, theme.accent),
                    egui::StrokeKind::Inside,
                );
            }

            let mut on_block = false;
            if let Some(length) = track.duration {
                let block = Rect::from_min_max(
                    pos2(scale.x(track.offset), rect.top() + 2.0),
                    pos2(scale.x(track.offset + length), rect.bottom() - 2.0),
                );
                let visible = block.intersect(rect);
                if visible.width() > 0.0 {
                    let drag = ui.interact(
                        visible,
                        ui.id().with(("audio-block", i)),
                        Sense::click_and_drag(),
                    );
                    on_block = drag.hovered() || drag.dragged();
                    let mut fill = if on_block {
                        theme.audio_block.gamma_multiply(1.25)
                    } else {
                        theme.audio_block
                    };
                    if track.muted {
                        fill = fill.gamma_multiply(0.4);
                    }
                    painter.rect_filled(visible, CornerRadius::same(3), fill);
                    painter.text(
                        visible.left_center() + vec2(6.0, 0.0),
                        Align2::LEFT_CENTER,
                        &track.name,
                        FontId::proportional(11.0),
                        theme.block_text,
                    );
                    if drag.drag_started() || drag.clicked() {
                        response.actions.push(TrackAction::Select(i));
                    }
                    if drag.dragged() {
                        let delta = f64::from(drag.drag_delta().x / scale.width) * scale.duration;
                        response
                            .actions
                            .push(TrackAction::SetOffset(i, track.offset + delta));
                    }
                    if on_block {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    drag.on_hover_text("Drag to move this track against the video");
                }
            } else {
                painter.text(
                    rect.left_center() + vec2(6.0, 0.0),
                    Align2::LEFT_CENTER,
                    "loading…",
                    FontId::proportional(11.0),
                    ui.visuals().weak_text_color(),
                );
            }
            if !on_block {
                if lane.clicked() || lane.drag_started() {
                    response.actions.push(TrackAction::Select(i));
                }
                seek(&lane, &scale, model, &mut response);
            }
        });
    }

    ui.horizontal(|ui| {
        header(ui, |ui| {
            if ui
                .button("+ Audio track")
                .on_hover_text("Add an audio file as a new track")
                .clicked()
            {
                response.actions.push(TrackAction::Add);
            }
        });
    });

    // The playhead, across every lane.
    if lanes.is_positive() {
        let x = scale.x(model.playhead as f64 / model.frame_rate);
        let painter = ui.painter_at(lanes.expand(2.0));
        painter.line_segment(
            [pos2(x, lanes.top()), pos2(x, lanes.bottom())],
            Stroke::new(2.0, theme.playhead),
        );
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(x - 5.0, lanes.top()),
                pos2(x + 5.0, lanes.top()),
                pos2(x, lanes.top() + 6.0),
            ],
            theme.playhead,
            Stroke::NONE,
        ));
    }
    response
}

/// A fixed-width header cell at the start of a row.
fn header(ui: &mut Ui, contents: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(vec2(HEADER_WIDTH, LANE_HEIGHT), Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    contents(&mut child);
    ui.add_space(8.0);
}

fn seek(
    lane: &egui::Response,
    scale: &TimeScale,
    model: &TimelineModel,
    response: &mut TimelineResponse,
) {
    if (lane.clicked() || lane.dragged())
        && let Some(p) = lane.interact_pointer_pos()
    {
        response.seek = Some(scale.frame_at(p.x, model.frame_rate, model.frame_count));
    }
}

/// `seconds` as `m:ss.cc`.
pub fn timecode(seconds: f64) -> String {
    let total = seconds.max(0.0);
    let minutes = (total / 60.0).floor();
    format!("{}:{:05.2}", minutes as u64, total - minutes * 60.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_time_and_frames() {
        let scale = TimeScale {
            left: 100.0,
            width: 300.0,
            duration: 3.0,
        };
        assert_eq!(scale.x(1.5), 250.0);
        assert_eq!(scale.seconds(250.0), 1.5);
        assert_eq!(scale.frame_at(250.0, 30.0, 90), 45);
        assert_eq!(scale.frame_at(0.0, 30.0, 90), 0, "clamped at the start");
        assert_eq!(scale.frame_at(1000.0, 30.0, 90), 89, "clamped at the end");
    }

    #[test]
    fn formats_timecodes() {
        assert_eq!(timecode(0.0), "0:00.00");
        assert_eq!(timecode(61.5), "1:01.50");
        assert_eq!(timecode(-3.0), "0:00.00");
    }

    #[test]
    fn ruler_ticks_stay_readable() {
        assert_eq!(tick_step(10.0, 1000.0), 1.0);
        assert_eq!(tick_step(600.0, 1000.0), 60.0);
        assert_eq!(tick_step(1.0, 1000.0), 0.1);
    }
}
