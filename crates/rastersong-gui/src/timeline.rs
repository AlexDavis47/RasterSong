//! The timeline: a ruler, the video tracks (thumbnails) and audio tracks (waveforms), each a lane
//! of items, with Reaper-style track headers on the left.
//!
//! - The scroll wheel zooms time around the pointer (over the headers it scrolls the tracks);
//!   middle- or right-drag pans in both directions; F fits the whole project.
//! - Click or drag on the ruler or empty lane space to seek; drag an item by its header bar to
//!   move it (with the other selected items, and the items they overlap on linked tracks).
//! - Click an item's header bar to select it, Ctrl+click to add or remove it; drag an item's
//!   edge to trim it, Alt+drag to change its rate. Drags snap to the grid, item edges and the
//!   playhead while snapping is on; Shift drags freely.
//! - Over the timeline, S splits at the playhead (the selected items, or with none selected
//!   every item under it), Delete removes the selected items, and Ctrl+C, Ctrl+X and Ctrl+V
//!   copy, cut and paste them at the playhead.
//! - Ctrl+drag on the ruler makes a loop region (or moves one of its edges).
//! - Drag a header to reorder audio tracks, its bottom edge to change the track's height;
//!   right-click it to link tracks.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use eframe::egui::{
    self, Align2, CornerRadius, FontId, Key, PointerButton, Rect, Sense, Stroke, TextureId, Ui,
    UiBuilder, Vec2, pos2, vec2,
};
use rastersong_engine::{
    Edge, Item, LoopRegion, Tempo, TimelineMode, TrackKind, Waveform, snap_offset,
};
use rastersong_lang::{tr, tr_args};

use crate::name_edit::name_edit;
use crate::theme::Theme;

/// Width of the track header column.
pub const HEADER_WIDTH: f32 = 220.0;
/// Gap between the headers and the lanes.
const GAP: f32 = 6.0;
/// Width of the name fields in the track headers, leaving room for the buttons beside them.
const NAME_WIDTH: f32 = HEADER_WIDTH - 136.0;
const RULER_HEIGHT: f32 = 22.0;
/// A track's height until the user changes it: tall enough for a header's two rows of widgets.
pub const LANE_HEIGHT: f32 = 58.0;
/// The shortest and tallest a track can be made.
pub const LANE_HEIGHT_RANGE: std::ops::RangeInclusive<f32> = 46.0..=320.0;
/// Height of the bar along the top of each item, which drags it and holds its mute button.
const ITEM_BAR: f32 = 14.0;
/// How close (pixels) to an item's edge the pointer must be to trim it.
const EDGE_GRAB: f32 = 5.0;
/// How close (pixels) a dragged edge must come to a snap target to snap to it.
const SNAP_DISTANCE: f32 = 8.0;
/// Width of the snapping button in the ruler header.
const SNAP_WIDTH: f32 = 52.0;
/// How close (pixels) to the bottom edge of a header the pointer must be to resize the track.
const RESIZE_GRAB: f32 = 3.0;
/// Width of the button that switches the ruler between time and tempo.
const MODE_WIDTH: f32 = 66.0;
/// Height of the row holding the "add track" button.
const ADD_ROW_HEIGHT: f32 = 34.0;
/// Smallest spacing between labelled ruler ticks, in pixels.
const MIN_TICK_SPACING: f64 = 90.0;
/// How far out the view can zoom, as a fraction of the zoom that fits the whole project.
const MIN_ZOOM_OF_FIT: f64 = 0.5;
/// How far in the view can zoom: this many frames across the lanes.
const MIN_VISIBLE_FRAMES: f64 = 4.0;
/// Most thumbnails asked for at once.
const MAX_THUMBNAIL_REQUEST: usize = 200;

/// One track as the timeline shows it.
#[derive(Debug, Clone)]
pub struct TrackView {
    pub name: String,
    pub kind: TrackKind,
    /// Length of the track's file in seconds, once known.
    pub duration: Option<f64>,
    pub items: Vec<Item>,
    pub muted: bool,
    pub solo: bool,
    /// Left out of the track mix, by its mute or another track's solo: drawn dimmed.
    pub silenced: bool,
    /// Level in the track mix, 0 to 1 (audio tracks).
    pub volume: f32,
    /// Height of the track's lane and header.
    pub height: f32,
    /// The other tracks linked to this one.
    pub linked: Vec<String>,
    pub waveform: Option<Arc<Waveform>>,
    /// The output bus an audio track is summed into in the track mix.
    pub bus: String,
    /// A line under the name, e.g. "1920×1080 · 29.97 fps".
    pub details: Option<String>,
    /// Whether the model's thumbnails are of this track's file.
    pub thumbnails: bool,
    /// The selected items, by index into [`Self::items`].
    pub selected_items: Vec<usize>,
}

impl TrackView {
    /// A track of `kind` with nothing loaded yet, at the default height.
    pub fn new(name: impl Into<String>, kind: TrackKind) -> Self {
        Self {
            name: name.into(),
            kind,
            duration: None,
            items: Vec::new(),
            muted: false,
            solo: false,
            silenced: false,
            volume: 1.0,
            height: LANE_HEIGHT,
            linked: Vec::new(),
            waveform: None,
            bus: String::new(),
            details: None,
            thumbnails: false,
            selected_items: Vec::new(),
        }
    }

    /// How long item `item` lasts on the timeline; `None` until the file's length is known.
    fn item_span(&self, item: &Item) -> Option<(f64, f64)> {
        self.duration.map(|d| (item.position, item.timeline_end(d)))
    }
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
    /// Every track, top first: the video tracks, then the audio tracks.
    pub tracks: Vec<TrackView>,
    /// The selected row of [`Self::tracks`].
    pub selected_track: Option<usize>,
    /// Source thumbnails decoded so far, by frame of the file they are of.
    pub thumbnails: &'a BTreeMap<usize, Thumbnail>,
    /// The frame rate of the file the thumbnails are of (0 when there is none).
    pub thumbnail_rate: f64,
    pub loop_region: Option<LoopRegion>,
    /// The project tempo, which the ruler follows in [`TimelineMode::Tempo`].
    pub tempo: Tempo,
    pub mode: TimelineMode,
    /// The project's output buses, master first. Track headers offer them when there are
    /// several.
    pub buses: Vec<String>,
    /// Whether dragged items and edges snap.
    pub snap: bool,
}

/// Something the user did to a track. Tracks are named by their row in
/// [`TimelineModel::tracks`].
#[derive(Debug, Clone, PartialEq)]
pub enum TrackAction {
    Select(usize),
    /// Item `item` of the track was clicked: select only it, or with `toggle` (Ctrl) add it to
    /// the selection or take it out.
    SelectItem {
        row: usize,
        item: usize,
        toggle: bool,
    },
    /// Empty lane space was clicked: select no items.
    ClearItems,
    /// A box was dragged over lane space: select these items (row, item), adding them to the
    /// selection with `additive` (Ctrl or Shift).
    SelectItems {
        items: Vec<(usize, usize)>,
        additive: bool,
    },
    /// Item `item` of the track was dragged `delta` seconds along the timeline, with the other
    /// selected items if it is selected.
    MoveItem {
        row: usize,
        item: usize,
        delta: f64,
    },
    ToggleItemMute {
        row: usize,
        item: usize,
    },
    /// An edge of item `item` was dragged to time `to` (seconds): trimmed, or with `stretch`
    /// (Alt) its rate changed.
    TrimItem {
        row: usize,
        item: usize,
        edge: Edge,
        to: f64,
        stretch: bool,
    },
    /// Split at time `at` (the playhead).
    SplitItems {
        at: f64,
    },
    DeleteItems,
    CopyItems,
    CutItems,
    /// Paste the copied items at time `at` (the playhead). `text` is the system clipboard's text
    /// for a keyboard paste, which pastes only when it is what the item copy put there; the
    /// right-click menu, which can't read the clipboard, has none.
    PasteItems {
        at: f64,
        text: Option<String>,
    },
    ToggleMute(usize),
    ToggleSolo(usize),
    SetVolume(usize, f32),
    SetHeight(usize, f32),
    /// Link the track with the one in the second row.
    Link(usize, usize),
    Unlink(usize),
    /// An audio track's header was dragged to a new place: the track in row `from` moves to row
    /// `to`.
    Move {
        from: usize,
        to: usize,
    },
    Rename(usize, String),
    /// Route the track to the named output bus.
    SetBus(usize, String),
    Remove(usize),
    Add,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimelineResponse {
    pub seek: Option<usize>,
    pub actions: Vec<TrackAction>,
    /// Frames whose thumbnails would fill the visible video items.
    pub wanted_thumbnails: Vec<usize>,
    /// A new loop region (`Some(None)` to remove it).
    pub loop_region: Option<Option<LoopRegion>>,
    /// The mode button was clicked: switch between the time and tempo rulers.
    pub toggle_mode: bool,
    /// The snapping button was clicked: turn snapping on or off.
    pub toggle_snap: bool,
    /// Screen x where the lanes start, for turning a pointer position into time.
    pub lanes_left: f32,
}

/// How close (pixels) the pointer must be to a loop edge on the ruler to drag that edge.
const LOOP_EDGE_GRAB: f32 = 6.0;

/// What a drag on the ruler is doing: moving the playhead, or (with Ctrl held when it started)
/// changing the loop region.
#[derive(Debug, Clone, Copy, PartialEq)]
enum RulerDrag {
    Seek,
    Loop(LoopDrag),
}

/// What a drag is doing to the loop region.
#[derive(Debug, Clone, Copy, PartialEq)]
enum LoopDrag {
    /// Making a new region from where the drag started (seconds).
    New {
        anchor: f64,
    },
    /// Moving the region's start or end.
    Start,
    End,
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

/// Labelled ruler tick spacing in beats, and the number of unlabelled divisions between labels,
/// for a zoom of `px_per_sec` at `tempo`: sixteenth notes down to single beats, then bars and runs
/// of bars.
pub fn beat_tick_steps(px_per_sec: f64, tempo: &Tempo) -> (f64, u32) {
    let min_beats = MIN_TICK_SPACING / px_per_sec.max(1e-9) / tempo.seconds_per_beat();
    let bar = f64::from(tempo.sanitized().beats_per_bar);
    let mut candidates = vec![
        (0.25, 1),
        (0.5, 2),
        (1.0, 4),
        (bar, tempo.sanitized().beats_per_bar),
    ];
    let mut bars = 2.0;
    while bars <= 4096.0 {
        let divisions = if bars <= 2.0 { 2 } else { 4 };
        candidates.push((bars * bar, divisions));
        bars *= 2.0;
    }
    candidates
        .into_iter()
        .find(|&(step, _)| step >= min_beats)
        .unwrap_or((8192.0 * bar, 4))
}

/// A position in beats (from the first beat) as `bar`, `bar.beat` or `bar.beat.sixteenth`,
/// counting from 1 like a DAW; the shortest form that names the position.
pub fn bars_label(beats: f64, beats_per_bar: u32) -> String {
    let per_bar = f64::from(beats_per_bar.max(1));
    let bar = (beats / per_bar + 1e-9).floor();
    let in_bar = (beats - bar * per_bar).max(0.0);
    let beat = (in_bar + 1e-9).floor();
    let sub = in_bar - beat;
    let bar_number = bar as i64 + 1;
    if in_bar < 1e-6 {
        bar_number.to_string()
    } else if sub < 1e-6 {
        format!("{bar_number}.{}", beat as i64 + 1)
    } else {
        format!(
            "{bar_number}.{}.{}",
            beat as i64 + 1,
            (sub * 4.0).round() as i64 + 1
        )
    }
}

/// The ruler's grid: where ticks fall and how major ones are labelled.
struct RulerGrid {
    /// Time of tick 0, in seconds.
    origin: f64,
    /// Seconds between ticks.
    minor: f64,
    /// Ticks per labelled (major) tick.
    divisions: u32,
    /// Beats between ticks and the bar length, in tempo mode.
    bars: Option<(f64, u32)>,
}

impl RulerGrid {
    fn new(model: &TimelineModel, px_per_sec: f64) -> Self {
        match model.mode {
            TimelineMode::Time => {
                let (major, divisions) = tick_steps(px_per_sec, model.frame_rate);
                Self {
                    origin: 0.0,
                    minor: major / f64::from(divisions),
                    divisions,
                    bars: None,
                }
            }
            TimelineMode::Tempo => {
                let tempo = model.tempo.sanitized();
                let (major, divisions) = beat_tick_steps(px_per_sec, &tempo);
                let minor_beats = major / f64::from(divisions);
                Self {
                    origin: tempo.offset_secs,
                    minor: minor_beats * tempo.seconds_per_beat(),
                    divisions,
                    bars: Some((minor_beats, tempo.beats_per_bar)),
                }
            }
        }
    }

    fn time(&self, k: i64) -> f64 {
        self.origin + k as f64 * self.minor
    }

    /// The tick at or before `seconds`.
    fn tick_at_or_before(&self, seconds: f64) -> i64 {
        ((seconds - self.origin) / self.minor).floor() as i64
    }

    fn label(&self, k: i64) -> String {
        match self.bars {
            Some((minor_beats, per_bar)) => bars_label(k as f64 * minor_beats, per_bar),
            None => timecode(self.time(k)),
        }
    }

    /// `seconds` moved to the nearest tick (in tempo mode) or frame (in time mode).
    fn snap(&self, seconds: f64, frame_rate: f64) -> f64 {
        match self.bars {
            Some(_) => self.time((((seconds - self.origin) / self.minor).round()) as i64),
            None => (seconds * frame_rate).round() / frame_rate,
        }
    }
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

/// The rectangles a timeline divides into, and where each track's row starts.
struct Areas {
    headers: Rect,
    lanes: Rect,
    ruler: Rect,
    /// Below the ruler, headers and lanes: the part that scrolls vertically.
    body: Rect,
    /// The top of each row from the top of the body before scrolling, then the bottom of the
    /// last.
    tops: Vec<f32>,
}

impl Areas {
    fn new(area: Rect, tracks: &[TrackView]) -> Self {
        let headers = Rect::from_min_max(
            pos2(area.left(), area.top() + RULER_HEIGHT),
            pos2(area.left() + HEADER_WIDTH, area.bottom()),
        );
        let lanes = Rect::from_min_max(pos2(headers.right() + GAP, area.top()), area.max);
        let ruler = Rect::from_min_max(lanes.min, pos2(lanes.right(), area.top() + RULER_HEIGHT));
        let body = Rect::from_min_max(pos2(area.left(), ruler.bottom()), area.max);
        let mut tops = vec![0.0];
        for track in tracks {
            tops.push(tops.last().unwrap() + track.height);
        }
        Self {
            headers,
            lanes,
            ruler,
            body,
            tops,
        }
    }

    /// The screen y of the top of row `row` (the number of rows for the bottom of the last).
    fn row_top(&self, row: usize, scroll: f32) -> f32 {
        self.body.top() + self.tops[row.min(self.tops.len() - 1)] - scroll
    }

    fn row_height(&self, row: usize) -> f32 {
        self.tops[row + 1] - self.tops[row]
    }

    /// Lane `row` in screen space, before clipping.
    fn lane(&self, row: usize, scroll: f32) -> Rect {
        Rect::from_min_size(
            pos2(self.lanes.left(), self.row_top(row, scroll)),
            vec2(self.lanes.width(), self.row_height(row)),
        )
        .shrink2(vec2(0.0, 2.0))
    }

    fn header(&self, row: usize, scroll: f32) -> Rect {
        Rect::from_min_size(
            pos2(self.headers.left(), self.row_top(row, scroll)),
            vec2(HEADER_WIDTH, self.row_height(row)),
        )
        .shrink2(vec2(0.0, 2.0))
    }

    /// Which row is at screen y, if any.
    fn row_at(&self, scroll: f32, y: f32) -> Option<usize> {
        let offset = y - self.body.top() + scroll;
        (offset >= 0.0)
            .then(|| self.tops.iter().rposition(|&top| top <= offset))
            .flatten()
            .filter(|&row| row + 1 < self.tops.len())
    }
}

pub fn timeline(ui: &mut Ui, model: &TimelineModel, view: &mut TimelineView) -> TimelineResponse {
    let mut response = TimelineResponse::default();
    let theme = Theme::of(ui.ctx());
    let area = ui.available_rect_before_wrap();
    let background = ui.allocate_rect(area, Sense::click_and_drag());
    let areas = Areas::new(area, &model.tracks);
    let lanes_left = areas.lanes.left();
    response.lanes_left = lanes_left;
    let width = areas.lanes.width().max(1.0);

    let duration = (model.frame_count as f64 / model.frame_rate).max(1e-6);
    let extent = model
        .tracks
        .iter()
        .flat_map(|t| t.items.iter().filter_map(|i| t.item_span(i)))
        .fold((0.0f64, duration), |(lo, hi), (start, end)| {
            (lo.min(start), hi.max(end))
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
    // The wheel and panning work anywhere over the timeline, including over items and header
    // widgets, which take the hover for themselves.
    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
    let pointer_here = ui.rect_contains_pointer(area);
    if scroll != 0.0 && pointer_here {
        if over(areas.lanes) {
            let anchor = pointer.map_or(0.0, |p| p.x - lanes_left);
            view.zoom_around(f64::from(scroll * 0.0015).exp(), anchor, limits);
        } else if over(areas.headers) {
            view.scroll_y -= scroll;
        }
    }
    let (panning, delta) = ui.input(|i| {
        let held = i.pointer.middle_down() || i.pointer.secondary_down();
        let started_here = i.pointer.press_origin().is_some_and(|p| area.contains(p));
        (held && started_here, i.pointer.delta())
    });
    if panning {
        view.left -= f64::from(delta.x) / view.px_per_sec;
        view.scroll_y -= delta.y;
    }
    let mode_button = Rect::from_min_size(
        pos2(area.left() + HEADER_WIDTH - MODE_WIDTH, area.top() + 1.0),
        vec2(MODE_WIDTH, RULER_HEIGHT - 2.0),
    );
    let (icon, name, hover) = match model.mode {
        TimelineMode::Time => ("⏱", tr("timeline.mode.time"), tr("timeline.mode.time.help")),
        TimelineMode::Tempo => (
            "♪",
            tr("timeline.mode.tempo"),
            tr("timeline.mode.tempo.help"),
        ),
    };
    if ui
        .put(
            mode_button,
            egui::Button::new(egui::RichText::new(format!("{icon} {name}")).small()),
        )
        .on_hover_text(hover)
        .clicked()
    {
        response.toggle_mode = true;
    }
    let snap_button = Rect::from_min_size(
        pos2(mode_button.left() - SNAP_WIDTH - 4.0, mode_button.top()),
        vec2(SNAP_WIDTH, mode_button.height()),
    );
    if ui
        .put(
            snap_button,
            egui::Button::selectable(model.snap, egui::RichText::new(tr("timeline.snap")).small()),
        )
        .on_hover_text(tr("timeline.snap.help"))
        .clicked()
    {
        response.toggle_snap = true;
    }
    if over(area) && !ui.ctx().egui_wants_keyboard_input() {
        item_keys(ui, model, &mut response);
    }
    // F shows the whole project.
    if over(area) && !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(Key::F)) {
        view.fit(width, duration);
    }
    view.clamp(width, limits, extent);
    let rows = model.tracks.len();
    let content = areas.tops[rows] + ADD_ROW_HEIGHT;
    view.scroll_y = view
        .scroll_y
        .clamp(0.0, (content - areas.body.height()).max(0.0));
    let x = |seconds: f64| view.x(lanes_left, seconds);

    // Lane space (empty, or an item's content below its header bar): a click selects the row and
    // no items, and a drag box-selects the items it touches, adding to the selection with Ctrl
    // or Shift. Seeking belongs to the ruler.
    let body_lanes = Rect::from_min_max(pos2(lanes_left, areas.body.top()), areas.body.max);
    let pressed_in_lanes = ui
        .input(|i| i.pointer.press_origin())
        .or(background.interact_pointer_pos())
        .is_some_and(|p| body_lanes.contains(p));
    if background.clicked()
        && pressed_in_lanes
        && let Some(p) = background.interact_pointer_pos()
    {
        response.actions.push(TrackAction::ClearItems);
        if let Some(row) = areas.row_at(view.scroll_y, p.y) {
            response.actions.push(TrackAction::Select(row));
        }
    }
    let box_id = ui.id().with("timeline-box-select");
    if background.drag_started_by(PointerButton::Primary)
        && pressed_in_lanes
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        let additive = ui.input(|i| i.modifiers.command || i.modifiers.shift);
        let start = BoxSelect {
            seconds: view.seconds(lanes_left, origin.x),
            y: origin.y - areas.body.top() + view.scroll_y,
            additive,
        };
        ui.data_mut(|d| d.insert_temp(box_id, start));
    }
    let boxing: Option<BoxSelect> = ui.data(|d| d.get_temp(box_id));
    // Drawn after the lanes, so it shows over the items.
    let mut dragged_box = None;
    if let (Some(start), Some(p)) = (boxing, background.interact_pointer_pos()) {
        let corner = pos2(
            view.x(lanes_left, start.seconds),
            start.y + areas.body.top() - view.scroll_y,
        );
        let selection = Rect::from_two_pos(corner, p);
        if background.dragged_by(PointerButton::Primary) {
            dragged_box = Some(selection);
        }
        if background.drag_stopped() {
            let items = items_in(&areas, model, view, selection);
            response.actions.push(TrackAction::SelectItems {
                items,
                additive: start.additive,
            });
        }
    }
    if background.drag_stopped() {
        ui.data_mut(|d| d.remove::<BoxSelect>(box_id));
    }

    loop_ruler(ui, &areas, model, view, &mut response);

    // Ruler and tick lines.
    let grid = RulerGrid::new(model, view.px_per_sec);
    let painter = ui.painter_at(areas.ruler.union(body_lanes));
    let shown_loop = match response.loop_region {
        Some(changed) => changed,
        None => model.loop_region,
    };
    if let Some(region) = shown_loop {
        paint_loop(&painter, region, x, areas.ruler, body_lanes, theme);
    }
    let first = grid.tick_at_or_before(view.left);
    let last = grid.tick_at_or_before(view.seconds(lanes_left, areas.lanes.right())) + 1;
    for k in first..=last {
        let t = grid.time(k);
        let tx = x(t);
        let is_major = k.rem_euclid(i64::from(grid.divisions)) == 0;
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
                grid.label(k),
                FontId::proportional(10.5),
                theme.tick_label,
            );
        }
    }
    // Rendered frames, along the bottom of the ruler.
    for range in model.cached {
        let x0 = x(range.start as f64 / model.frame_rate);
        let x1 = x(range.end as f64 / model.frame_rate);
        let bar = Rect::from_min_max(
            pos2(x0, areas.ruler.bottom() - 3.0),
            pos2(x1.max(x0 + 1.0), areas.ruler.bottom()),
        );
        painter.rect_filled(bar, CornerRadius::ZERO, theme.cached);
    }

    // Lanes, clipped to the scrolling body.
    let lane_painter = ui.painter_at(body_lanes);
    for (row, track) in model.tracks.iter().enumerate() {
        let lane = Lane {
            track,
            row,
            rect: areas.lane(row, view.scroll_y),
            clip: body_lanes,
        };
        lane.show(ui, &lane_painter, model, view, theme, &mut response);
    }
    if let Some(selection) = dragged_box {
        lane_painter.rect_filled(
            selection,
            CornerRadius::ZERO,
            theme.accent.gamma_multiply(0.12),
        );
        lane_painter.rect_stroke(
            selection,
            CornerRadius::ZERO,
            Stroke::new(1.0, theme.accent.gamma_multiply(0.8)),
            egui::StrokeKind::Inside,
        );
    }

    // Headers.
    let header_clip = Rect::from_min_max(
        areas.headers.min,
        pos2(areas.headers.right(), area.bottom()),
    );
    let header_painter = ui.painter_at(header_clip);
    let audio_rows = model
        .tracks
        .iter()
        .position(|t| t.kind == TrackKind::Audio)
        .unwrap_or(rows)..rows;
    let mut dragging: Option<(usize, usize)> = None;
    for (row, track) in model.tracks.iter().enumerate() {
        let rect = areas.header(row, view.scroll_y);
        if !rect.intersects(header_clip) {
            continue;
        }
        let fill = if model.selected_track == Some(row) {
            theme.accent.gamma_multiply(0.18)
        } else {
            ui.visuals().faint_bg_color
        };
        header_painter.rect_filled(rect, CornerRadius::same(3), fill);
        // The header's background selects the track, drags an audio track to a new place and
        // has the link menu. It goes under the widgets (which are made after it), so they keep
        // their clicks.
        let grip = ui.interact(
            rect.intersect(header_clip),
            ui.id().with(("track-grip", row)),
            Sense::click_and_drag(),
        );
        if grip.clicked() || grip.drag_started() {
            response.actions.push(TrackAction::Select(row));
        }
        let movable = audio_rows.contains(&row);
        if movable
            && (grip.dragged() || grip.drag_stopped())
            && let Some(p) = grip.interact_pointer_pos()
        {
            let boundaries: Vec<f32> = areas.tops[audio_rows.start..=audio_rows.end].to_vec();
            let slot =
                audio_rows.start + drop_slot(p.y - areas.body.top() + view.scroll_y, &boundaries);
            if grip.dragged() {
                dragging = Some((row, slot));
            } else if let Some(to) = move_destination(row, slot) {
                response.actions.push(TrackAction::Move { from: row, to });
            }
        }
        if movable && grip.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if movable && grip.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let grip = grip.on_hover_text(if movable {
            tr("timeline.track.reorder")
        } else {
            tr("timeline.track.header.help")
        });
        grip.context_menu(|ui| link_menu(ui, model, row, &mut response));
        header(ui, rect, header_clip, |ui| {
            track_header(ui, track, &model.buses, row, &mut response);
        });
        // The bottom edge resizes the track.
        let edge = Rect::from_min_max(
            pos2(rect.left(), rect.bottom() + 2.0 - RESIZE_GRAB),
            pos2(rect.right(), rect.bottom() + 2.0 + RESIZE_GRAB),
        );
        let resize = ui.interact(
            edge.intersect(header_clip),
            ui.id().with(("track-resize", row)),
            Sense::drag(),
        );
        if resize.hovered() || resize.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if resize.dragged() && resize.drag_delta().y != 0.0 {
            let height = (track.height + resize.drag_delta().y)
                .clamp(*LANE_HEIGHT_RANGE.start(), *LANE_HEIGHT_RANGE.end());
            response.actions.push(TrackAction::SetHeight(row, height));
        }
        if resize.double_clicked() {
            response
                .actions
                .push(TrackAction::SetHeight(row, LANE_HEIGHT));
        }
        resize.on_hover_text(tr("timeline.track.resize"));
    }
    // Where a dragged header would land.
    if let Some((from, slot)) = dragging {
        let y = areas.row_top(slot, view.scroll_y);
        let marker = move_destination(from, slot).is_some();
        header_painter.line_segment(
            [
                pos2(areas.headers.left(), y),
                pos2(areas.headers.right(), y),
            ],
            Stroke::new(
                2.5,
                if marker {
                    theme.accent
                } else {
                    theme.accent.gamma_multiply(0.3)
                },
            ),
        );
    }
    let add_row = Rect::from_min_size(
        pos2(areas.headers.left(), areas.row_top(rows, view.scroll_y)),
        vec2(HEADER_WIDTH, ADD_ROW_HEIGHT),
    );
    header(ui, add_row, header_clip, |ui| {
        if ui
            .button(tr("timeline.track.add"))
            .on_hover_text(tr("timeline.track.add.help"))
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

/// The header's right-click menu: link the track with another, or take it out of its link.
fn link_menu(ui: &mut Ui, model: &TimelineModel, row: usize, response: &mut TimelineResponse) {
    let track = &model.tracks[row];
    ui.menu_button(tr("timeline.track.link"), |ui| {
        for (other, candidate) in model.tracks.iter().enumerate() {
            if other == row || track.linked.contains(&candidate.name) {
                continue;
            }
            if ui.button(&candidate.name).clicked() {
                response.actions.push(TrackAction::Link(row, other));
                ui.close();
            }
        }
    });
    if !track.linked.is_empty()
        && ui
            .button(tr("timeline.track.unlink"))
            .on_hover_text(tr_args(
                "timeline.track.linked",
                &[("tracks", &track.linked.join(", "))],
            ))
            .clicked()
    {
        response.actions.push(TrackAction::Unlink(row));
        ui.close();
    }
    if track.height != LANE_HEIGHT && ui.button(tr("timeline.track.reset_height")).clicked() {
        response
            .actions
            .push(TrackAction::SetHeight(row, LANE_HEIGHT));
        ui.close();
    }
}

/// A box selection being dragged over the lanes: where it started, in seconds and in body
/// content y (before scrolling), so it stays put while the view moves.
#[derive(Debug, Clone, Copy)]
struct BoxSelect {
    seconds: f64,
    y: f32,
    additive: bool,
}

/// The items (row, item) whose blocks touch `selection`, in screen space.
fn items_in(
    areas: &Areas,
    model: &TimelineModel,
    view: &TimelineView,
    selection: Rect,
) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    for (row, track) in model.tracks.iter().enumerate() {
        let lane = areas.lane(row, view.scroll_y);
        if !lane.y_range().intersects(selection.y_range()) {
            continue;
        }
        for (k, item) in track.items.iter().enumerate() {
            let Some((start, end)) = track.item_span(item) else {
                continue;
            };
            let block = Rect::from_min_max(
                pos2(view.x(lane.left(), start), lane.top()),
                pos2(view.x(lane.left(), end), lane.bottom()),
            );
            if block.intersects(selection) {
                found.push((row, k));
            }
        }
    }
    found
}

/// The ruler's own clicks and drags: a click or drag moves the playhead, Ctrl+drag makes a loop
/// region (or moves one of its edges, when it starts on one), and right-click offers looping on
/// and off and clearing.
fn loop_ruler(
    ui: &mut Ui,
    areas: &Areas,
    model: &TimelineModel,
    view: &TimelineView,
    response: &mut TimelineResponse,
) {
    let lanes_left = areas.lanes.left();
    let ruler = ui.interact(
        areas.ruler,
        ui.id().with("timeline-ruler"),
        Sense::click_and_drag(),
    );
    let grid = RulerGrid::new(model, view.px_per_sec);
    let duration = model.frame_count as f64 / model.frame_rate;
    let snap = |x: f32| {
        grid.snap(view.seconds(lanes_left, x), model.frame_rate)
            .clamp(0.0, duration)
    };
    let seek = |x: f32, response: &mut TimelineResponse| {
        let frame = (view.seconds(lanes_left, x) * model.frame_rate).floor();
        response.seek = Some((frame.max(0.0) as usize).min(model.frame_count.saturating_sub(1)));
    };
    let region = model.loop_region;
    let ctrl = ui.input(|i| i.modifiers.command);
    let near = |x: f32, seconds: f64| (view.x(lanes_left, seconds) - x).abs() <= LOOP_EDGE_GRAB;
    if ctrl
        && let (Some(p), Some(r)) = (ruler.hover_pos(), region)
        && (near(p.x, r.start) || near(p.x, r.end))
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    if ruler.clicked()
        && !ctrl
        && let Some(p) = ruler.interact_pointer_pos()
    {
        seek(p.x, response);
    }

    let id = ui.id().with("ruler-drag");
    if ruler.drag_started_by(PointerButton::Primary)
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        let drag = if !ctrl {
            RulerDrag::Seek
        } else {
            RulerDrag::Loop(match region {
                Some(r) if near(origin.x, r.start) => LoopDrag::Start,
                Some(r) if near(origin.x, r.end) => LoopDrag::End,
                _ => LoopDrag::New {
                    anchor: snap(origin.x),
                },
            })
        };
        ui.data_mut(|d| d.insert_temp(id, drag));
    }
    let drag: Option<RulerDrag> = ui.data(|d| d.get_temp(id));
    if let (Some(drag), true, Some(p)) = (
        drag,
        ruler.dragged_by(PointerButton::Primary),
        ruler.interact_pointer_pos(),
    ) {
        match drag {
            RulerDrag::Seek => seek(p.x, response),
            RulerDrag::Loop(drag) => {
                let at = snap(p.x);
                let enabled = region.is_none_or(|r| r.enabled);
                let (start, end) = match (drag, region) {
                    (LoopDrag::New { anchor }, _) => (anchor.min(at), anchor.max(at)),
                    (LoopDrag::Start, Some(r)) => (at.min(r.end), at.max(r.end)),
                    (LoopDrag::End, Some(r)) => (r.start.min(at), r.start.max(at)),
                    (_, None) => (at, at),
                };
                if end > start {
                    response.loop_region = Some(Some(LoopRegion {
                        start,
                        end,
                        enabled: enabled || matches!(drag, LoopDrag::New { .. }),
                    }));
                }
            }
        }
    }
    if ruler.drag_stopped() {
        ui.data_mut(|d| d.remove::<RulerDrag>(id));
    }

    let ruler = ruler.on_hover_text(tr("timeline.ruler.help"));
    ruler.context_menu(|ui| {
        let Some(mut r) = region else {
            ui.weak(tr("timeline.loop.hint"));
            return;
        };
        if ui
            .checkbox(&mut r.enabled, tr("timeline.loop.enable"))
            .changed()
        {
            response.loop_region = Some(Some(r));
        }
        if ui.button(tr("timeline.loop.remove")).clicked() {
            response.loop_region = Some(None);
            ui.close();
        }
    });
}

/// The loop region: a band on the ruler with a handle at each edge, and faint shading over the
/// lanes. Greyed out when looping is off.
fn paint_loop(
    painter: &egui::Painter,
    region: LoopRegion,
    x: impl Fn(f64) -> f32,
    ruler: Rect,
    lanes: Rect,
    theme: &Theme,
) {
    let color = if region.enabled {
        theme.accent
    } else {
        theme.tick
    };
    let (x0, x1) = (x(region.start), x(region.end));
    let band = Rect::from_min_max(pos2(x0, ruler.top() + 2.0), pos2(x1, ruler.bottom()));
    painter.rect_filled(band, CornerRadius::same(2), color.gamma_multiply(0.35));
    painter.rect_filled(
        Rect::from_min_max(pos2(x0, lanes.top()), pos2(x1, lanes.bottom())),
        CornerRadius::ZERO,
        color.gamma_multiply(0.07),
    );
    for edge in [x0, x1] {
        painter.line_segment(
            [pos2(edge, ruler.top() + 2.0), pos2(edge, lanes.bottom())],
            Stroke::new(1.0, color.gamma_multiply(0.8)),
        );
        painter.rect_filled(
            Rect::from_center_size(pos2(edge, ruler.top() + 6.0), vec2(5.0, 9.0)),
            CornerRadius::same(1),
            color,
        );
    }
}

/// One track's lane: its items, each with a header bar along its top.
struct Lane<'a> {
    track: &'a TrackView,
    row: usize,
    rect: Rect,
    /// The visible part of the lanes.
    clip: Rect,
}

impl Lane<'_> {
    fn show(
        &self,
        ui: &Ui,
        painter: &egui::Painter,
        model: &TimelineModel,
        view: &TimelineView,
        theme: &Theme,
        response: &mut TimelineResponse,
    ) {
        let (track, row, lane) = (self.track, self.row, self.rect);
        if !lane.intersects(self.clip) {
            return;
        }
        if model.selected_track == Some(row) {
            painter.rect_stroke(
                lane,
                CornerRadius::same(3),
                Stroke::new(1.0, theme.accent),
                egui::StrokeKind::Inside,
            );
        }
        let Some(duration) = track.duration else {
            painter.text(
                lane.left_center() + vec2(6.0, 0.0),
                Align2::LEFT_CENTER,
                tr("timeline.track.loading"),
                FontId::proportional(11.0),
                ui.visuals().weak_text_color(),
            );
            return;
        };
        for (k, item) in track.items.iter().enumerate() {
            let block = Rect::from_min_max(
                pos2(view.x(lane.left(), item.position), lane.top() + 1.0),
                pos2(
                    view.x(lane.left(), item.timeline_end(duration)),
                    lane.bottom() - 1.0,
                ),
            );
            let visible = block.intersect(self.clip);
            if visible.width() <= 0.0 || visible.height() <= 0.0 {
                continue;
            }
            self.item(ui, painter, model, view, theme, (k, item, block), response);
        }
    }

    /// Item `k`, drawn in `block`: its content, then the header bar that drags it and mutes it.
    #[allow(clippy::too_many_arguments)]
    fn item(
        &self,
        ui: &Ui,
        painter: &egui::Painter,
        model: &TimelineModel,
        view: &TimelineView,
        theme: &Theme,
        (k, item, block): (usize, &Item, Rect),
        response: &mut TimelineResponse,
    ) {
        let (track, row, lane) = (self.track, self.row, self.rect);
        let visible = block.intersect(self.clip);
        let bar = Rect::from_min_max(block.min, pos2(block.right(), block.top() + ITEM_BAR));
        let grab = ui.interact(
            bar.intersect(self.clip),
            ui.id().with(("item-bar", row, k)),
            Sense::click_and_drag(),
        );
        let hovered = grab.hovered() || grab.dragged();
        let selected = track.selected_items.contains(&k);
        let (ctrl, alt) = ui.input(|i| (i.modifiers.command, i.modifiers.alt));
        let span = track.item_span(item);
        let drag_id = ui.id().with(("item-drag", row, k));
        // The edges trim. They are made after the bar, so they win where they overlap it, and
        // before the mute button, so it keeps its clicks.
        for edge in [Edge::Start, Edge::End] {
            let Some((start, end)) = span else { break };
            if block.width() < EDGE_GRAB * 3.0 {
                break;
            }
            let (ex, at) = match edge {
                Edge::Start => (block.left(), start),
                Edge::End => (block.right(), end),
            };
            let rect = Rect::from_min_max(
                pos2(ex - EDGE_GRAB, block.top()),
                pos2(ex + EDGE_GRAB, block.bottom()),
            )
            .intersect(self.clip);
            if rect.width() <= 0.0 || rect.height() <= 0.0 {
                continue;
            }
            let handle = ui.interact(
                rect,
                ui.id().with(("item-edge", row, k, edge == Edge::End)),
                Sense::drag(),
            );
            if handle.hovered() || handle.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if handle.drag_started_by(PointerButton::Primary) {
                let drag = ItemDrag {
                    edge: Some(edge),
                    origin: at,
                    length: 0.0,
                    targets: snap_targets(model, row, k, false),
                };
                ui.ctx().data_mut(|d| d.insert_temp(drag_id, drag));
            }
            if handle.dragged_by(PointerButton::Primary)
                && let Some(drag) = ui.ctx().data(|d| d.get_temp::<ItemDrag>(drag_id))
                && let Some(to) = drag_to(ui, model, view, &drag)
            {
                response.actions.push(TrackAction::TrimItem {
                    row,
                    item: k,
                    edge,
                    to,
                    stretch: alt,
                });
            }
            handle.on_hover_text(tr("timeline.item.edge"));
        }
        let base = match track.kind {
            TrackKind::Video => theme.video_block,
            TrackKind::Audio => theme.audio_block,
        };
        let mut fill = if hovered {
            base.gamma_multiply(1.2)
        } else {
            base
        };
        let dim = item.muted || track.silenced;
        if dim {
            fill = fill.gamma_multiply(0.4);
        }
        painter.rect_filled(block, CornerRadius::same(3), fill);
        let content = Rect::from_min_max(pos2(block.left(), bar.bottom()), block.max);
        match track.kind {
            TrackKind::Video if track.thumbnails => {
                let duration = track.duration.unwrap_or_default();
                thumbnails(
                    painter,
                    model,
                    view,
                    lane.left(),
                    (item, duration),
                    content,
                    self.clip,
                    response,
                );
            }
            TrackKind::Video => {}
            TrackKind::Audio => {
                if let Some(waveform) = &track.waveform {
                    let color = theme
                        .block_text
                        .gamma_multiply(if dim { 0.3 } else { 0.55 });
                    draw_waveform(
                        painter,
                        waveform,
                        view,
                        lane.left(),
                        item,
                        content,
                        self.clip,
                        color,
                    );
                }
            }
        }
        // The header bar: the track's name and the item's mute button.
        painter.rect_filled(
            bar,
            CornerRadius {
                nw: 3,
                ne: 3,
                sw: 0,
                se: 0,
            },
            egui::Color32::from_black_alpha(if hovered { 70 } else { 45 }),
        );
        let text = if item.muted {
            theme.block_text.gamma_multiply(0.5)
        } else {
            theme.block_text
        };
        let painter_bar = painter.with_clip_rect(bar.intersect(self.clip));
        painter_bar.text(
            pos2(visible.left() + 4.0, bar.center().y),
            Align2::LEFT_CENTER,
            &track.name,
            FontId::proportional(10.0),
            text,
        );
        let mute_rect = Rect::from_center_size(
            pos2(bar.right() - ITEM_BAR / 2.0 - 1.0, bar.center().y),
            vec2(ITEM_BAR, ITEM_BAR),
        );
        if bar.width() >= ITEM_BAR * 3.0 && self.clip.contains_rect(mute_rect) {
            let mute = ui.interact(
                mute_rect,
                ui.id().with(("item-mute", row, k)),
                Sense::click(),
            );
            painter.text(
                mute_rect.center(),
                Align2::CENTER_CENTER,
                if item.muted { "🔇" } else { "🔊" },
                FontId::proportional(9.5),
                if mute.hovered() { theme.accent } else { text },
            );
            if mute.clicked() {
                response
                    .actions
                    .push(TrackAction::ToggleItemMute { row, item: k });
            }
            mute.on_hover_text(if item.muted {
                tr("timeline.item.unmute")
            } else {
                tr("timeline.item.mute")
            });
        }

        if selected {
            painter.rect_stroke(
                block,
                CornerRadius::same(3),
                Stroke::new(2.0, theme.accent),
                egui::StrokeKind::Inside,
            );
        }

        if grab.clicked() {
            response.actions.push(TrackAction::SelectItem {
                row,
                item: k,
                toggle: ctrl,
            });
        }
        if grab.secondary_clicked() && !selected {
            response.actions.push(TrackAction::SelectItem {
                row,
                item: k,
                toggle: false,
            });
        }
        if grab.drag_started_by(PointerButton::Primary) {
            if !selected {
                response.actions.push(TrackAction::SelectItem {
                    row,
                    item: k,
                    toggle: ctrl,
                });
            }
            let drag = ItemDrag {
                edge: None,
                origin: item.position,
                length: span.map_or(f64::INFINITY, |(s, e)| e - s),
                targets: snap_targets(model, row, k, selected || ctrl),
            };
            ui.ctx().data_mut(|d| d.insert_temp(drag_id, drag));
        }
        if grab.dragged_by(PointerButton::Primary)
            && let Some(drag) = ui.ctx().data(|d| d.get_temp::<ItemDrag>(drag_id))
            && let Some(to) = drag_to(ui, model, view, &drag)
            && to != item.position
        {
            response.actions.push(TrackAction::MoveItem {
                row,
                item: k,
                delta: to - item.position,
            });
        }
        grab.context_menu(|ui| item_menu(ui, model, response));
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        grab.on_hover_text(if track.linked.is_empty() {
            tr("timeline.item.drag")
        } else {
            tr("timeline.item.drag_linked")
        });
    }
}

/// A drag of an item or one of its edges, remembered from where it started so snapping can
/// work from the pointer's whole movement.
#[derive(Debug, Clone)]
struct ItemDrag {
    /// The edge being trimmed, or `None` when the item is being moved.
    edge: Option<Edge>,
    /// Where the item started, or the edge was, when the drag began (seconds).
    origin: f64,
    /// The item's length, for a move, so its end snaps too.
    length: f64,
    /// Times the drag snaps to.
    targets: Vec<f64>,
}

/// Where a drag puts the item's start (or the edge), snapped unless snapping is off or Shift is
/// held.
fn drag_to(ui: &Ui, model: &TimelineModel, view: &TimelineView, drag: &ItemDrag) -> Option<f64> {
    let (from, to, free) = ui.input(|i| {
        (
            i.pointer.press_origin(),
            i.pointer.interact_pos(),
            i.modifiers.shift,
        )
    });
    let want = drag.origin + f64::from(to?.x - from?.x) / view.px_per_sec;
    if !model.snap || free {
        return Some(want);
    }
    let grid = RulerGrid::new(model, view.px_per_sec);
    let line = |t: f64| grid.time(((t - grid.origin) / grid.minor).round() as i64);
    let edges = match drag.edge {
        Some(_) => vec![want],
        None => vec![want, want + drag.length],
    };
    let reach = f64::from(SNAP_DISTANCE) / view.px_per_sec;
    Some(want + snap_offset(&edges, &drag.targets, line, reach))
}

/// Where a drag of item `k` of row `row` can snap: the timeline's start, the playhead, and the
/// edges of the items that don't move with it (the selected ones, when `with_selection`, and
/// those of linked tracks that overlap it).
fn snap_targets(model: &TimelineModel, row: usize, k: usize, with_selection: bool) -> Vec<f64> {
    let dragged = &model.tracks[row];
    let span = dragged.items.get(k).and_then(|i| dragged.item_span(i));
    let mut targets = vec![0.0, model.playhead as f64 / model.frame_rate];
    for (r, track) in model.tracks.iter().enumerate() {
        let linked = dragged.linked.contains(&track.name);
        for (j, item) in track.items.iter().enumerate() {
            let Some((start, end)) = track.item_span(item) else {
                continue;
            };
            let moving = (r == row && j == k)
                || (with_selection && track.selected_items.contains(&j))
                || (linked && span.is_some_and(|(a, b)| start < b && end > a));
            if !moving {
                targets.extend([start, end]);
            }
        }
    }
    targets
}

/// The item keys, while the pointer is over the timeline: S splits at the playhead, Delete and
/// Backspace remove the selected items, and the clipboard events copy, cut and paste them. The
/// clipboard events are taken out of the input so the graph, drawn later, doesn't see them.
fn item_keys(ui: &Ui, model: &TimelineModel, response: &mut TimelineResponse) {
    let at = model.playhead as f64 / model.frame_rate;
    let (split, delete) = ui.input(|i| {
        (
            i.modifiers.is_none() && i.key_pressed(Key::S),
            i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
        )
    });
    if split {
        response.actions.push(TrackAction::SplitItems { at });
    }
    if delete {
        response.actions.push(TrackAction::DeleteItems);
    }
    ui.input_mut(|i| {
        i.events.retain(|event| {
            let action = match event {
                egui::Event::Copy => TrackAction::CopyItems,
                egui::Event::Cut => TrackAction::CutItems,
                egui::Event::Paste(text) => TrackAction::PasteItems {
                    at,
                    text: Some(text.clone()),
                },
                _ => return true,
            };
            response.actions.push(action);
            false
        });
    });
}

/// An item's right-click menu: the item keys, for the selected items.
fn item_menu(ui: &mut Ui, model: &TimelineModel, response: &mut TimelineResponse) {
    let at = model.playhead as f64 / model.frame_rate;
    let entries = [
        (
            tr("timeline.item.split"),
            "S",
            TrackAction::SplitItems { at },
        ),
        (tr("menu.edit.copy"), "Ctrl+C", TrackAction::CopyItems),
        (tr("menu.edit.cut"), "Ctrl+X", TrackAction::CutItems),
        (
            tr("menu.edit.paste"),
            "Ctrl+V",
            TrackAction::PasteItems { at, text: None },
        ),
        (tr("timeline.item.delete"), "Del", TrackAction::DeleteItems),
    ];
    for (label, keys, action) in entries {
        if ui
            .add(egui::Button::new(label).shortcut_text(keys))
            .clicked()
        {
            response.actions.push(action);
            ui.close();
        }
    }
}

/// An audio item's waveform in `content`: one min/max line per pixel column of the visible part.
#[allow(clippy::too_many_arguments)]
fn draw_waveform(
    painter: &egui::Painter,
    waveform: &Waveform,
    view: &TimelineView,
    lanes_left: f32,
    item: &Item,
    content: Rect,
    clip: Rect,
    color: egui::Color32,
) {
    let visible = content.intersect(clip);
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        return;
    }
    let columns = visible.width().ceil() as usize;
    let start = item.source_time(view.seconds(lanes_left, visible.left()));
    let end = item.source_time(view.seconds(lanes_left, visible.left() + columns as f32));
    let mut peaks = Vec::with_capacity(columns);
    waveform.peaks(start, end, columns, &mut peaks);
    let mid = content.center().y;
    let half = content.height() / 2.0 - 2.0;
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

/// A video item's thumbnails in `content`: one per slot, each the nearest thumbnail decoded so
/// far of the source frame there. The frames that would fill the slots are added to the
/// response's wanted thumbnails.
#[allow(clippy::too_many_arguments)]
fn thumbnails(
    painter: &egui::Painter,
    model: &TimelineModel,
    view: &TimelineView,
    lanes_left: f32,
    (item, duration): (&Item, f64),
    content: Rect,
    clip: Rect,
    response: &mut TimelineResponse,
) {
    let fps = model.thumbnail_rate;
    if fps <= 0.0 {
        return;
    }
    let strip = Rect::from_min_max(
        pos2(content.left(), content.top() + 2.0),
        pos2(content.right(), content.bottom() - 2.0),
    );
    let shown = strip.intersect(clip);
    if shown.width() <= 0.0 || strip.height() <= 0.0 {
        return;
    }
    let aspect = model
        .thumbnails
        .values()
        .next()
        .map_or(16.0 / 9.0, |t| t.size.x / t.size.y.max(1.0));
    let slot_px = f64::from(strip.height() * aspect);
    // What the item shows of its file, in file seconds.
    let visible = (
        item.source_time(view.seconds(lanes_left, shown.left())),
        item.source_time(view.seconds(lanes_left, shown.right())),
    );
    let file_frames = (item.out(duration) * fps).ceil() as usize;
    let (step, frames) = thumbnail_frames(
        visible,
        slot_px / view.px_per_sec * item.rate,
        fps,
        file_frames,
    );
    // Where file time `s` is on screen.
    let at = |s: f64| view.x(lanes_left, item.position + (s - item.start) / item.rate);
    let painter = painter.with_clip_rect(shown);
    for &frame in &frames {
        let Some(thumbnail) = nearest(model.thumbnails, frame) else {
            continue;
        };
        let start = at(frame as f64 / fps);
        let end = at((frame + step) as f64 / fps);
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
    let room = MAX_THUMBNAIL_REQUEST.saturating_sub(response.wanted_thumbnails.len());
    response
        .wanted_thumbnails
        .extend(frames.into_iter().take(room));
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

/// The gap nearest to `y` among `boundaries`: the tops of the rows a header can be dropped
/// between, then the bottom of the last, all measured like `y` from the top of the body.
fn drop_slot(y: f32, boundaries: &[f32]) -> usize {
    boundaries
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - y).abs().total_cmp(&(b.1 - y).abs()))
        .map_or(0, |(i, _)| i)
}

/// The index a track dragged from `from` ends up at when dropped in gap `slot`, or `None` if that
/// leaves it where it is.
pub fn move_destination(from: usize, slot: usize) -> Option<usize> {
    let to = if slot > from { slot - 1 } else { slot };
    (to != from).then_some(to)
}

/// The widgets of a track's header: name, mute, solo and (for audio) remove; then the video's
/// size and frame rate, or an audio track's volume and its bus when the project has several (or
/// the track's is gone); and a link mark when it is linked.
fn track_header(
    ui: &mut Ui,
    track: &TrackView,
    buses: &[String],
    row: usize,
    response: &mut TimelineResponse,
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.horizontal(|ui| {
            ui.label(match track.kind {
                TrackKind::Video => "▣",
                TrackKind::Audio => "♪",
            });
            let edit = name_edit(ui, ui.id().with(("track-name", row)), &track.name, |e| {
                e.desired_width(NAME_WIDTH)
            });
            let help = match track.kind {
                TrackKind::Video => tr("timeline.video_name.help"),
                TrackKind::Audio => tr("timeline.track.name.help"),
            };
            let edit_response = edit.response.on_hover_text(help);
            if edit_response.gained_focus() {
                response.actions.push(TrackAction::Select(row));
            }
            if let Some(name) = edit.committed {
                response.actions.push(TrackAction::Rename(row, name));
            }
            let mute = egui::Button::new(if track.muted { "🔇" } else { "🔊" }).frame(false);
            if ui
                .add(mute)
                .on_hover_text(if track.muted {
                    tr("timeline.track.unmute")
                } else {
                    tr("timeline.track.mute")
                })
                .clicked()
            {
                response.actions.push(TrackAction::ToggleMute(row));
            }
            if ui
                .add(egui::Button::selectable(track.solo, "S"))
                .on_hover_text(tr("timeline.track.solo"))
                .clicked()
            {
                response.actions.push(TrackAction::ToggleSolo(row));
            }
            if track.kind == TrackKind::Audio
                && ui
                    .add(egui::Button::new("×").frame(false))
                    .on_hover_text(tr("timeline.track.remove"))
                    .clicked()
            {
                response.actions.push(TrackAction::Remove(row));
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            if !track.linked.is_empty() {
                ui.weak(tr("timeline.track.linked.mark"))
                    .on_hover_text(tr_args(
                        "timeline.track.linked",
                        &[("tracks", &track.linked.join(", "))],
                    ));
            }
            match track.kind {
                TrackKind::Video => {
                    if let Some(details) = &track.details {
                        ui.add(
                            egui::Label::new(egui::RichText::new(details).weak().small())
                                .truncate(),
                        );
                    }
                }
                TrackKind::Audio => audio_controls(ui, track, buses, row, response),
            }
        });
    });
}

/// An audio track's volume, and its bus when there is a choice to make.
fn audio_controls(
    ui: &mut Ui,
    track: &TrackView,
    buses: &[String],
    row: usize,
    response: &mut TimelineResponse,
) {
    let mut percent = f64::from(track.volume) * 100.0;
    if ui
        .add(
            crate::value_box::ValueBox::new(&mut percent)
                .range(0.0..=100.0)
                .speed(0.5)
                .max_decimals(0)
                .suffix(tr("timeline.track.volume.suffix")),
        )
        .on_hover_text(tr("timeline.track.volume.help"))
        .changed()
    {
        response
            .actions
            .push(TrackAction::SetVolume(row, (percent / 100.0) as f32));
    }
    if buses.len() > 1 || !buses.contains(&track.bus) {
        let mut bus = track.bus.clone();
        egui::ComboBox::from_id_salt(("track-bus", row))
            .width(64.0)
            .selected_text(&bus)
            .show_ui(ui, |ui| {
                for name in buses {
                    ui.selectable_value(&mut bus, name.clone(), name);
                }
            })
            .response
            .on_hover_text(tr("timeline.track.bus.help"));
        if bus != track.bus {
            response.actions.push(TrackAction::SetBus(row, bus));
        }
    }
}

/// Lays out a header's widgets inside `rect` from its top, clipped to `clip`.
fn header(ui: &mut Ui, rect: Rect, clip: Rect, contents: impl FnOnce(&mut Ui)) {
    if !rect.intersects(clip) {
        return;
    }
    let top = Rect::from_min_size(
        rect.min,
        vec2(rect.width(), rect.height().min(LANE_HEIGHT - 4.0)),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(top.shrink2(vec2(6.0, 3.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(clip.intersect(ui.clip_rect()));
    contents(&mut child);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_in_a_gap_moves_the_track_there() {
        // Three tracks: gaps 0..=3. Dragging track 0 below track 1 (gap 2) puts it at index 1.
        assert_eq!(move_destination(0, 2), Some(1));
        assert_eq!(move_destination(2, 0), Some(0));
        assert_eq!(move_destination(1, 1), None);
        assert_eq!(move_destination(1, 2), None);
        assert_eq!(move_destination(0, 3), Some(2));
    }

    #[test]
    fn the_nearest_gap_is_chosen_and_limited_to_the_tracks() {
        // Three tracks below a 58-point video row, the middle one 100 points tall.
        let boundaries = [58.0, 116.0, 216.0, 274.0];
        assert_eq!(drop_slot(58.0, &boundaries), 0);
        assert_eq!(drop_slot(150.0, &boundaries), 1);
        assert_eq!(drop_slot(180.0, &boundaries), 2);
        assert_eq!(drop_slot(-50.0, &boundaries), 0);
        assert_eq!(drop_slot(5000.0, &boundaries), 3);
    }

    #[test]
    fn rows_stack_at_their_own_heights() {
        let tall = TrackView {
            height: 100.0,
            ..TrackView::new("b", TrackKind::Audio)
        };
        let tracks = [TrackView::new("a", TrackKind::Video), tall];
        let areas = Areas::new(
            Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 400.0)),
            &tracks,
        );
        let body = RULER_HEIGHT;
        assert_eq!(areas.row_top(1, 0.0), body + LANE_HEIGHT);
        assert_eq!(areas.lane(1, 0.0).height(), 96.0);
        assert_eq!(areas.row_at(0.0, body + 10.0), Some(0));
        assert_eq!(areas.row_at(0.0, body + LANE_HEIGHT + 90.0), Some(1));
        assert_eq!(areas.row_at(0.0, body + LANE_HEIGHT + 110.0), None);
        assert_eq!(areas.row_at(20.0, body + LANE_HEIGHT - 10.0), Some(1));
    }

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
    fn beat_ticks_go_from_sixteenths_to_runs_of_bars() {
        let tempo = Tempo::default(); // 120 bpm, 4/4: a beat is half a second.
        // 150 px per beat (300 px/s): half a beat is under the minimum spacing, so a label every
        // beat, divided in quarters.
        assert_eq!(beat_tick_steps(300.0, &tempo), (1.0, 4));
        // 180 px per beat: half beats fit.
        assert_eq!(beat_tick_steps(360.0, &tempo), (0.5, 2));
        // Zoomed far in: sixteenths.
        assert_eq!(beat_tick_steps(10_000.0, &tempo), (0.25, 1));
        // 1000 px for 10 s = 100 px/s = 50 px a beat, 200 px a bar: a label per bar.
        assert_eq!(beat_tick_steps(100.0, &tempo), (4.0, 4));
        // Far out: runs of bars.
        let (major, _) = beat_tick_steps(1.0, &tempo);
        assert!(major >= 4.0 * 32.0, "{major}");
        // A 3/4 bar is three beats.
        let waltz = Tempo {
            beats_per_bar: 3,
            ..tempo
        };
        assert_eq!(beat_tick_steps(100.0, &waltz), (3.0, 3));
    }

    #[test]
    fn bar_labels_count_from_one() {
        assert_eq!(bars_label(0.0, 4), "1");
        assert_eq!(bars_label(1.0, 4), "1.2");
        assert_eq!(bars_label(4.0, 4), "2");
        assert_eq!(bars_label(5.75, 4), "2.2.4");
        assert_eq!(bars_label(6.0, 3), "3");
        // Before the first beat counts back through the bars.
        assert_eq!(bars_label(-4.0, 4), "0");
        assert_eq!(bars_label(-1.0, 4), "0.4");
    }

    #[test]
    fn the_bars_grid_follows_tempo_and_offset() {
        let model = |mode| TimelineModel {
            frame_count: 300,
            frame_rate: 30.0,
            playhead: 0,
            cached: &[],
            tracks: Vec::new(),
            selected_track: None,
            thumbnails: Box::leak(Box::default()),
            thumbnail_rate: 0.0,
            loop_region: None,
            tempo: Tempo {
                bpm: 120.0,
                beats_per_bar: 4,
                offset_secs: 0.25,
            },
            mode,
            buses: Vec::new(),
            snap: true,
        };
        let grid = RulerGrid::new(&model(TimelineMode::Tempo), 360.0);
        // Ticks every quarter beat (0.125 s), starting at the offset.
        assert_eq!(grid.time(0), 0.25);
        assert!((grid.time(4) - 0.75).abs() < 1e-12);
        assert_eq!(grid.label(0), "1");
        assert_eq!(grid.label(4), "1.2");
        assert_eq!(grid.label(16), "2");
        // Snapping lands on ticks; in time mode, on frames.
        assert!((grid.snap(0.77, 30.0) - 0.75).abs() < 1e-12);
        let time = RulerGrid::new(&model(TimelineMode::Time), 360.0);
        assert!((time.snap(0.77, 30.0) - 23.0 / 30.0).abs() < 1e-12);
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
