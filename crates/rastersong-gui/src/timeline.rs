//! The timeline: a ruler, the video track (thumbnails and rendered frames) and any number of
//! audio tracks (waveforms, each with an offset), with Reaper-style track headers on the left.
//!
//! - The scroll wheel zooms time around the pointer (over the headers it scrolls the tracks);
//!   middle- or right-drag pans in both directions; F fits the whole video.
//! - Click or drag on the ruler or empty lane space to seek; drag an audio block to move it.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use eframe::egui::{
    self, Align2, CornerRadius, FontId, Key, PointerButton, Rect, Sense, Stroke, TextureId, Ui,
    UiBuilder, Vec2, pos2, vec2,
};
use rastersong_engine::Waveform;

use crate::theme::Theme;

/// Width of the track header column.
pub const HEADER_WIDTH: f32 = 220.0;
/// Gap between the headers and the lanes.
const GAP: f32 = 6.0;
const RULER_HEIGHT: f32 = 22.0;
pub const LANE_HEIGHT: f32 = 48.0;
/// Width of the button that fits the whole video.
const FIT_WIDTH: f32 = 44.0;
/// Height of the row holding the "add track" button.
const ADD_ROW_HEIGHT: f32 = 34.0;
/// Smallest spacing between labelled ruler ticks, in pixels.
const MIN_TICK_SPACING: f64 = 90.0;
/// How far out the view can zoom, as a fraction of the zoom that fits the whole video.
const MIN_ZOOM_OF_FIT: f64 = 0.5;
/// How far in the view can zoom: this many frames across the lanes.
const MIN_VISIBLE_FRAMES: f64 = 4.0;
/// Most thumbnails asked for at once.
const MAX_THUMBNAIL_REQUEST: usize = 200;

/// One audio track as the timeline shows it.
#[derive(Debug, Clone)]
pub struct TrackView {
    pub name: String,
    /// Length in seconds, once decoded.
    pub duration: Option<f64>,
    pub offset: f64,
    pub muted: bool,
    pub waveform: Option<Arc<Waveform>>,
}

/// A thumbnail ready to draw.
#[derive(Debug, Clone, Copy)]
pub struct Thumbnail {
    pub texture: TextureId,
    pub size: Vec2,
}

/// What the timeline shows.
#[derive(Debug, Clone)]
pub struct TimelineModel<'a> {
    pub frame_count: usize,
    pub frame_rate: f64,
    pub playhead: usize,
    pub cached: &'a [Range<usize>],
    pub video_name: Option<String>,
    /// e.g. "1920×1080 · 29.97 fps".
    pub video_details: Option<String>,
    pub tracks: Vec<TrackView>,
    pub selected_track: Option<usize>,
    /// Source thumbnails decoded so far, by frame.
    pub thumbnails: &'a BTreeMap<usize, Thumbnail>,
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
    /// Frames whose thumbnails would fill the visible part of the video track.
    pub wanted_thumbnails: Vec<usize>,
}

/// The zoom and scroll of the timeline. Kept by the app between frames.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TimelineView {
    /// Zoom; zero until the view is first fitted to a video.
    pub px_per_sec: f64,
    /// The time at the left edge of the lanes, in seconds.
    pub left: f64,
    /// Vertical scroll of the tracks, in pixels.
    pub scroll_y: f32,
}

impl TimelineView {
    /// Screen x of `seconds`, for lanes starting at `lanes_left`.
    pub fn x(&self, lanes_left: f32, seconds: f64) -> f32 {
        lanes_left + ((seconds - self.left) * self.px_per_sec) as f32
    }

    /// The time at screen x.
    pub fn seconds(&self, lanes_left: f32, x: f32) -> f64 {
        self.left + f64::from(x - lanes_left) / self.px_per_sec
    }

    /// Shows all of `duration` seconds across `width` pixels.
    pub fn fit(&mut self, width: f32, duration: f64) {
        self.px_per_sec = f64::from(width.max(1.0)) / duration.max(1e-6);
        self.left = 0.0;
    }

    /// Zooms by `factor`, keeping the time at `anchor_x` (relative to the lanes' left) in place.
    pub fn zoom_around(&mut self, factor: f64, anchor_x: f32, limits: (f64, f64)) {
        let anchor = self.left + f64::from(anchor_x) / self.px_per_sec;
        self.px_per_sec = (self.px_per_sec * factor).clamp(limits.0, limits.1);
        self.left = anchor - f64::from(anchor_x) / self.px_per_sec;
    }

    /// Keeps the zoom within `limits` and the visible time within `extent` (seconds) where it
    /// fits, so the content can't be scrolled out of sight.
    pub fn clamp(&mut self, width: f32, limits: (f64, f64), extent: (f64, f64)) {
        self.px_per_sec = self.px_per_sec.clamp(limits.0, limits.1);
        let visible = f64::from(width) / self.px_per_sec;
        let (start, end) = extent;
        // A little room beyond either end.
        let margin = visible * 0.05;
        let min_left = start - margin;
        let max_left = (end + margin - visible).max(min_left);
        self.left = self.left.clamp(min_left, max_left);
    }
}

/// Labelled ruler tick spacing and the number of unlabelled divisions between labels, for a
/// zoom of `px_per_sec`. Steps go down to single frames.
pub fn tick_steps(px_per_sec: f64, frame_rate: f64) -> (f64, u32) {
    let min = MIN_TICK_SPACING / px_per_sec.max(1e-9);
    let frame = 1.0 / frame_rate.max(1e-6);
    let candidates = [
        (frame, 1),
        (2.0 * frame, 2),
        (5.0 * frame, 5),
        (10.0 * frame, 2),
        (0.5, 5),
        (1.0, 5),
        (2.0, 4),
        (5.0, 5),
        (10.0, 5),
        (15.0, 3),
        (30.0, 6),
        (60.0, 6),
        (120.0, 4),
        (300.0, 5),
        (600.0, 5),
        (1200.0, 4),
        (1800.0, 6),
        (3600.0, 6),
    ];
    candidates
        .into_iter()
        .find(|&(step, _)| step >= min)
        .unwrap_or((7200.0, 4))
}

/// `seconds` as `m:ss.cc`.
pub fn timecode(seconds: f64) -> String {
    let total = seconds.max(0.0);
    let minutes = (total / 60.0).floor();
    format!("{}:{:05.2}", minutes as u64, total - minutes * 60.0)
}

/// Frames to show thumbnails of: one per slot of `slot_secs`, on a grid of power-of-two frame
/// steps so the set barely changes as the view zooms. Returns `(step, frames)`.
pub fn thumbnail_frames(
    visible: (f64, f64),
    slot_secs: f64,
    frame_rate: f64,
    frame_count: usize,
) -> (usize, Vec<usize>) {
    let frames_per_slot = (slot_secs * frame_rate).max(1.0);
    let step = (frames_per_slot.log2().ceil().exp2() as usize).max(1);
    let first_visible = (visible.0 * frame_rate).floor().max(0.0) as usize;
    let first = (first_visible / step).saturating_sub(1) * step;
    let last = ((visible.1 * frame_rate).ceil().max(0.0) as usize).min(frame_count);
    let frames = (first..last)
        .step_by(step)
        .take(MAX_THUMBNAIL_REQUEST)
        .collect();
    (step, frames)
}

/// The rectangles a timeline divides into.
struct Areas {
    headers: Rect,
    lanes: Rect,
    ruler: Rect,
    /// Below the ruler, headers and lanes: the part that scrolls vertically.
    body: Rect,
}

impl Areas {
    fn new(area: Rect) -> Self {
        let headers = Rect::from_min_max(
            pos2(area.left(), area.top() + RULER_HEIGHT),
            pos2(area.left() + HEADER_WIDTH, area.bottom()),
        );
        let lanes = Rect::from_min_max(pos2(headers.right() + GAP, area.top()), area.max);
        let ruler = Rect::from_min_max(lanes.min, pos2(lanes.right(), area.top() + RULER_HEIGHT));
        let body = Rect::from_min_max(pos2(area.left(), ruler.bottom()), area.max);
        Self {
            headers,
            lanes,
            ruler,
            body,
        }
    }

    fn row_top(&self, row: usize, scroll: f32) -> f32 {
        self.body.top() + row as f32 * LANE_HEIGHT - scroll
    }

    /// Lane `row` (0 is the video) in screen space, before clipping.
    fn lane(&self, row: usize, scroll: f32) -> Rect {
        Rect::from_min_size(
            pos2(self.lanes.left(), self.row_top(row, scroll)),
            vec2(self.lanes.width(), LANE_HEIGHT),
        )
        .shrink2(vec2(0.0, 2.0))
    }

    fn header(&self, row: usize, scroll: f32) -> Rect {
        Rect::from_min_size(
            pos2(self.headers.left(), self.row_top(row, scroll)),
            vec2(HEADER_WIDTH, LANE_HEIGHT),
        )
        .shrink2(vec2(0.0, 2.0))
    }

    /// Which row (0 is the video) is at screen y, if any.
    fn row_at(&self, scroll: f32, y: f32) -> Option<usize> {
        let offset = y - self.body.top() + scroll;
        (offset >= 0.0).then(|| (offset / LANE_HEIGHT) as usize)
    }
}

pub fn timeline(
    ui: &mut Ui,
    model: &TimelineModel,
    view: &mut TimelineView,
    track_names: &mut [String],
) -> TimelineResponse {
    let mut response = TimelineResponse::default();
    let theme = Theme::of(ui.ctx());
    let area = ui.available_rect_before_wrap();
    let background = ui.allocate_rect(area, Sense::click_and_drag());
    let areas = Areas::new(area);
    let lanes_left = areas.lanes.left();
    let width = areas.lanes.width().max(1.0);

    let duration = (model.frame_count as f64 / model.frame_rate).max(1e-6);
    let extent = model.tracks.iter().fold((0.0f64, duration), |(lo, hi), t| {
        let length = t.duration.unwrap_or(0.0);
        (lo.min(t.offset), hi.max(t.offset + length))
    });
    let fit_zoom = f64::from(width) / duration;
    let limits = (
        fit_zoom * MIN_ZOOM_OF_FIT,
        (f64::from(width) * model.frame_rate / MIN_VISIBLE_FRAMES).max(fit_zoom),
    );
    if view.px_per_sec <= 0.0 {
        view.fit(width, duration);
    }

    // Zoom, pan and fit.
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let over = |r: Rect| pointer.is_some_and(|p| r.contains(p));
    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
    if scroll != 0.0 && background.hovered() {
        if over(areas.lanes) {
            let anchor = pointer.map_or(0.0, |p| p.x - lanes_left);
            view.zoom_around(f64::from(scroll * 0.0015).exp(), anchor, limits);
        } else if over(areas.headers) {
            view.scroll_y -= scroll;
        }
    }
    if background.dragged_by(PointerButton::Middle)
        || background.dragged_by(PointerButton::Secondary)
    {
        let delta = background.drag_delta();
        view.left -= f64::from(delta.x) / view.px_per_sec;
        view.scroll_y -= delta.y;
    }
    let fit_button = Rect::from_min_size(
        pos2(area.left() + HEADER_WIDTH - FIT_WIDTH, area.top() + 1.0),
        vec2(FIT_WIDTH, RULER_HEIGHT - 2.0),
    );
    let fit_clicked = ui
        .put(
            fit_button,
            egui::Button::new(egui::RichText::new("Fit").small()),
        )
        .on_hover_text("Show the whole video (F)")
        .clicked();
    let fit_key =
        over(area) && !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(Key::F));
    if fit_clicked || fit_key {
        view.fit(width, duration);
    }
    view.clamp(width, limits, extent);
    let rows = 1 + model.tracks.len();
    let content = rows as f32 * LANE_HEIGHT + ADD_ROW_HEIGHT;
    view.scroll_y = view
        .scroll_y
        .clamp(0.0, (content - areas.body.height()).max(0.0));
    let x = |seconds: f64| view.x(lanes_left, seconds);

    // Seeking: a primary click or drag that started on the ruler or empty lane space.
    // (A drag still knows where it started; a click is over by now, so use where it was.)
    let pressed_in_lanes = ui
        .input(|i| i.pointer.press_origin())
        .or(background.interact_pointer_pos())
        .is_some_and(|p| areas.lanes.contains(p));
    if (background.clicked() || background.dragged_by(PointerButton::Primary))
        && pressed_in_lanes
        && let Some(p) = background.interact_pointer_pos()
    {
        let frame = (view.seconds(lanes_left, p.x) * model.frame_rate).floor();
        response.seek = Some((frame.max(0.0) as usize).min(model.frame_count.saturating_sub(1)));
        if background.clicked()
            && let Some(row) = areas.row_at(view.scroll_y, p.y)
            && (1..=model.tracks.len()).contains(&row)
        {
            response.actions.push(TrackAction::Select(row - 1));
        }
    }

    // Ruler and tick lines.
    let (major, divisions) = tick_steps(view.px_per_sec, model.frame_rate);
    let minor = major / f64::from(divisions);
    let body_lanes = Rect::from_min_max(pos2(lanes_left, areas.body.top()), areas.body.max);
    let painter = ui.painter_at(areas.ruler.union(body_lanes));
    let first = (view.left / minor).floor() as i64;
    let last = (view.seconds(lanes_left, areas.lanes.right()) / minor).ceil() as i64;
    for k in first..=last {
        let t = k as f64 * minor;
        let tx = x(t);
        let is_major = k.rem_euclid(i64::from(divisions)) == 0;
        let tick_top = if is_major { 8.0 } else { 15.0 };
        painter.line_segment(
            [
                pos2(tx, areas.ruler.top() + tick_top),
                pos2(tx, areas.ruler.bottom()),
            ],
            Stroke::new(1.0, theme.tick),
        );
        let strength = if is_major { 0.28 } else { 0.12 };
        painter.line_segment(
            [pos2(tx, body_lanes.top()), pos2(tx, body_lanes.bottom())],
            Stroke::new(1.0, theme.tick.gamma_multiply(strength)),
        );
        if is_major && t >= -1e-9 {
            painter.text(
                pos2(tx + 3.0, areas.ruler.top() + 1.0),
                Align2::LEFT_TOP,
                timecode(t),
                FontId::proportional(10.5),
                theme.tick_label,
            );
        }
    }

    // Lanes, clipped to the scrolling body.
    let lane_painter = ui.painter_at(body_lanes);
    let video = areas.lane(0, view.scroll_y);
    video_lane(&lane_painter, model, view, video, theme, &mut response);
    for (i, track) in model.tracks.iter().enumerate() {
        let lane = AudioLane {
            track,
            index: i,
            rect: areas.lane(i + 1, view.scroll_y),
            clip: body_lanes,
        };
        lane.show(ui, &lane_painter, model, view, theme, &mut response);
    }

    // Headers.
    let header_clip = Rect::from_min_max(
        areas.headers.min,
        pos2(areas.headers.right(), area.bottom()),
    );
    let header_painter = ui.painter_at(header_clip);
    let rect = areas.header(0, view.scroll_y);
    header_painter.rect_filled(rect, CornerRadius::same(3), ui.visuals().faint_bg_color);
    header(ui, rect, header_clip, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("▣ Video").strong());
                if let Some(name) = &model.video_name {
                    ui.add(egui::Label::new(egui::RichText::new(name).weak()).truncate());
                }
            });
            if let Some(details) = &model.video_details {
                ui.add(egui::Label::new(egui::RichText::new(details).weak().small()).truncate());
            }
        });
    });
    for (i, track) in model.tracks.iter().enumerate() {
        let rect = areas.header(i + 1, view.scroll_y);
        let fill = if model.selected_track == Some(i) {
            theme.accent.gamma_multiply(0.18)
        } else {
            ui.visuals().faint_bg_color
        };
        header_painter.rect_filled(rect, CornerRadius::same(3), fill);
        header(ui, rect, header_clip, |ui| {
            audio_header(ui, track, i, &mut track_names[i], &mut response);
        });
    }
    let add_row = Rect::from_min_size(
        pos2(areas.headers.left(), areas.row_top(rows, view.scroll_y)),
        vec2(HEADER_WIDTH, ADD_ROW_HEIGHT),
    );
    header(ui, add_row, header_clip, |ui| {
        if ui
            .button("+ Audio track")
            .on_hover_text("Add an audio file as a new track")
            .clicked()
        {
            response.actions.push(TrackAction::Add);
        }
    });

    // The playhead, across the ruler and every lane.
    let px = x(model.playhead as f64 / model.frame_rate);
    if areas.lanes.x_range().contains(px) {
        let painter = ui.painter_at(areas.lanes.expand(2.0));
        painter.line_segment(
            [pos2(px, areas.lanes.top()), pos2(px, areas.lanes.bottom())],
            Stroke::new(2.0, theme.playhead),
        );
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(px - 5.0, areas.lanes.top()),
                pos2(px + 5.0, areas.lanes.top()),
                pos2(px, areas.lanes.top() + 6.0),
            ],
            theme.playhead,
            Stroke::NONE,
        ));
    }
    response
}

fn video_lane(
    painter: &egui::Painter,
    model: &TimelineModel,
    view: &TimelineView,
    lane: Rect,
    theme: &Theme,
    response: &mut TimelineResponse,
) {
    let x = |seconds: f64| view.x(lane.left(), seconds);
    let duration = model.frame_count as f64 / model.frame_rate;
    let clip = Rect::from_min_max(pos2(x(0.0), lane.top()), pos2(x(duration), lane.bottom()));
    painter.rect_filled(clip, CornerRadius::same(3), theme.video_block);

    // Thumbnails: one per slot, each the nearest thumbnail decoded so far.
    let strip = Rect::from_min_max(
        pos2(clip.left(), clip.top() + 3.0),
        pos2(clip.right(), clip.bottom() - 7.0),
    );
    let aspect = model
        .thumbnails
        .values()
        .next()
        .map_or(16.0 / 9.0, |t| t.size.x / t.size.y.max(1.0));
    let slot_px = f64::from(strip.height() * aspect);
    let visible = (
        view.seconds(lane.left(), lane.left()),
        view.seconds(lane.left(), lane.right()),
    );
    let (step, frames) = thumbnail_frames(
        visible,
        slot_px / view.px_per_sec,
        model.frame_rate,
        model.frame_count,
    );
    for &frame in &frames {
        let Some(thumbnail) = nearest(model.thumbnails, frame) else {
            continue;
        };
        let start = x(frame as f64 / model.frame_rate);
        let end = x((frame + step).min(model.frame_count) as f64 / model.frame_rate);
        let slot = Rect::from_min_max(pos2(start, strip.top()), pos2(end, strip.bottom()))
            .shrink2(vec2(0.5, 0.0));
        // Fill the slot, cropping whichever way the image overflows it.
        let ratio = slot.width() / (slot.height() * aspect);
        let uv = if ratio >= 1.0 {
            let shown = 1.0 / ratio;
            Rect::from_min_max(pos2(0.0, 0.5 - shown / 2.0), pos2(1.0, 0.5 + shown / 2.0))
        } else {
            Rect::from_min_max(pos2(0.5 - ratio / 2.0, 0.0), pos2(0.5 + ratio / 2.0, 1.0))
        };
        painter.image(thumbnail.texture, slot, uv, egui::Color32::WHITE);
    }
    response.wanted_thumbnails = frames;

    // Rendered frames along the bottom.
    for range in model.cached {
        let x0 = x(range.start as f64 / model.frame_rate);
        let x1 = x(range.end as f64 / model.frame_rate);
        let bar = Rect::from_min_max(
            pos2(x0, clip.bottom() - 4.0),
            pos2(x1.max(x0 + 1.0), clip.bottom()),
        );
        painter.rect_filled(bar, CornerRadius::ZERO, theme.cached);
    }
}

/// The decoded thumbnail closest to `frame`.
fn nearest(thumbnails: &BTreeMap<usize, Thumbnail>, frame: usize) -> Option<Thumbnail> {
    let after = thumbnails.range(frame..).next();
    let before = thumbnails.range(..frame).next_back();
    match (before, after) {
        (Some(b), Some(a)) => Some(if frame - b.0 < a.0 - frame {
            *b.1
        } else {
            *a.1
        }),
        (Some(only), None) | (None, Some(only)) => Some(*only.1),
        (None, None) => None,
    }
}

/// One audio track's lane.
struct AudioLane<'a> {
    track: &'a TrackView,
    index: usize,
    rect: Rect,
    /// The visible part of the lanes.
    clip: Rect,
}

impl AudioLane<'_> {
    fn show(
        &self,
        ui: &Ui,
        painter: &egui::Painter,
        model: &TimelineModel,
        view: &TimelineView,
        theme: &Theme,
        response: &mut TimelineResponse,
    ) {
        let (track, index, lane) = (self.track, self.index, self.rect);
        if model.selected_track == Some(index) {
            painter.rect_stroke(
                lane,
                CornerRadius::same(3),
                Stroke::new(1.0, theme.accent),
                egui::StrokeKind::Inside,
            );
        }
        let Some(length) = track.duration else {
            painter.text(
                lane.left_center() + vec2(6.0, 0.0),
                Align2::LEFT_CENTER,
                "loading…",
                FontId::proportional(11.0),
                ui.visuals().weak_text_color(),
            );
            return;
        };
        let block = Rect::from_min_max(
            pos2(view.x(lane.left(), track.offset), lane.top() + 1.0),
            pos2(
                view.x(lane.left(), track.offset + length),
                lane.bottom() - 1.0,
            ),
        );
        let visible = block.intersect(self.clip);
        if visible.width() <= 0.0 || visible.height() <= 0.0 {
            return;
        }
        let drag = ui.interact(
            visible,
            ui.id().with(("audio-block", index)),
            Sense::click_and_drag(),
        );
        let hovered = drag.hovered() || drag.dragged();
        let mut fill = if hovered {
            theme.audio_block.gamma_multiply(1.2)
        } else {
            theme.audio_block
        };
        if track.muted {
            fill = fill.gamma_multiply(0.4);
        }
        painter.rect_filled(block, CornerRadius::same(3), fill);

        // The waveform: one min/max line per pixel column of the visible part.
        if let Some(waveform) = &track.waveform {
            let columns = visible.width().ceil() as usize;
            let start = view.seconds(lane.left(), visible.left()) - track.offset;
            let end = view.seconds(lane.left(), visible.left() + columns as f32) - track.offset;
            let mut peaks = Vec::with_capacity(columns);
            waveform.peaks(start, end, columns, &mut peaks);
            let mid = block.center().y;
            let half = block.height() / 2.0 - 3.0;
            let color = theme.block_text.gamma_multiply(0.55);
            let mut previous: Option<(f32, f32)> = None;
            for (c, &(lo, hi)) in peaks.iter().enumerate() {
                // Reach the previous column's range, so steep slopes draw as a continuous line.
                let (lo, hi) = match previous {
                    Some((prev_lo, prev_hi)) => (lo.min(prev_hi), hi.max(prev_lo)),
                    None => (lo, hi),
                };
                previous = Some(peaks[c]);
                let px = visible.left() + c as f32 + 0.5;
                let top = mid - hi.clamp(-1.0, 1.0) * half;
                let bottom = (mid - lo.clamp(-1.0, 1.0) * half).max(top + 1.0);
                painter.line_segment([pos2(px, top), pos2(px, bottom)], Stroke::new(1.0, color));
            }
        }
        painter.text(
            visible.left_top() + vec2(5.0, 2.0),
            Align2::LEFT_TOP,
            &track.name,
            FontId::proportional(10.5),
            theme.block_text,
        );

        if drag.drag_started_by(PointerButton::Primary) || drag.clicked() {
            response.actions.push(TrackAction::Select(index));
        }
        if drag.dragged_by(PointerButton::Primary) {
            let delta = f64::from(drag.drag_delta().x) / view.px_per_sec;
            response
                .actions
                .push(TrackAction::SetOffset(index, track.offset + delta));
        }
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        drag.on_hover_text("Drag to move this track against the video");
    }
}

/// The widgets of an audio track's header: name, mute and remove; then the offset.
fn audio_header(
    ui: &mut Ui,
    track: &TrackView,
    index: usize,
    name: &mut String,
    response: &mut TimelineResponse,
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.horizontal(|ui| {
            ui.label("♪");
            let edit = ui
                .add(egui::TextEdit::singleline(name).desired_width(HEADER_WIDTH - 96.0))
                .on_hover_text("Track name, used by Audio Input nodes");
            if edit.gained_focus() {
                response.actions.push(TrackAction::Select(index));
            }
            if edit.lost_focus() && *name != track.name {
                response
                    .actions
                    .push(TrackAction::Rename(index, name.trim().to_owned()));
            }
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
                response.actions.push(TrackAction::ToggleMute(index));
            }
            if ui
                .add(egui::Button::new("×").frame(false))
                .on_hover_text("Remove track")
                .clicked()
            {
                response.actions.push(TrackAction::Remove(index));
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.weak("Offset");
            let mut offset = track.offset;
            let edit = ui
                .add(
                    egui::DragValue::new(&mut offset)
                        .speed(0.01)
                        .suffix(" s")
                        .max_decimals(3),
                )
                .on_hover_text("Seconds this track starts after the video");
            if edit.changed() {
                response.actions.push(TrackAction::SetOffset(index, offset));
            }
        });
    });
}

/// Lays out a header's widgets inside `rect`, clipped to `clip`.
fn header(ui: &mut Ui, rect: Rect, clip: Rect, contents: impl FnOnce(&mut Ui)) {
    if !rect.intersects(clip) {
        return;
    }
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(6.0, 3.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(clip.intersect(ui.clip_rect()));
    contents(&mut child);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_maps_time_and_zooms_around_the_pointer() {
        let mut view = TimelineView::default();
        view.fit(300.0, 3.0);
        assert_eq!(view.px_per_sec, 100.0);
        assert_eq!(view.x(100.0, 1.5), 250.0);
        assert_eq!(view.seconds(100.0, 250.0), 1.5);

        let anchor = view.seconds(0.0, 120.0);
        view.zoom_around(2.0, 120.0, (1.0, 10_000.0));
        assert_eq!(view.px_per_sec, 200.0);
        assert!(
            (view.seconds(0.0, 120.0) - anchor).abs() < 1e-9,
            "the time under the pointer stays put"
        );
        view.zoom_around(1000.0, 0.0, (1.0, 10_000.0));
        assert_eq!(view.px_per_sec, 10_000.0, "zoom is limited");
    }

    #[test]
    fn clamping_keeps_the_content_in_view() {
        let mut view = TimelineView {
            px_per_sec: 100.0,
            left: -50.0,
            scroll_y: 0.0,
        };
        // 2 s visible of 0..10 s, with 5% of the view as margin.
        view.clamp(200.0, (1.0, 1000.0), (0.0, 10.0));
        assert!((view.left - -0.1).abs() < 1e-9, "{}", view.left);
        view.left = 50.0;
        view.clamp(200.0, (1.0, 1000.0), (0.0, 10.0));
        assert!((view.left - 8.1).abs() < 1e-9, "{}", view.left);
    }

    #[test]
    fn ticks_stay_readable_down_to_frames() {
        // 1000 px for 10 s: labels every second.
        assert_eq!(tick_steps(100.0, 30.0), (1.0, 5));
        // 1000 px for 10 minutes: every minute.
        assert_eq!(tick_steps(1000.0 / 600.0, 30.0).0, 60.0);
        // Very zoomed in: every frame.
        assert_eq!(tick_steps(10_000.0, 30.0).0, 1.0 / 30.0);
        assert_eq!(tick_steps(1000.0, 30.0).0, 5.0 / 30.0);
    }

    #[test]
    fn thumbnail_frames_sit_on_a_stable_grid() {
        // 30 fps, slots of 0.5 s (15 frames) round up to steps of 16 frames.
        let (step, frames) = thumbnail_frames((0.0, 4.0), 0.5, 30.0, 300);
        assert_eq!(step, 16);
        assert_eq!(frames[..3], [0, 16, 32]);
        assert!(frames.iter().all(|f| f % 16 == 0 && *f <= 120));
        // A slightly different zoom asks for the same frames.
        let (_, again) = thumbnail_frames((0.0, 4.0), 0.52, 30.0, 300);
        assert_eq!(again, frames);
        // Nothing past the end.
        let (_, tail) = thumbnail_frames((9.0, 12.0), 0.5, 30.0, 300);
        assert!(tail.iter().all(|&f| f < 300));
    }

    #[test]
    fn formats_timecodes() {
        assert_eq!(timecode(0.0), "0:00.00");
        assert_eq!(timecode(61.5), "1:01.50");
        assert_eq!(timecode(-3.0), "0:00.00");
    }

    #[test]
    fn nearest_thumbnail_is_used_until_the_exact_one_arrives() {
        let thumb = |id| Thumbnail {
            texture: TextureId::User(id),
            size: vec2(16.0, 9.0),
        };
        let thumbnails = BTreeMap::from([(0, thumb(0)), (32, thumb(32))]);
        assert_eq!(
            nearest(&thumbnails, 10).unwrap().texture,
            TextureId::User(0)
        );
        assert_eq!(
            nearest(&thumbnails, 20).unwrap().texture,
            TextureId::User(32)
        );
        assert!(nearest(&BTreeMap::new(), 5).is_none());
    }
}
