//! The timeline: a ruler, the video tracks (thumbnails) and audio tracks (waveforms), each a lane
//! of items, with Reaper-style track headers on the left.
//!
//! - The scroll wheel zooms time around the pointer (over the headers it scrolls the tracks);
//!   middle- or right-drag pans in both directions; F fits the whole project.
//! - Click or drag on the ruler to seek; drag over empty lane space to box-select items; drag an
//!   item by its header bar to move it (with the other selected items, and the items they
//!   overlap on linked tracks).
//! - Click an item's header bar to select it, Ctrl+click to add or remove it; drag an item's
//!   edge to trim it, Alt+drag to change its rate. Drags snap to the grid, item edges and the
//!   playhead while snapping is on; Shift drags freely.
//! - Over the timeline, S splits at the playhead (the selected items, or with none selected
//!   every item under it), Delete removes the selected items, and Ctrl+C, Ctrl+X and Ctrl+V
//!   copy, cut and paste them at the playhead.
//! - Ctrl+drag on the ruler makes a loop region (or moves one of its edges).
//! - Drag a header to reorder the tracks of its kind, its bottom edge to change the track's
//!   height; right-click it to link tracks or remove one.
//! - Graph layers are lanes above the tracks (top layer first). Their items work like track
//!   items (header bar, trim at the edges, snapping, S, Delete) but are not stretched, and a
//!   move is applied when the drag ends, because the model trims what an item lands on.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use eframe::egui::{
    self, Align2, CornerRadius, FontId, Key, PointerButton, Rect, Sense, Stroke, TextureId, Ui,
    UiBuilder, Vec2, pos2, vec2,
};
use rastersong_engine::{
    Edge, Item, LoopRegion, Tempo, TimelineMode, TrackKind, Waveform, drop_depths, snap_offset,
};
use rastersong_lang::{tr, tr_args};

use crate::name_edit::name_edit;
use crate::resources::DraggedGraph;
use crate::theme::Theme;

/// Width of the track header column.
pub const HEADER_WIDTH: f32 = 220.0;
/// Gap between the headers and the lanes.
const GAP: f32 = 6.0;
/// Width of the name fields in the track headers, leaving room for the buttons beside them.
const NAME_WIDTH: f32 = HEADER_WIDTH - 136.0;
const RULER_HEIGHT: f32 = 22.0;
/// How far a track's header is indented for each folder it is in.
const INDENT: f32 = 14.0;
/// The narrowest a deeply indented track's name gets.
const MIN_NAME_WIDTH: f32 = 40.0;
/// A track's height until the user changes it: tall enough for a header's two rows of widgets.
pub const LANE_HEIGHT: f32 = 58.0;
/// The shortest and tallest a track can be made.
pub const LANE_HEIGHT_RANGE: std::ops::RangeInclusive<f32> = 46.0..=320.0;
/// Height of a graph layer's lane and header.
pub const LAYER_HEIGHT: f32 = 40.0;
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
    /// What the track plays; `None` for a folder or an empty track.
    pub kind: Option<TrackKind>,
    /// A folder holds the deeper tracks below it and has no items of its own.
    pub folder: bool,
    /// How many folders the track is in.
    pub depth: u32,
    /// A folder whose tracks are hidden.
    pub collapsed: bool,
    /// Whether the track feeds the master; a track that doesn't is left out of the mix.
    pub master_send: bool,
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
    /// The track's file is missing: it shows an error where its items would be.
    pub missing: bool,
    /// The thumbnails of this track's file, once it is open.
    pub thumbnails: Option<TrackThumbnails>,
    /// The selected items, by index into [`Self::items`].
    pub selected_items: Vec<usize>,
}

impl TrackView {
    /// A track of `kind` with nothing loaded yet, at the top of the tree and the default height.
    pub fn new(name: impl Into<String>, kind: Option<TrackKind>) -> Self {
        Self {
            name: name.into(),
            kind,
            folder: false,
            depth: 0,
            collapsed: false,
            master_send: true,
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
            missing: false,
            thumbnails: None,
            selected_items: Vec::new(),
        }
    }

    /// How long item `item` lasts on the timeline; `None` until the file's length is known.
    fn item_span(&self, item: &Item) -> Option<(f64, f64)> {
        self.duration.map(|d| (item.position, item.timeline_end(d)))
    }
}

/// The thumbnails decoded so far of one video track's file.
#[derive(Debug, Clone)]
pub struct TrackThumbnails {
    /// By frame of the file.
    pub frames: Arc<BTreeMap<usize, Thumbnail>>,
    /// The frame rate of the file.
    pub rate: f64,
}

/// A thumbnail ready to draw.
#[derive(Debug, Clone, Copy)]
pub struct Thumbnail {
    pub texture: TextureId,
    pub size: Vec2,
}

/// A graph item as the timeline shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphItemView {
    /// The graph's name.
    pub name: String,
    pub position: f64,
    pub length: f64,
    pub muted: bool,
}

/// One graph layer as the timeline shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerView {
    /// The layer's index in the project (layers are stored bottom first; the timeline lists the
    /// top layer first).
    pub index: usize,
    pub name: String,
    pub muted: bool,
    pub solo: bool,
    /// Left out by its mute or another layer's solo: drawn dimmed.
    pub silenced: bool,
    pub items: Vec<GraphItemView>,
    /// The selected item, by index into [`Self::items`].
    pub selected_item: Option<usize>,
}

/// What the timeline shows.
#[derive(Debug, Clone)]
pub struct TimelineModel<'a> {
    pub frame_count: usize,
    pub frame_rate: f64,
    pub playhead: usize,
    pub cached: &'a [Range<usize>],
    /// The graph layers above the tracks, top layer first.
    pub layers: Vec<LayerView>,
    /// The tracks shown, top first: every track but those in a collapsed folder.
    pub tracks: Vec<TrackView>,
    /// The selected row of [`Self::tracks`].
    pub selected_track: Option<usize>,
    pub loop_region: Option<LoopRegion>,
    /// The project tempo, which the ruler follows in [`TimelineMode::Tempo`].
    pub tempo: Tempo,
    pub mode: TimelineMode,
    /// The project's output buses, master first. Track headers offer them when there are
    /// several.
    pub buses: Vec<String>,
    /// Whether dragged items and edges snap.
    pub snap: bool,
    /// A resource or graph being dragged from the Resources panel, for the drop preview.
    pub drop_ghost: Option<DropGhost>,
}

/// What is being dragged over the timeline from the Resources panel. While the pointer is over
/// the lanes a ghost shows where it would land: an item on the track or layer that takes it, or
/// a ghost row where a new track or layer would be made.
#[derive(Debug, Clone, PartialEq)]
pub struct DropGhost {
    pub name: String,
    /// Its length in seconds. `None` for a resource no track plays yet, whose length isn't
    /// known: the ghost then marks only where it starts.
    pub length: Option<f64>,
    pub target: GhostTarget,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GhostTarget {
    /// A resource for tracks; `lands_on[row]` says whether the track in that row of
    /// [`TimelineModel::tracks`] takes it. Anywhere else a new track is made, before row
    /// `new_row` (the number of rows for the bottom).
    Media { lands_on: Vec<bool>, new_row: usize },
    /// A graph: on the layer under the pointer, or a new layer on top.
    Graph,
}

/// Something the user did to a track. Tracks are named by their row in
/// [`TimelineModel::tracks`].
#[derive(Debug, Clone, PartialEq)]
pub enum TrackAction {
    Select(usize),
    /// The "+ Track" menu asked for an empty track.
    AddEmpty,
    /// The "+ Track" menu asked for a folder.
    AddFolder,
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
    /// Show or hide what is in the folder.
    ToggleCollapsed(usize),
    /// Send the track to the master or leave it out of the mix.
    SetMasterSend(usize, bool),
    SetVolume(usize, f32),
    SetHeight(usize, f32),
    /// Link the track with the one in the second row.
    Link(usize, usize),
    Unlink(usize),
    /// A track's header was dragged to a new place: the track in row `from` (with what is in
    /// it) moves before the track in row `slot` (the number of rows for the bottom), `depth`
    /// folders deep.
    Move {
        from: usize,
        slot: usize,
        depth: u32,
    },
    Rename(usize, String),
    /// Route the track to the named output bus.
    SetBus(usize, String),
    Remove(usize),
    Add,
}

/// Something the user did to a graph layer or its items. Layers are named by their index in the
/// project (see [`LayerView::index`]).
#[derive(Debug, Clone, PartialEq)]
pub enum LayerAction {
    /// The "+ Layer" button.
    Add,
    SelectItem {
        layer: usize,
        item: usize,
    },
    /// The item was dragged by `delta` seconds, and released.
    MoveItem {
        layer: usize,
        item: usize,
        delta: f64,
    },
    /// An edge of the item was dragged to time `to` (seconds).
    TrimItem {
        layer: usize,
        item: usize,
        edge: Edge,
        to: f64,
    },
    ToggleItemMute {
        layer: usize,
        item: usize,
    },
    /// The item's menu asked to split it at time `at` (the playhead).
    SplitItem {
        layer: usize,
        item: usize,
        at: f64,
    },
    DeleteItem {
        layer: usize,
        item: usize,
    },
    /// The item was double-clicked: open its graph in the editor.
    OpenItem {
        layer: usize,
        item: usize,
    },
    ToggleMute(usize),
    ToggleSolo(usize),
    Rename(usize, String),
    Remove(usize),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimelineResponse {
    pub seek: Option<usize>,
    pub actions: Vec<TrackAction>,
    pub layer_actions: Vec<LayerAction>,
    /// The layer (by project index) the pointer is over, for dropping a graph on it.
    pub layer_under_pointer: Option<usize>,
    /// Frames whose thumbnails would fill the visible video items, with the track they are of.
    pub wanted_thumbnails: Vec<(String, usize)>,
    /// A new loop region (`Some(None)` to remove it).
    pub loop_region: Option<Option<LoopRegion>>,
    /// The mode button was clicked: switch between the time and tempo rulers.
    pub toggle_mode: bool,
    /// The snapping button was clicked: turn snapping on or off.
    pub toggle_snap: bool,
    /// Screen x where the lanes start, for turning a pointer position into time.
    pub lanes_left: f32,
    /// The row of the lanes the pointer is over, for dropping a resource on a track.
    pub row_under_pointer: Option<usize>,
    /// Where a resource or graph dragged over the lanes would start (seconds), snapped like an
    /// item drag. A drop lands there, where its ghost was drawn.
    pub drop_at: Option<f64>,
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
    /// The top of each track row from the top of the body before scrolling (below the graph
    /// layers), then the bottom of the last.
    tops: Vec<f32>,
}

impl Areas {
    fn new(area: Rect, tracks: &[TrackView], layers: usize) -> Self {
        let headers = Rect::from_min_max(
            pos2(area.left(), area.top() + RULER_HEIGHT),
            pos2(area.left() + HEADER_WIDTH, area.bottom()),
        );
        let lanes = Rect::from_min_max(pos2(headers.right() + GAP, area.top()), area.max);
        let ruler = Rect::from_min_max(lanes.min, pos2(lanes.right(), area.top() + RULER_HEIGHT));
        let body = Rect::from_min_max(pos2(area.left(), ruler.bottom()), area.max);
        let mut tops = vec![layers as f32 * LAYER_HEIGHT];
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

    /// Layer lane `display` (0 is the top layer) in screen space, before clipping.
    fn layer_lane(&self, display: usize, scroll: f32) -> Rect {
        Rect::from_min_size(
            pos2(
                self.lanes.left(),
                self.body.top() + display as f32 * LAYER_HEIGHT - scroll,
            ),
            vec2(self.lanes.width(), LAYER_HEIGHT),
        )
        .shrink2(vec2(0.0, 2.0))
    }

    fn layer_header(&self, display: usize, scroll: f32) -> Rect {
        let lane = self.layer_lane(display, scroll);
        Rect::from_min_size(
            pos2(self.headers.left(), lane.top()),
            vec2(HEADER_WIDTH, lane.height()),
        )
    }

    /// Which layer (0 is the top) is at screen y, if any.
    fn layer_at(&self, scroll: f32, y: f32, layers: usize) -> Option<usize> {
        let offset = y - self.body.top() + scroll;
        (offset >= 0.0)
            .then(|| (offset / LAYER_HEIGHT) as usize)
            .filter(|&d| d < layers)
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
    let areas = Areas::new(area, &model.tracks, model.layers.len());
    let lanes_left = areas.lanes.left();
    response.lanes_left = lanes_left;
    let width = areas.lanes.width().max(1.0);

    let duration = (model.frame_count as f64 / model.frame_rate).max(1e-6);
    let extent = model
        .tracks
        .iter()
        .flat_map(|t| t.items.iter().filter_map(|i| t.item_span(i)))
        .chain(
            model
                .layers
                .iter()
                .flat_map(|l| l.items.iter().map(|i| (i.position, i.position + i.length))),
        )
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
    response.row_under_pointer = pointer
        .filter(|p| body_lanes.contains(*p))
        .and_then(|p| areas.row_at(view.scroll_y, p.y));
    response.layer_under_pointer = pointer
        .filter(|p| body_lanes.contains(*p))
        .and_then(|p| areas.layer_at(view.scroll_y, p.y, model.layers.len()))
        .map(|display| model.layers[display].index);
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
    let graph_dragged = egui::DragAndDrop::has_payload_of_type::<DraggedGraph>(ui.ctx());
    for (display, layer) in model.layers.iter().enumerate() {
        let lane = LayerLane {
            layer,
            rect: areas.layer_lane(display, view.scroll_y),
            clip: body_lanes,
            drop_target: graph_dragged && response.layer_under_pointer == Some(layer.index),
        };
        lane.show(ui, &lane_painter, model, view, theme, &mut response);
    }
    if graph_dragged && response.layer_under_pointer.is_none() && over(body_lanes) {
        lane_painter.text(
            pos2(areas.lanes.right() - 8.0, body_lanes.top() + 4.0),
            Align2::RIGHT_TOP,
            tr("timeline.layer.drop_new"),
            FontId::proportional(11.0),
            theme.accent,
        );
    }
    for (row, track) in model.tracks.iter().enumerate() {
        let lane = Lane {
            track,
            row,
            rect: areas.lane(row, view.scroll_y),
            clip: body_lanes,
        };
        lane.show(ui, &lane_painter, model, view, theme, &mut response);
    }
    if let Some(ghost) = &model.drop_ghost
        && let Some(p) = pointer.filter(|p| body_lanes.contains(*p))
    {
        let want = view.seconds(lanes_left, p.x).max(0.0);
        let at = snap_time(
            ui,
            model,
            view,
            want,
            ghost.length,
            &layer_snap_targets(model, None),
        );
        response.drop_at = Some(at);
        let (band, new_row) = ghost_band(&areas, model, view, ghost, &response);
        if new_row {
            lane_painter.rect_filled(
                band,
                CornerRadius::same(3),
                theme.accent.gamma_multiply(0.1),
            );
        }
        paint_ghost(
            &lane_painter,
            theme,
            band,
            x(at),
            ghost.length.map(|l| x(at + l)),
            &ghost.name,
        );
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
    for (display, layer) in model.layers.iter().enumerate() {
        let rect = areas.layer_header(display, view.scroll_y);
        if !rect.intersects(header_clip) {
            continue;
        }
        header_painter.rect_filled(rect, CornerRadius::same(3), ui.visuals().faint_bg_color);
        // The background has the layer's menu; it goes under the widgets, so they keep their
        // clicks.
        let grip = ui.interact(
            rect.intersect(header_clip),
            ui.id().with(("layer-grip", layer.index)),
            Sense::click(),
        );
        grip.context_menu(|ui| {
            if ui.button(tr("timeline.layer.remove")).clicked() {
                response
                    .layer_actions
                    .push(LayerAction::Remove(layer.index));
                ui.close();
            }
        });
        grip.on_hover_text(tr("timeline.layer.header.help"));
        header(ui, rect, header_clip, |ui| {
            layer_header(ui, layer, &mut response);
        });
    }
    // The shown tree, for where a dragged header can land.
    let shape: Vec<(u32, bool)> = model.tracks.iter().map(|t| (t.depth, t.folder)).collect();
    let mut dragging: Option<(usize, usize, Option<u32>)> = None;
    for (row, track) in model.tracks.iter().enumerate() {
        let rect = areas.header(row, view.scroll_y);
        if !rect.intersects(header_clip) {
            continue;
        }
        let fill = if model.selected_track == Some(row) {
            theme.accent.gamma_multiply(0.18)
        } else if track.folder {
            ui.visuals().widgets.noninteractive.bg_fill
        } else {
            ui.visuals().faint_bg_color
        };
        header_painter.rect_filled(rect, CornerRadius::same(3), fill);
        // The header's background selects the track, drags it (with what is in it) to a new
        // place in the tree and has the track menu. It goes under the widgets (which are made
        // after it), so they keep their clicks.
        let grip = ui.interact(
            rect.intersect(header_clip),
            ui.id().with(("track-grip", row)),
            Sense::click_and_drag(),
        );
        if grip.clicked() || grip.drag_started() {
            response.actions.push(TrackAction::Select(row));
        }
        let movable = rows > 1;
        if movable
            && (grip.dragged() || grip.drag_stopped())
            && let Some(p) = grip.interact_pointer_pos()
        {
            let boundaries: Vec<f32> = areas.tops[..=rows].to_vec();
            let slot = drop_slot(p.y - areas.body.top() + view.scroll_y, &boundaries);
            let wanted = ((p.x - areas.headers.left() - INDENT / 2.0) / INDENT).max(0.0) as u32;
            let depth = drop_depths(&shape, row, slot)
                .map(|range| wanted.clamp(*range.start(), *range.end()));
            if grip.dragged() {
                dragging = Some((row, slot, depth));
            } else if let Some(depth) = depth.filter(|&d| moves(&shape, row, slot, d)) {
                response.actions.push(TrackAction::Move {
                    from: row,
                    slot,
                    depth,
                });
            }
        }
        if movable && grip.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            crate::widgets::drag_bubble(ui.ctx(), rect, track_icon(track), &track.name);
        } else if movable && grip.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let grip = grip.on_hover_text(if movable {
            tr("timeline.track.reorder")
        } else {
            tr("timeline.track.header.help")
        });
        grip.context_menu(|ui| track_menu(ui, model, row, &mut response));
        let indent = track.depth as f32 * INDENT;
        if indent > 0.0 {
            // A guide down the left of the header for each folder the track is in.
            for level in 0..track.depth {
                let gx = rect.left() + level as f32 * INDENT + INDENT / 2.0;
                header_painter.line_segment(
                    [pos2(gx, rect.top()), pos2(gx, rect.bottom())],
                    Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.4)),
                );
            }
        }
        header(
            ui,
            rect.with_min_x(rect.left() + indent),
            header_clip,
            |ui| {
                track_header(ui, track, &model.buses, row, &mut response);
            },
        );
        // The bottom edge resizes the track.
        let edge = Rect::from_min_max(
            pos2(rect.left(), rect.bottom() + 2.0 - RESIZE_GRAB),
            pos2(rect.right(), rect.bottom() + 2.0 + RESIZE_GRAB),
        );
        let resize = ui.interact(
            edge.intersect(header_clip),
            ui.id().with(("track-resize", row)),
            // Takes clicks too: a plain drag sense never reports the double-click that resets.
            Sense::click_and_drag(),
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
    // Where a dragged header would land, indented as deep as it would go.
    if let Some((from, slot, depth)) = dragging {
        let y = areas.row_top(slot, view.scroll_y);
        let marker = depth.is_some_and(|d| moves(&shape, from, slot, d));
        let left = areas.headers.left() + depth.unwrap_or(0) as f32 * INDENT;
        header_painter.line_segment(
            [pos2(left, y), pos2(areas.headers.right(), y)],
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
        ui.menu_button(tr("timeline.track.add"), |ui| {
            if ui
                .button(tr("timeline.track.add_empty"))
                .on_hover_text(tr("timeline.track.add_empty.help"))
                .clicked()
            {
                response.actions.push(TrackAction::AddEmpty);
                ui.close();
            }
            if ui
                .button(tr("timeline.track.add_folder"))
                .on_hover_text(tr("timeline.track.add_folder.help"))
                .clicked()
            {
                response.actions.push(TrackAction::AddFolder);
                ui.close();
            }
            if ui
                .button(tr("timeline.track.add_file"))
                .on_hover_text(tr("timeline.track.add_file.help"))
                .clicked()
            {
                response.actions.push(TrackAction::Add);
                ui.close();
            }
        })
        .response
        .on_hover_text(tr("timeline.track.add.help"));
        if ui
            .button(tr("timeline.layer.add"))
            .on_hover_text(tr("timeline.layer.add.help"))
            .clicked()
        {
            response.layer_actions.push(LayerAction::Add);
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

/// The header's right-click menu: send the track to the master or not, link it with another or
/// take it out of its link, reset its height, and remove it.
fn track_menu(ui: &mut Ui, model: &TimelineModel, row: usize, response: &mut TimelineResponse) {
    let track = &model.tracks[row];
    let mut send = track.master_send;
    if ui
        .checkbox(&mut send, tr("timeline.track.master_send"))
        .on_hover_text(tr("timeline.track.master_send.help"))
        .clicked()
    {
        response.actions.push(TrackAction::SetMasterSend(row, send));
        ui.close();
    }
    ui.separator();
    if !track.folder {
        track_link_menu(ui, model, row, response);
    }
    if track.height != LANE_HEIGHT && ui.button(tr("timeline.track.reset_height")).clicked() {
        response
            .actions
            .push(TrackAction::SetHeight(row, LANE_HEIGHT));
        ui.close();
    }
    ui.separator();
    if ui.button(tr("timeline.track.remove")).clicked() {
        response.actions.push(TrackAction::Remove(row));
        ui.close();
    }
}

/// Linking a track with another, or taking it out of its link. Folders hold no items, so they
/// aren't offered.
fn track_link_menu(
    ui: &mut Ui,
    model: &TimelineModel,
    row: usize,
    response: &mut TimelineResponse,
) {
    let track = &model.tracks[row];
    ui.menu_button(tr("timeline.track.link"), |ui| {
        for (other, candidate) in model.tracks.iter().enumerate() {
            if other == row || candidate.folder || track.linked.contains(&candidate.name) {
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
        // A folder's lane is a plain band: it holds no items of its own.
        let fill = if track.folder {
            ui.visuals().widgets.noninteractive.bg_fill
        } else {
            theme.lane_bg
        };
        painter.rect_filled(lane, CornerRadius::same(3), fill);
        let (outline, width) = if model.selected_track == Some(row) {
            (theme.accent, 1.5)
        } else {
            (theme.lane_outline, 1.0)
        };
        painter.rect_stroke(
            lane,
            CornerRadius::same(3),
            Stroke::new(width, outline),
            egui::StrokeKind::Inside,
        );
        if track.folder || (track.kind.is_none() && !track.missing) {
            return;
        }
        if track.missing {
            painter.text(
                lane.left_center() + vec2(6.0, 0.0),
                Align2::LEFT_CENTER,
                tr("timeline.track.missing"),
                FontId::proportional(11.0),
                ui.visuals().error_fg_color,
            );
            return;
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
        edge_handles(
            ui,
            model,
            view,
            self.clip,
            block,
            span,
            (("item-edge", row, k), drag_id),
            &|| snap_targets(model, row, k, false),
            tr("timeline.item.edge"),
            |edge, to| {
                response.actions.push(TrackAction::TrimItem {
                    row,
                    item: k,
                    edge,
                    to,
                    stretch: alt,
                });
            },
        );
        let base = match track.kind {
            Some(TrackKind::Video) => theme.video_block,
            _ => theme.audio_block,
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
            Some(TrackKind::Video) if track.thumbnails.is_some() => {
                let duration = track.duration.unwrap_or_default();
                thumbnails(
                    painter,
                    track,
                    view,
                    lane.left(),
                    (item, duration),
                    content,
                    self.clip,
                    response,
                );
            }
            Some(TrackKind::Video) | None => {}
            Some(TrackKind::Audio) => {
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
        if item_bar(
            ui,
            painter,
            self.clip,
            theme,
            bar,
            (visible.left(), &track.name, item.muted),
            hovered,
            ui.id().with(("item-mute", row, k)),
        ) {
            response
                .actions
                .push(TrackAction::ToggleItemMute { row, item: k });
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

/// The handles along an item's two edges, which trim it: `trim` gets the edge and the time it
/// was dragged to (snapped). `ids` are the handles' id parts and the id the drag is remembered
/// under; `targets` lists where a drag snaps to, asked when a drag starts. The handles are made
/// after the item's bar, so they win where they overlap it, and before its mute button, so that
/// keeps its clicks. Shared by track items and graph items.
#[allow(clippy::too_many_arguments)]
fn edge_handles(
    ui: &Ui,
    model: &TimelineModel,
    view: &TimelineView,
    clip: Rect,
    block: Rect,
    span: Option<(f64, f64)>,
    ((tag, a, b), drag_id): ((&str, usize, usize), egui::Id),
    targets: &dyn Fn() -> Vec<f64>,
    help: &str,
    mut trim: impl FnMut(Edge, f64),
) {
    let Some((start, end)) = span else { return };
    if block.width() < EDGE_GRAB * 3.0 {
        return;
    }
    for edge in [Edge::Start, Edge::End] {
        let (ex, at) = match edge {
            Edge::Start => (block.left(), start),
            Edge::End => (block.right(), end),
        };
        let rect = Rect::from_min_max(
            pos2(ex - EDGE_GRAB, block.top()),
            pos2(ex + EDGE_GRAB, block.bottom()),
        )
        .intersect(clip);
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            continue;
        }
        let handle = ui.interact(
            rect,
            ui.id().with((tag, a, b, edge == Edge::End)),
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
                targets: targets(),
            };
            ui.ctx().data_mut(|d| d.insert_temp(drag_id, drag));
        }
        if handle.dragged_by(PointerButton::Primary)
            && let Some(drag) = ui.ctx().data(|d| d.get_temp::<ItemDrag>(drag_id))
            && let Some(to) = drag_to(ui, model, view, &drag)
        {
            trim(edge, to);
        }
        handle.on_hover_text(help);
    }
}

/// The header bar of an item, drawn over `bar`: the name at `left` and the mute button at its
/// right end. Returns whether the mute button was clicked. Shared by track items and graph items.
#[allow(clippy::too_many_arguments)]
fn item_bar(
    ui: &Ui,
    painter: &egui::Painter,
    clip: Rect,
    theme: &Theme,
    bar: Rect,
    (left, name, muted): (f32, &str, bool),
    hovered: bool,
    mute_id: egui::Id,
) -> bool {
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
    let text = if muted {
        theme.block_text.gamma_multiply(0.5)
    } else {
        theme.block_text
    };
    let painter_bar = painter.with_clip_rect(bar.intersect(clip));
    painter_bar.text(
        pos2(left + 4.0, bar.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(10.0),
        text,
    );
    let mute_rect = Rect::from_center_size(
        pos2(bar.right() - ITEM_BAR / 2.0 - 1.0, bar.center().y),
        vec2(ITEM_BAR, ITEM_BAR),
    );
    if bar.width() < ITEM_BAR * 3.0 || !clip.contains_rect(mute_rect) {
        return false;
    }
    let mute = ui.interact(mute_rect, mute_id, Sense::click());
    painter.text(
        mute_rect.center(),
        Align2::CENTER_CENTER,
        if muted { "🔇" } else { "🔊" },
        FontId::proportional(9.5),
        if mute.hovered() { theme.accent } else { text },
    );
    let clicked = mute.clicked();
    mute.on_hover_text(if muted {
        tr("timeline.item.unmute")
    } else {
        tr("timeline.item.mute")
    });
    clicked
}

/// One graph layer's lane: its items, each with a header bar along its top.
struct LayerLane<'a> {
    layer: &'a LayerView,
    rect: Rect,
    /// The visible part of the lanes.
    clip: Rect,
    /// A graph is being dragged over this lane.
    drop_target: bool,
}

impl LayerLane<'_> {
    fn show(
        &self,
        ui: &Ui,
        painter: &egui::Painter,
        model: &TimelineModel,
        view: &TimelineView,
        theme: &Theme,
        response: &mut TimelineResponse,
    ) {
        let lane = self.rect;
        if !lane.intersects(self.clip) {
            return;
        }
        painter.rect_filled(lane, CornerRadius::same(3), theme.lane_bg);
        let (outline, width) = if self.drop_target {
            (theme.accent, 1.5)
        } else {
            (theme.lane_outline, 1.0)
        };
        painter.rect_stroke(
            lane,
            CornerRadius::same(3),
            Stroke::new(width, outline),
            egui::StrokeKind::Inside,
        );
        for (k, item) in self.layer.items.iter().enumerate() {
            let block = Rect::from_min_max(
                pos2(view.x(lane.left(), item.position), lane.top() + 1.0),
                pos2(
                    view.x(lane.left(), item.position + item.length),
                    lane.bottom() - 1.0,
                ),
            );
            if block.intersect(self.clip).width() <= 0.0 {
                continue;
            }
            self.item(ui, painter, model, view, theme, (k, item, block), response);
        }
    }

    /// Item `k`, drawn in `block`: a coloured body and the header bar that drags it and mutes
    /// it. A drag only shows where the item would land; the move is made when it is released,
    /// because the model trims what the item lands on.
    #[allow(clippy::too_many_arguments)]
    fn item(
        &self,
        ui: &Ui,
        painter: &egui::Painter,
        model: &TimelineModel,
        view: &TimelineView,
        theme: &Theme,
        (k, item, block): (usize, &GraphItemView, Rect),
        response: &mut TimelineResponse,
    ) {
        let (layer, index) = (self.layer, self.layer.index);
        let bar_of = |block: Rect| {
            Rect::from_min_max(block.min, pos2(block.right(), block.top() + ITEM_BAR))
        };
        let grab = ui.interact(
            bar_of(block).intersect(self.clip),
            ui.id().with(("layer-item-bar", index, k)),
            Sense::click_and_drag(),
        );
        let hovered = grab.hovered() || grab.dragged();
        let selected = layer.selected_item == Some(k);
        let ctx = ui.ctx();
        let drag_id = ui.id().with(("layer-item-drag", index, k));
        let landing_id = drag_id.with("landing");
        let select = LayerAction::SelectItem {
            layer: index,
            item: k,
        };
        edge_handles(
            ui,
            model,
            view,
            self.clip,
            block,
            Some((item.position, item.position + item.length)),
            (("layer-item-edge", index, k), drag_id),
            &|| layer_snap_targets(model, Some((index, k))),
            tr("timeline.layer.item.edge"),
            |edge, to| {
                response.layer_actions.push(LayerAction::TrimItem {
                    layer: index,
                    item: k,
                    edge,
                    to,
                });
            },
        );
        if grab.drag_started_by(PointerButton::Primary) {
            if !selected {
                response.layer_actions.push(select.clone());
            }
            let drag = ItemDrag {
                edge: None,
                origin: item.position,
                length: item.length,
                targets: layer_snap_targets(model, Some((index, k))),
            };
            ctx.data_mut(|d| d.insert_temp(drag_id, drag));
        }
        let mut landing = None;
        if grab.dragged_by(PointerButton::Primary)
            && let Some(drag) = ctx.data(|d| d.get_temp::<ItemDrag>(drag_id))
            && let Some(to) = drag_to(ui, model, view, &drag)
        {
            let to = to.max(0.0);
            ctx.data_mut(|d| d.insert_temp(landing_id, to));
            landing = Some(to);
        }
        if grab.drag_stopped()
            && let Some(to) = ctx.data(|d| d.get_temp::<f64>(landing_id))
        {
            ctx.data_mut(|d| d.remove::<f64>(landing_id));
            if to != item.position {
                response.layer_actions.push(LayerAction::MoveItem {
                    layer: index,
                    item: k,
                    delta: to - item.position,
                });
            }
        }
        // Where the item is drawn: where it would land, while it is dragged.
        let shown = match landing {
            Some(to) => block.translate(vec2(((to - item.position) * view.px_per_sec) as f32, 0.0)),
            None => block,
        };
        let bar = bar_of(shown);
        let base = theme.graph_block;
        let mut fill = if hovered {
            base.gamma_multiply(1.2)
        } else {
            base
        };
        if item.muted || layer.silenced {
            fill = fill.gamma_multiply(0.4);
        }
        painter.rect_filled(shown, CornerRadius::same(3), fill);
        if item_bar(
            ui,
            painter,
            self.clip,
            theme,
            bar,
            (shown.intersect(self.clip).left(), &item.name, item.muted),
            hovered,
            ui.id().with(("layer-item-mute", index, k)),
        ) {
            response.layer_actions.push(LayerAction::ToggleItemMute {
                layer: index,
                item: k,
            });
        }
        if selected {
            painter.rect_stroke(
                shown,
                CornerRadius::same(3),
                Stroke::new(2.0, theme.accent),
                egui::StrokeKind::Inside,
            );
        }

        if grab.clicked() || (grab.secondary_clicked() && !selected) {
            response.layer_actions.push(select);
        }
        if grab.double_clicked() {
            response.layer_actions.push(LayerAction::OpenItem {
                layer: index,
                item: k,
            });
        }
        grab.context_menu(|ui| graph_item_menu(ui, model, index, k, response));
        if hovered {
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
        }
        grab.on_hover_text(tr("timeline.layer.item.drag"));
    }
}

/// Where a drag of graph item `k` of layer `layer` can snap: the timeline's start, the playhead,
/// and the edges of every track item and of the layers' other items.
fn layer_snap_targets(model: &TimelineModel, skip: Option<(usize, usize)>) -> Vec<f64> {
    let mut targets = vec![0.0, model.playhead as f64 / model.frame_rate];
    for track in &model.tracks {
        for item in &track.items {
            if let Some((start, end)) = track.item_span(item) {
                targets.extend([start, end]);
            }
        }
    }
    for l in &model.layers {
        for (j, item) in l.items.iter().enumerate() {
            if skip != Some((l.index, j)) {
                targets.extend([item.position, item.position + item.length]);
            }
        }
    }
    targets
}

/// A graph item's right-click menu: split at the playhead and delete.
fn graph_item_menu(
    ui: &mut Ui,
    model: &TimelineModel,
    layer: usize,
    item: usize,
    response: &mut TimelineResponse,
) {
    let at = model.playhead as f64 / model.frame_rate;
    if ui
        .add(egui::Button::new(tr("timeline.item.split")).shortcut_text("S"))
        .clicked()
    {
        response
            .layer_actions
            .push(LayerAction::SplitItem { layer, item, at });
        ui.close();
    }
    if ui
        .add(egui::Button::new(tr("timeline.item.delete")).shortcut_text("Del"))
        .clicked()
    {
        response
            .layer_actions
            .push(LayerAction::DeleteItem { layer, item });
        ui.close();
    }
}

/// The widgets of a graph layer's header: its name, mute and solo.
fn layer_header(ui: &mut Ui, layer: &LayerView, response: &mut TimelineResponse) {
    ui.label("▤");
    let edit = name_edit(
        ui,
        ui.id().with(("layer-name", layer.index)),
        &layer.name,
        |e| e.desired_width(NAME_WIDTH),
    );
    edit.response.on_hover_text(tr("timeline.layer.name.help"));
    if let Some(name) = edit.committed {
        response
            .layer_actions
            .push(LayerAction::Rename(layer.index, name));
    }
    let mute = egui::Button::new(if layer.muted { "🔇" } else { "🔊" }).frame(false);
    if ui
        .add(mute)
        .on_hover_text(if layer.muted {
            tr("timeline.layer.unmute")
        } else {
            tr("timeline.layer.mute")
        })
        .clicked()
    {
        response
            .layer_actions
            .push(LayerAction::ToggleMute(layer.index));
    }
    if ui
        .add(egui::Button::selectable(layer.solo, "S"))
        .on_hover_text(tr("timeline.layer.solo"))
        .clicked()
    {
        response
            .layer_actions
            .push(LayerAction::ToggleSolo(layer.index));
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
    let (from, to) = ui.input(|i| (i.pointer.press_origin(), i.pointer.interact_pos()));
    let want = drag.origin + f64::from(to?.x - from?.x) / view.px_per_sec;
    let length = drag.edge.is_none().then_some(drag.length);
    Some(snap_time(ui, model, view, want, length, &drag.targets))
}

/// `want` (seconds) snapped to the grid and `targets`, as an item of `length` starting there
/// snaps (its start, or its end): unchanged when snapping is off or Shift is held.
fn snap_time(
    ui: &Ui,
    model: &TimelineModel,
    view: &TimelineView,
    want: f64,
    length: Option<f64>,
    targets: &[f64],
) -> f64 {
    if !model.snap || ui.input(|i| i.modifiers.shift) {
        return want;
    }
    let grid = RulerGrid::new(model, view.px_per_sec);
    let line = |t: f64| grid.time(((t - grid.origin) / grid.minor).round() as i64);
    let edges = match length {
        Some(length) => vec![want, want + length],
        None => vec![want],
    };
    let reach = f64::from(SNAP_DISTANCE) / view.px_per_sec;
    want + snap_offset(&edges, targets, line, reach)
}

/// Height of the ghost row drawn where a dropped resource or graph would make a new track or
/// layer.
const GHOST_ROW_HEIGHT: f32 = 26.0;

/// Where the drop ghost goes (screen space, before clipping): the lane of the track or layer
/// that takes it, or a ghost row (`true`) where a new one would be made.
fn ghost_band(
    areas: &Areas,
    model: &TimelineModel,
    view: &TimelineView,
    ghost: &DropGhost,
    response: &TimelineResponse,
) -> (Rect, bool) {
    let row = |y: f32| {
        Rect::from_center_size(
            pos2(areas.lanes.center().x, y),
            vec2(areas.lanes.width(), GHOST_ROW_HEIGHT),
        )
    };
    match &ghost.target {
        GhostTarget::Media { lands_on, new_row } => {
            if let Some(r) = response
                .row_under_pointer
                .filter(|&r| lands_on.get(r).copied().unwrap_or(false))
            {
                return (areas.lane(r, view.scroll_y), false);
            }
            (row(areas.row_top(*new_row, view.scroll_y)), true)
        }
        GhostTarget::Graph => match response.layer_under_pointer {
            Some(index) => {
                let display = model
                    .layers
                    .iter()
                    .position(|l| l.index == index)
                    .unwrap_or(0);
                (areas.layer_lane(display, view.scroll_y), false)
            }
            // A new layer goes on top.
            None => (
                row(areas.body.top() - view.scroll_y + GHOST_ROW_HEIGHT / 2.0),
                true,
            ),
        },
    }
}

/// The drop ghost: a translucent item from `x0` to `x1` in `band`, or a start marker when the
/// length isn't known, labelled with what is dragged.
fn paint_ghost(
    painter: &egui::Painter,
    theme: &Theme,
    band: Rect,
    x0: f32,
    x1: Option<f32>,
    name: &str,
) {
    let block = Rect::from_min_max(
        pos2(x0, band.top()),
        pos2(x1.unwrap_or(x0 + 2.0).max(x0 + 2.0), band.bottom()),
    );
    painter.rect(
        block,
        CornerRadius::same(3),
        theme.accent.gamma_multiply(0.25),
        Stroke::new(1.5, theme.accent),
        egui::StrokeKind::Inside,
    );
    painter.text(
        pos2(x0 + 6.0, band.top() + 3.0),
        Align2::LEFT_TOP,
        name,
        FontId::proportional(11.0),
        theme.accent,
    );
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
    track: &TrackView,
    view: &TimelineView,
    lanes_left: f32,
    (item, duration): (&Item, f64),
    content: Rect,
    clip: Rect,
    response: &mut TimelineResponse,
) {
    let Some(own) = &track.thumbnails else {
        return;
    };
    let fps = own.rate;
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
    let aspect = own
        .frames
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
        let Some(thumbnail) = nearest(&own.frames, frame) else {
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
    let asked = response
        .wanted_thumbnails
        .iter()
        .filter(|(name, _)| *name == track.name)
        .count();
    let room = MAX_THUMBNAIL_REQUEST.saturating_sub(asked);
    response.wanted_thumbnails.extend(
        frames
            .into_iter()
            .take(room)
            .map(|frame| (track.name.clone(), frame)),
    );
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

/// Whether dropping track `from` of the shown tree `shape` (see [`drop_depths`]) in gap `slot`,
/// `depth` folders deep, changes anything: not when it lands where it is, at its own depth.
pub fn moves(shape: &[(u32, bool)], from: usize, slot: usize, depth: u32) -> bool {
    let end = (from + 1..shape.len())
        .find(|&j| shape[j].0 <= shape[from].0)
        .unwrap_or(shape.len());
    !((slot == from || slot == end) && depth == shape[from].0)
}

/// The glyph a track is marked with, in its header and while it is dragged.
fn track_icon(track: &TrackView) -> &'static str {
    match track.kind {
        _ if track.folder => "🗀",
        Some(TrackKind::Video) => "▣",
        Some(TrackKind::Audio) => "♪",
        None => "○",
    }
}

/// The widgets of a track's header: a folder's open/closed toggle, then name, mute, solo and
/// remove; then the video's size and frame rate, or the volume of an audio track or folder and,
/// at the top of the tree, its bus when the project has several (or the track's is gone); and a
/// link mark when it is linked.
fn track_header(
    ui: &mut Ui,
    track: &TrackView,
    buses: &[String],
    row: usize,
    response: &mut TimelineResponse,
) {
    // Labels don't select text here, so a drag that starts on one reaches the header below and
    // reorders the track.
    ui.style_mut().interaction.selectable_labels = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.horizontal(|ui| {
            if track.folder {
                let (glyph, help) = if track.collapsed {
                    ("⏵", tr("timeline.folder.expand"))
                } else {
                    ("⏷", tr("timeline.folder.collapse"))
                };
                if ui
                    .add(egui::Button::new(glyph).frame(false))
                    .on_hover_text(help)
                    .clicked()
                {
                    response.actions.push(TrackAction::ToggleCollapsed(row));
                }
            }
            ui.label(track_icon(track));
            let width = (NAME_WIDTH - track.depth as f32 * INDENT).max(MIN_NAME_WIDTH);
            let edit = name_edit(ui, ui.id().with(("track-name", row)), &track.name, |e| {
                e.desired_width(width)
            });
            let help = if track.folder {
                tr("timeline.folder.name.help")
            } else {
                tr("timeline.track.name.help")
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
            if ui
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
                _ if track.folder => audio_controls(ui, track, buses, row, response),
                Some(TrackKind::Video) => {
                    if let Some(details) = &track.details {
                        ui.add(
                            egui::Label::new(egui::RichText::new(details).weak().small())
                                .truncate(),
                        );
                    }
                }
                Some(TrackKind::Audio) => audio_controls(ui, track, buses, row, response),
                None => {
                    ui.weak(tr("timeline.track.empty"));
                }
            }
        });
    });
}

/// The volume of an audio track or folder, and at the top of the tree its bus when there is a
/// choice to make.
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
    if track.depth == 0 && (buses.len() > 1 || !buses.contains(&track.bus)) {
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
    fn dropping_a_track_where_it_is_moves_nothing() {
        // A folder holding one track, then a track at the top: gaps 0..=3.
        let shape = [(0, true), (1, false), (0, false)];
        assert!(!moves(&shape, 0, 0, 0));
        assert!(!moves(&shape, 0, 2, 0), "just below its own subtree");
        assert!(moves(&shape, 0, 3, 0));
        assert!(!moves(&shape, 1, 1, 1));
        assert!(moves(&shape, 1, 2, 0), "out of the folder in place");
        assert!(moves(&shape, 2, 2, 1), "into the folder in place");
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
            ..TrackView::new("b", Some(TrackKind::Audio))
        };
        let tracks = [TrackView::new("a", Some(TrackKind::Video)), tall];
        let areas = Areas::new(
            Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 400.0)),
            &tracks,
            0,
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
    fn graph_layers_sit_above_the_tracks() {
        let tracks = [TrackView::new("a", Some(TrackKind::Video))];
        let areas = Areas::new(
            Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 400.0)),
            &tracks,
            2,
        );
        let body = RULER_HEIGHT;
        // Two layers, then the track.
        assert_eq!(areas.row_top(0, 0.0), body + 2.0 * LAYER_HEIGHT);
        assert_eq!(areas.layer_at(0.0, body + 5.0, 2), Some(0));
        assert_eq!(areas.layer_at(0.0, body + LAYER_HEIGHT + 5.0, 2), Some(1));
        assert_eq!(
            areas.layer_at(0.0, body + 2.0 * LAYER_HEIGHT + 5.0, 2),
            None
        );
        assert_eq!(areas.row_at(0.0, body + 5.0), None, "a layer is no track");
        assert_eq!(areas.row_at(0.0, body + 2.0 * LAYER_HEIGHT + 5.0), Some(0));
        // Layers scroll with the tracks.
        assert_eq!(areas.layer_lane(0, 10.0).top(), body - 10.0 + 2.0);
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
            layers: Vec::new(),
            tracks: Vec::new(),
            selected_track: None,
            loop_region: None,
            tempo: Tempo {
                bpm: 120.0,
                beats_per_bar: 4,
                offset_secs: 0.25,
            },
            mode,
            buses: Vec::new(),
            snap: true,
            drop_ghost: None,
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
