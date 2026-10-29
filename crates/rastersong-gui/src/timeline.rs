//! The timeline: a ruler, then three sections divided by a thick line, like an editor's: the
//! graph tracks (graph items), the video tracks (thumbnails) and the audio tracks (waveforms),
//! each a lane of items with a header on the left.
//!
//! - The scroll wheel zooms time around the pointer (over the headers it scrolls the tracks);
//!   middle- or right-drag pans in both directions; F fits the whole project.
//! - Click or drag on the ruler to seek; drag over empty lane space to box-select items; drag an
//!   item by its header bar to move it (with the other selected items and the items grouped with
//!   them).
//! - Click an item's header bar to select it, Ctrl+click to add or remove it; drag an item's
//!   edge to trim it, Alt+drag to change its rate. Drags snap to the grid, item edges and the
//!   playhead while snapping is on; Shift drags freely.
//! - Over the timeline, S splits at the playhead (the selected items, or with none selected
//!   every item under it), Delete removes the selected items, and Ctrl+C, Ctrl+X and Ctrl+V
//!   copy, cut and paste them at the playhead. Ctrl+G groups the selected items, so they move,
//!   trim, split and delete together; Ctrl+Shift+G ungroups them.
//! - Ctrl+drag on the ruler makes a loop region (or moves one of its edges).
//! - Drag a header to reorder the tracks of its section, its bottom edge to change the track's
//!   height; right-click it for the track menu. There is no way to make an empty track: a track
//!   appears when a resource is placed. Dropped between two tracks (or above the first or below
//!   the last of a section), at the ghost line, a resource gets a new track there; the track menu's
//!   "New track above" and "New track below" place a resource the same way.
//! - A graph dragged from the Resources panel lands on the graph track under the pointer, or on
//!   a new one. A graph item is selected, moved (when released), trimmed and muted like any item;
//!   double-click it to open its graph.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Key, PointerButton, Rect, Sense, Stroke,
    TextureId, Ui, UiBuilder, Vec2, pos2, vec2,
};
use rastersong_engine::{
    Edge, Item, LoopRegion, ResourceId, Tempo, TimelineMode, TrackKind, Waveform, snap_offset,
};
use rastersong_lang::{tr, tr_args};

use crate::name_edit::name_edit;
use crate::resources::DraggedGraph;
use crate::theme::Theme;

/// Width of the track header column.
pub const HEADER_WIDTH: f32 = 220.0;
/// Gap between the headers and the lanes.
const GAP: f32 = 6.0;
/// How far a header's widgets sit from its left edge, past the bar of its kind's colour.
const KIND_INSET: f32 = 18.0;
/// Size of a track's name in its header.
const NAME_SIZE: f32 = 12.5;
/// Height of a header's name, and of the name with the row of details under it.
const NAME_HEIGHT: f32 = 18.0;
const NAME_ROWS_HEIGHT: f32 = 40.0;
/// Width of a header's M and S buttons.
const TOGGLE_WIDTH: f32 = 22.0;
const RULER_HEIGHT: f32 = 22.0;
/// The ruler in tempo mode, in two tiers: bars along the top, beats under them.
const TEMPO_RULER_HEIGHT: f32 = 38.0;
/// Where the tempo ruler's beat tier starts, below its top.
const BEAT_TIER: f32 = 23.0;
/// Smallest spacing (pixels) between beat labels on the tempo ruler.
const MIN_BEAT_LABEL_SPACING: f32 = 34.0;
/// Narrowest a bar can be drawn (pixels) and still be shaded.
const MIN_SHADED_BAR: f32 = 8.0;
/// A track's height until the user changes it: tall enough for a header's two rows of widgets.
pub const LANE_HEIGHT: f32 = 58.0;
/// A graph track's height.
pub const GRAPH_LANE_HEIGHT: f32 = 40.0;
/// The shortest and tallest a track can be made.
pub const LANE_HEIGHT_RANGE: std::ops::RangeInclusive<f32> = 46.0..=320.0;
/// The space between two sections, with the thick line that divides them in its middle.
const SECTION_GAP: f32 = 10.0;
/// Height of the bar along the top of each item, which drags it and holds its mute button.
const ITEM_BAR: f32 = 14.0;
/// How close (pixels) to an item's edge the pointer must be to trim it.
const EDGE_GRAB: f32 = 5.0;
/// How close (pixels) a dragged edge must come to a snap target to snap to it.
const SNAP_DISTANCE: f32 = 8.0;
/// How close (pixels) to the bottom edge of a header the pointer must be to resize the track.
const RESIZE_GRAB: f32 = 3.0;
/// Height of the drop zone below an empty section, which names what to drop there.
const EMPTY_SECTION_HEIGHT: f32 = 30.0;
/// Height of the space below a section's last track, where a dropped resource's ghost row shows.
const TAIL_HEIGHT: f32 = 6.0;
/// How close (pixels) to the line between two tracks a dragged resource must be to make a new
/// track there.
const SLOT_GRAB: f32 = 5.0;
/// Smallest spacing between labelled ruler ticks, in pixels.
const MIN_TICK_SPACING: f64 = 90.0;
/// How far out the view can zoom, as a fraction of the zoom that fits the whole project.
const MIN_ZOOM_OF_FIT: f64 = 0.5;
/// How far in the view can zoom: this many frames across the lanes.
const MIN_VISIBLE_FRAMES: f64 = 4.0;
/// Most thumbnails asked for at once.
const MAX_THUMBNAIL_REQUEST: usize = 200;

/// One video or audio track as the timeline shows it.
#[derive(Debug, Clone)]
pub struct TrackView {
    pub name: String,
    /// Its section.
    pub kind: TrackKind,
    /// The track plays nothing yet: it takes the first resource of its kind dropped on it.
    pub empty: bool,
    /// Length of the track's file in seconds, once known.
    pub duration: Option<f64>,
    pub items: Vec<Item>,
    pub muted: bool,
    pub solo: bool,
    /// Left out of the mix, by its mute or another track's solo: drawn dimmed.
    pub silenced: bool,
    /// Level in the mix, 0 to 1 (audio tracks).
    pub volume: f32,
    /// Height of the track's lane and header.
    pub height: f32,
    pub waveform: Option<Arc<Waveform>>,
    /// The output bus an audio track is summed into in the mix.
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
    /// A track of `kind` with nothing loaded yet, at the default height.
    pub fn new(name: impl Into<String>, kind: TrackKind) -> Self {
        Self {
            name: name.into(),
            kind,
            empty: false,
            duration: None,
            items: Vec::new(),
            muted: false,
            solo: false,
            silenced: false,
            volume: 1.0,
            height: LANE_HEIGHT,
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

/// A graph item as the timeline shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphItemView {
    /// The graph's name.
    pub name: String,
    pub position: f64,
    pub length: f64,
    pub muted: bool,
}

/// One graph track as the timeline shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphTrackView {
    pub name: String,
    pub muted: bool,
    pub items: Vec<GraphItemView>,
    /// The selected items, by index into [`Self::items`].
    pub selected_items: Vec<usize>,
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

/// What the timeline shows.
#[derive(Debug, Clone)]
pub struct TimelineModel<'a> {
    pub frame_count: usize,
    pub frame_rate: f64,
    pub playhead: usize,
    pub cached: &'a [Range<usize>],
    /// The graph tracks, top first.
    pub graph_tracks: Vec<GraphTrackView>,
    /// The video tracks, top first, then the audio tracks.
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
    /// The resources the track menu can place on a new track: id, name and the kind of track.
    pub resources: Vec<(ResourceId, String, TrackKind)>,
    /// A resource or graph being dragged from the Resources panel, for the drop preview.
    pub drop_ghost: Option<DropGhost>,
}

/// What is being dragged over the timeline from the Resources panel. While the pointer is over
/// the timeline, ghosts show where it would land.
#[derive(Debug, Clone, PartialEq)]
pub enum DropGhost {
    /// Resources for tracks (a video and its sound land together), each with where it lands.
    Media(Vec<GhostItem>),
    /// A graph, which lands on the graph track under the pointer or a new one; see
    /// [`GraphDrop`].
    Graph { name: String, length: f64 },
}

/// One resource of a drop and where it would land.
#[derive(Debug, Clone, PartialEq)]
pub struct GhostItem {
    pub name: String,
    /// Its length in seconds. `None` for a resource no track plays yet, whose length isn't
    /// known: the ghost then marks only where it starts.
    pub length: Option<f64>,
    pub row: GhostRow,
}

/// Where a dropped resource lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GhostRow {
    /// The track in this row of [`TimelineModel::tracks`].
    Track(usize),
    /// A new track in this section: at the ghost line when the pointer is on one, else at the
    /// bottom.
    New(TrackKind),
}

/// Where a graph dropped on the timeline lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphDrop {
    /// The graph track in this row of [`TimelineModel::graph_tracks`].
    Track(usize),
    /// A new graph track on top.
    NewTrack,
    /// A new graph track before the one now at this index (the number of graph tracks for the
    /// bottom): the pointer is on the line between two graph tracks, or below the last.
    NewTrackAt(usize),
}

/// Something the user did to a video or audio track. Tracks are named by their row in
/// [`TimelineModel::tracks`].
#[derive(Debug, Clone, PartialEq)]
pub enum TrackAction {
    Select(usize),
    /// The track menu asked for a new track before the track now at index `slot` (one past the
    /// last of a section for below it) playing this resource.
    NewTrack {
        slot: usize,
        resource: ResourceId,
    },
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
    /// Ctrl+A: select every item of every section.
    SelectAll,
    /// Group the selected items, and ungroup them.
    GroupItems,
    UngroupItems,
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
    /// A track's header was dragged to a new place in its section: the track in row `from`
    /// moves before the track in row `slot` (one past its section's last for the bottom).
    Move {
        from: usize,
        slot: usize,
    },
    Rename(usize, String),
    /// Route the track to the named output bus.
    SetBus(usize, String),
    Remove(usize),
}

/// Something the user did to a graph track or its items. Graph tracks are named by their row in
/// [`TimelineModel::graph_tracks`].
#[derive(Debug, Clone, PartialEq)]
pub enum GraphAction {
    SelectItem {
        track: usize,
        item: usize,
    },
    /// The item was dragged by `delta` seconds, and released.
    MoveItem {
        track: usize,
        item: usize,
        delta: f64,
    },
    /// An edge of the item was dragged to time `to` (seconds).
    TrimItem {
        track: usize,
        item: usize,
        edge: Edge,
        to: f64,
    },
    ToggleItemMute {
        track: usize,
        item: usize,
    },
    /// The item's menu asked to split it at time `at` (the playhead).
    SplitItem {
        track: usize,
        item: usize,
        at: f64,
    },
    DeleteItem {
        track: usize,
        item: usize,
    },
    /// The item was double-clicked: open its graph in the editor.
    OpenItem {
        track: usize,
        item: usize,
    },
    ToggleMute(usize),
    Rename(usize, String),
    Remove(usize),
    /// A graph track's header was dragged: the track in row `from` moves before row `slot`.
    Move {
        from: usize,
        slot: usize,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimelineResponse {
    pub seek: Option<usize>,
    pub actions: Vec<TrackAction>,
    pub graph_actions: Vec<GraphAction>,
    /// Where a graph dragged over the timeline would land, if dropped now.
    pub graph_drop: Option<GraphDrop>,
    /// Frames whose thumbnails would fill the visible video items, with the track they are of.
    pub wanted_thumbnails: Vec<(String, usize)>,
    /// A new loop region (`Some(None)` to remove it).
    pub loop_region: Option<Option<LoopRegion>>,
    /// Screen x where the lanes start, for turning a pointer position into time.
    pub lanes_left: f32,
    /// The row of [`TimelineModel::tracks`] the pointer is over, for dropping a resource on a
    /// track.
    pub row_under_pointer: Option<usize>,
    /// Where a resource or graph dragged over the timeline would start (seconds), snapped like
    /// an item drag. A drop lands there, where its ghost was drawn.
    pub drop_at: Option<f64>,
    /// While a resource is dragged with the pointer on the line between two tracks: the index
    /// into [`TimelineModel::tracks`] of the track a new one would be put before.
    pub drop_slot: Option<usize>,
    /// Where the rows were drawn.
    pub layout: TimelineLayout,
}

/// Where the timeline's rows were drawn, in screen space (before clipping to the visible part).
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineLayout {
    /// The ruler along the top, over the lanes.
    pub ruler: Rect,
    /// The lane of each graph track, top first.
    pub graph_lanes: Vec<Rect>,
    /// The lane of each video and audio track, as [`TimelineModel::tracks`] lists them.
    pub lanes: Vec<Rect>,
    /// The header of each video and audio track.
    pub headers: Vec<Rect>,
    /// The space below the Graphs, Video and Audio sections' tracks, across the headers and lanes.
    pub tails: Vec<Rect>,
}

impl Default for TimelineLayout {
    fn default() -> Self {
        Self {
            ruler: Rect::NOTHING,
            graph_lanes: Vec::new(),
            lanes: Vec::new(),
            headers: Vec::new(),
            tails: Vec::new(),
        }
    }
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

    /// Whether tick `k` starts a bar (always false in time mode).
    fn on_bar(&self, k: i64) -> bool {
        self.bars.is_some_and(|(minor_beats, per_bar)| {
            let bars = k as f64 * minor_beats / f64::from(per_bar);
            (bars - bars.round()).abs() < 1e-6
        })
    }

    /// `seconds` moved to the nearest tick (in tempo mode) or frame (in time mode).
    fn snap(&self, seconds: f64, frame_rate: f64) -> f64 {
        match self.bars {
            Some(_) => self.time((((seconds - self.origin) / self.minor).round()) as i64),
            None => (seconds * frame_rate).round() / frame_rate,
        }
    }
}

/// The ruler's height in `mode`.
fn ruler_height(mode: TimelineMode) -> f32 {
    match mode {
        TimelineMode::Time => RULER_HEIGHT,
        TimelineMode::Tempo => TEMPO_RULER_HEIGHT,
    }
}

/// Beats from `tempo`'s first beat to `seconds` (negative before it).
pub fn beats_at(tempo: &Tempo, seconds: f64) -> f64 {
    let tempo = tempo.sanitized();
    (seconds - tempo.offset_secs) / tempo.seconds_per_beat()
}

/// The bar, beat and sixteenth `beats` falls in, each counting from 1.
fn bar_beat_parts(beats: f64, beats_per_bar: u32) -> (i64, i64, i64) {
    let sixteenths = (beats * 4.0 + 1e-6).floor() as i64;
    let per_bar = i64::from(beats_per_bar.max(1)) * 4;
    let in_bar = sixteenths.rem_euclid(per_bar);
    (
        sixteenths.div_euclid(per_bar) + 1,
        in_bar / 4 + 1,
        in_bar % 4 + 1,
    )
}

/// `seconds` as `bar.beat` at `tempo`, or `bar.beat.sixteenth` when it falls between beats: a
/// point on the beat grid, like a loop edge.
pub fn bar_beat(tempo: &Tempo, seconds: f64) -> String {
    let beats = (beats_at(tempo, seconds) * 4.0).round() / 4.0;
    match bar_beat_parts(beats, tempo.sanitized().beats_per_bar) {
        (bar, beat, 1) => format!("{bar}.{beat}"),
        (bar, beat, sixteenth) => format!("{bar}.{beat}.{sixteenth}"),
    }
}

/// `seconds` as `bar.beat.sixteenth` at `tempo`, counting the sixteenth it is in: where the
/// playhead is.
pub fn bar_beat_tick(tempo: &Tempo, seconds: f64) -> String {
    let (bar, beat, sixteenth) =
        bar_beat_parts(beats_at(tempo, seconds), tempo.sanitized().beats_per_bar);
    format!("{bar}.{beat}.{sixteenth}")
}

/// `seconds` as `m:ss.cc`.
pub fn timecode(seconds: f64) -> String {
    let total = seconds.max(0.0);
    let minutes = (total / 60.0).floor();
    format!("{}:{:05.2}", minutes as u64, total - minutes * 60.0)
}

/// `seconds` as `m:ss` when it falls on a whole second, else as [`timecode`] does.
pub fn short_timecode(seconds: f64) -> String {
    let total = seconds.max(0.0);
    if (total - total.round()).abs() < 0.005 {
        let whole = total.round() as u64;
        format!("{}:{:02}", whole / 60, whole % 60)
    } else {
        timecode(total)
    }
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

/// The timeline's three sections, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Graphs,
    Video,
    Audio,
}

impl Section {
    const ALL: [Self; 3] = [Self::Graphs, Self::Video, Self::Audio];

    fn index(self) -> usize {
        match self {
            Self::Graphs => 0,
            Self::Video => 1,
            Self::Audio => 2,
        }
    }

    fn of(kind: TrackKind) -> Self {
        match kind {
            TrackKind::Video => Self::Video,
            TrackKind::Audio => Self::Audio,
        }
    }
}

/// The rectangles a timeline divides into, and where each row starts and ends, measured from
/// the top of the body before scrolling.
struct Areas {
    headers: Rect,
    lanes: Rect,
    ruler: Rect,
    /// Below the ruler, headers and lanes: the part that scrolls vertically.
    body: Rect,
    /// Each graph track's (top, bottom), top first.
    graph_rows: Vec<(f32, f32)>,
    /// Each video and audio track's (top, bottom), in [`TimelineModel::tracks`] order.
    rows: Vec<(f32, f32)>,
    /// Where each section's tracks are in [`TimelineModel::tracks`] (none for the Graphs).
    sections: [Range<usize>; 3],
    /// The top of the space below each section's tracks.
    tails: [f32; 3],
    /// The thick lines between the sections.
    dividers: [f32; 2],
    /// The bottom of everything.
    bottom: f32,
}

impl Areas {
    fn new(area: Rect, ruler_height: f32, graph_tracks: usize, tracks: &[TrackView]) -> Self {
        let headers = Rect::from_min_max(
            pos2(area.left(), area.top() + ruler_height),
            pos2(area.left() + HEADER_WIDTH, area.bottom()),
        );
        let lanes = Rect::from_min_max(pos2(headers.right() + GAP, area.top()), area.max);
        let ruler = Rect::from_min_max(lanes.min, pos2(lanes.right(), area.top() + ruler_height));
        let body = Rect::from_min_max(pos2(area.left(), ruler.bottom()), area.max);
        let videos = tracks
            .iter()
            .take_while(|t| t.kind == TrackKind::Video)
            .count();
        let sections = [0..0, 0..videos, videos..tracks.len()];
        let tail_height = |section: Section| {
            let empty = match section {
                Section::Graphs => graph_tracks == 0,
                _ => sections[section.index()].is_empty(),
            };
            if empty {
                EMPTY_SECTION_HEIGHT
            } else {
                TAIL_HEIGHT
            }
        };
        let mut y = 0.0;
        let mut graph_rows = Vec::with_capacity(graph_tracks);
        for _ in 0..graph_tracks {
            graph_rows.push((y, y + GRAPH_LANE_HEIGHT));
            y += GRAPH_LANE_HEIGHT;
        }
        let mut tails = [0.0; 3];
        let mut dividers = [0.0; 2];
        let mut rows = Vec::with_capacity(tracks.len());
        for section in Section::ALL {
            if section != Section::Graphs {
                for track in &tracks[sections[section.index()].clone()] {
                    rows.push((y, y + track.height));
                    y += track.height;
                }
            }
            tails[section.index()] = y;
            y += tail_height(section);
            if section != Section::Audio {
                dividers[section.index()] = y + SECTION_GAP / 2.0;
                y += SECTION_GAP;
            }
        }
        Self {
            headers,
            lanes,
            ruler,
            body,
            graph_rows,
            rows,
            sections,
            tails,
            dividers,
            bottom: y,
        }
    }

    /// The screen y of body offset `y`.
    fn y(&self, y: f32, scroll: f32) -> f32 {
        self.body.top() + y - scroll
    }

    /// A row's lane in screen space, before clipping.
    fn lane_of(&self, (top, bottom): (f32, f32), scroll: f32) -> Rect {
        Rect::from_min_max(
            pos2(self.lanes.left(), self.y(top, scroll)),
            pos2(self.lanes.right(), self.y(bottom, scroll)),
        )
        .shrink2(vec2(0.0, 2.0))
    }

    fn header_of(&self, (top, bottom): (f32, f32), scroll: f32) -> Rect {
        Rect::from_min_max(
            pos2(self.headers.left(), self.y(top, scroll)),
            pos2(self.headers.left() + HEADER_WIDTH, self.y(bottom, scroll)),
        )
        .shrink2(vec2(0.0, 2.0))
    }

    /// Lane `row` of the video and audio tracks.
    fn lane(&self, row: usize, scroll: f32) -> Rect {
        self.lane_of(self.rows[row], scroll)
    }

    fn header(&self, row: usize, scroll: f32) -> Rect {
        self.header_of(self.rows[row], scroll)
    }

    fn graph_lane(&self, row: usize, scroll: f32) -> Rect {
        self.lane_of(self.graph_rows[row], scroll)
    }

    fn graph_header(&self, row: usize, scroll: f32) -> Rect {
        self.header_of(self.graph_rows[row], scroll)
    }

    /// The space below section `section`'s tracks across the headers and lanes, in screen space.
    fn tail(&self, section: Section, scroll: f32) -> Rect {
        let top = self.tails[section.index()];
        let height = match section {
            Section::Graphs if self.graph_rows.is_empty() => EMPTY_SECTION_HEIGHT,
            Section::Graphs => TAIL_HEIGHT,
            _ if self.sections[section.index()].is_empty() => EMPTY_SECTION_HEIGHT,
            _ => TAIL_HEIGHT,
        };
        Rect::from_min_max(
            pos2(self.headers.left(), self.y(top, scroll)),
            pos2(self.lanes.right(), self.y(top + height, scroll)),
        )
    }

    /// The body offset of the line a new track at index `slot` would sit on: the top of the
    /// track now there, or the bottom of the last of the section.
    fn slot_offset(&self, slot: usize) -> Option<f32> {
        let section = Section::ALL.into_iter().skip(1).find_map(|s| {
            let range = self.sections[s.index()].clone();
            (range.start..=range.end).contains(&slot).then_some(range)
        })?;
        if section.is_empty() {
            return None;
        }
        Some(if slot < section.end {
            self.rows[slot].0
        } else {
            self.rows[section.end - 1].1
        })
    }

    /// The body offset of the line a new graph track at index `slot` would sit on.
    fn graph_slot_offset(&self, slot: usize) -> f32 {
        match self.graph_rows.get(slot) {
            Some(row) => row.0,
            None => self.graph_rows.last().map_or(0.0, |row| row.1),
        }
    }

    /// The graph slot a dragged graph at screen y is on: the line between two graph tracks, or
    /// anywhere below the last one (down to the next section).
    fn graph_slot_at(&self, scroll: f32, y: f32) -> Option<usize> {
        let last = self.graph_rows.last()?.1;
        let y = self.offset(scroll, y);
        if y > last && y <= self.dividers[0] {
            return Some(self.graph_rows.len());
        }
        (0..=self.graph_rows.len())
            .find(|&slot| (self.graph_slot_offset(slot) - y).abs() <= SLOT_GRAB)
    }

    /// The slot whose line is within [`SLOT_GRAB`] of screen y, if any.
    fn slot_at(&self, scroll: f32, y: f32) -> Option<usize> {
        let y = self.offset(scroll, y);
        Section::ALL.into_iter().skip(1).find_map(|s| {
            let range = self.sections[s.index()].clone();
            if range.is_empty() {
                return None;
            }
            (range.start..=range.end).find(|&slot| {
                self.slot_offset(slot)
                    .is_some_and(|line| (line - y).abs() <= SLOT_GRAB)
            })
        })
    }

    /// The body offset of screen y.
    fn offset(&self, scroll: f32, y: f32) -> f32 {
        y - self.body.top() + scroll
    }

    /// Which video or audio track's row is at screen y, if any.
    fn row_at(&self, scroll: f32, y: f32) -> Option<usize> {
        let y = self.offset(scroll, y);
        self.rows
            .iter()
            .position(|&(top, bottom)| top <= y && y < bottom)
    }

    /// Which graph track's row is at screen y, if any.
    fn graph_row_at(&self, scroll: f32, y: f32) -> Option<usize> {
        let y = self.offset(scroll, y);
        self.graph_rows
            .iter()
            .position(|&(top, bottom)| top <= y && y < bottom)
    }
}

pub fn timeline(ui: &mut Ui, model: &TimelineModel, view: &mut TimelineView) -> TimelineResponse {
    let mut response = TimelineResponse::default();
    let theme = Theme::of(ui.ctx());
    let area = ui.available_rect_before_wrap();
    let background = ui.allocate_rect(area, Sense::click_and_drag());
    let areas = Areas::new(
        area,
        ruler_height(model.mode),
        model.graph_tracks.len(),
        &model.tracks,
    );
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
                .graph_tracks
                .iter()
                .flat_map(|t| t.items.iter().map(|i| (i.position, i.position + i.length))),
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
    if over(area) && !ui.ctx().egui_wants_keyboard_input() {
        item_keys(ui, model, &mut response);
    }
    // F shows the whole project.
    if over(area) && !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(Key::F)) {
        view.fit(width, duration);
    }
    view.clamp(width, limits, extent);
    view.scroll_y = view
        .scroll_y
        .clamp(0.0, (areas.bottom - areas.body.height()).max(0.0));
    let x = |seconds: f64| view.x(lanes_left, seconds);

    // Lane space (empty, or an item's content below its header bar): a click selects the row and
    // no items, and a drag box-selects the items it touches, adding to the selection with Ctrl
    // or Shift. Seeking belongs to the ruler.
    let body_lanes = Rect::from_min_max(pos2(lanes_left, areas.body.top()), areas.body.max);
    // A resource dragged onto the line between two tracks makes a new track there; elsewhere over
    // a lane it goes on that track.
    if matches!(model.drop_ghost, Some(DropGhost::Media(_))) {
        response.drop_slot = pointer
            .filter(|p| body_lanes.contains(*p))
            .and_then(|p| areas.slot_at(view.scroll_y, p.y));
    }
    response.row_under_pointer = pointer
        .filter(|p| body_lanes.contains(*p) && response.drop_slot.is_none())
        .and_then(|p| areas.row_at(view.scroll_y, p.y));
    let graph_dragged = egui::DragAndDrop::has_payload_of_type::<DraggedGraph>(ui.ctx());
    if graph_dragged && let Some(p) = pointer.filter(|p| areas.body.contains(*p)) {
        response.graph_drop = Some(match areas.graph_slot_at(view.scroll_y, p.y) {
            Some(slot) => GraphDrop::NewTrackAt(slot),
            None => match areas.graph_row_at(view.scroll_y, p.y) {
                Some(row) => GraphDrop::Track(row),
                None => GraphDrop::NewTrack,
            },
        });
    }
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
        // In tempo mode the band keeps to the bar tier, leaving the beats readable.
        let band = match model.mode {
            TimelineMode::Time => areas.ruler,
            TimelineMode::Tempo => areas.ruler.with_max_y(areas.ruler.top() + BEAT_TIER - 4.0),
        };
        paint_loop(&painter, region, x, band, body_lanes, theme);
    }
    let first = grid.tick_at_or_before(view.left);
    let last = grid.tick_at_or_before(view.seconds(lanes_left, areas.lanes.right())) + 1;
    if model.mode == TimelineMode::Tempo {
        let corner = Rect::from_min_max(
            pos2(area.left(), areas.ruler.top()),
            pos2(areas.headers.right(), areas.ruler.bottom()),
        );
        tempo_ruler(
            ui,
            &grid,
            first..=last,
            x,
            (corner, areas.ruler),
            body_lanes,
        );
        let first_beat = model.tempo.sanitized().offset_secs;
        first_beat_marker(ui, x(first_beat), first_beat, areas.ruler);
    }
    for k in (first..=last).filter(|_| model.mode == TimelineMode::Time) {
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
    for (row, track) in model.graph_tracks.iter().enumerate() {
        let lane = GraphLane {
            track,
            row,
            rect: areas.graph_lane(row, view.scroll_y),
            clip: body_lanes,
            drop_target: response.graph_drop == Some(GraphDrop::Track(row)),
        };
        lane.show(ui, &lane_painter, model, view, theme, &mut response);
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
    // The thick lines between the sections, across the headers and the lanes.
    let divider_painter = ui.painter_at(areas.body);
    for y in areas.dividers {
        let y = areas.y(y, view.scroll_y);
        divider_painter.line_segment(
            [pos2(areas.headers.left(), y), pos2(areas.lanes.right(), y)],
            Stroke::new(3.0, theme.lane_outline),
        );
    }
    if let Some(ghost) = &model.drop_ghost
        && let Some(p) = pointer.filter(|p| areas.body.contains(*p))
    {
        let want = view.seconds(lanes_left, p.x.max(lanes_left)).max(0.0);
        let length = match ghost {
            DropGhost::Media(items) => items.first().and_then(|i| i.length),
            DropGhost::Graph { length, .. } => Some(*length),
        };
        let at = snap_time(ui, model, view, want, length, &all_snap_targets(model));
        response.drop_at = Some(at);
        let graph_line = match (ghost, response.graph_drop) {
            (DropGhost::Graph { .. }, Some(GraphDrop::NewTrackAt(slot))) => {
                Some(areas.graph_slot_offset(slot))
            }
            _ => None,
        };
        if let Some(line) = response
            .drop_slot
            .and_then(|slot| areas.slot_offset(slot))
            .or(graph_line)
            .map(|offset| areas.y(offset, view.scroll_y))
        {
            ui.painter_at(areas.body).line_segment(
                [
                    pos2(areas.headers.left(), line),
                    pos2(areas.lanes.right(), line),
                ],
                Stroke::new(3.0, theme.accent),
            );
        }
        for (band, new_row, name, length) in ghost_bands(&areas, view, ghost, &response) {
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
                length.map(|l| x(at + l)),
                name,
            );
        }
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
    let graph_rows = model.graph_tracks.len();
    let mut dragging: Option<(f32, bool)> = None;
    for (row, track) in model.graph_tracks.iter().enumerate() {
        let rect = areas.graph_header(row, view.scroll_y);
        if !rect.intersects(header_clip) {
            continue;
        }
        header_painter.hline(
            rect.x_range(),
            rect.top() + 0.5,
            Stroke::new(1.0, theme.surfaces.divider),
        );
        // The background drags the track among the graph tracks and has the track's menu; it
        // goes under the widgets, so they keep their clicks.
        let grip = ui.interact(
            rect.intersect(header_clip),
            ui.id().with(("graph-track-grip", row)),
            Sense::click_and_drag(),
        );
        let movable = graph_rows > 1;
        if movable
            && (grip.dragged() || grip.drag_stopped())
            && let Some(p) = grip.interact_pointer_pos()
        {
            let mut boundaries: Vec<f32> = areas.graph_rows.iter().map(|r| r.0).collect();
            boundaries.push(areas.graph_rows.last().map_or(0.0, |r| r.1));
            let slot = drop_slot(areas.offset(view.scroll_y, p.y), &boundaries);
            let moves = slot != row && slot != row + 1;
            if grip.dragged() {
                dragging = Some((boundaries[slot], moves));
            } else if moves {
                response
                    .graph_actions
                    .push(GraphAction::Move { from: row, slot });
            }
        }
        if movable && grip.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            crate::widgets::drag_bubble(ui.ctx(), rect, GRAPH_ICON, &track.name);
        } else if movable && grip.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let grip = grip.on_hover_text(tr("timeline.graph_track.header.help"));
        grip.context_menu(|ui| {
            if ui.button(tr("timeline.track.remove")).clicked() {
                response.graph_actions.push(GraphAction::Remove(row));
                ui.close();
            }
        });
        kind_bar(
            ui,
            rect,
            header_clip,
            theme.graph_block,
            tr("timeline.graph_track.kind"),
            ("graph-track-kind", row),
        );
        header(ui, rect, header_clip, |ui| {
            graph_track_header(ui, track, row, &mut response);
        });
    }
    for (row, track) in model.tracks.iter().enumerate() {
        let rect = areas.header(row, view.scroll_y);
        if !rect.intersects(header_clip) {
            continue;
        }
        if model.selected_track == Some(row) {
            header_painter.rect_filled(
                rect,
                CornerRadius::same(3),
                theme.accent.gamma_multiply(0.18),
            );
        }
        header_painter.hline(
            rect.x_range(),
            rect.top() + 0.5,
            Stroke::new(1.0, theme.surfaces.divider),
        );
        // The header's background selects the track, drags it to a new place in its section
        // and has the track menu. It goes under the widgets (which are made after it), so they
        // keep their clicks.
        let grip = ui.interact(
            rect.intersect(header_clip),
            ui.id().with(("track-grip", row)),
            Sense::click_and_drag(),
        );
        if grip.clicked() || grip.drag_started() {
            response.actions.push(TrackAction::Select(row));
        }
        let section = areas.sections[Section::of(track.kind).index()].clone();
        let movable = section.len() > 1;
        if movable
            && (grip.dragged() || grip.drag_stopped())
            && let Some(p) = grip.interact_pointer_pos()
        {
            let mut boundaries: Vec<f32> =
                areas.rows[section.clone()].iter().map(|r| r.0).collect();
            boundaries.push(areas.rows[section.end - 1].1);
            let slot = section.start + drop_slot(areas.offset(view.scroll_y, p.y), &boundaries);
            let moves = slot != row && slot != row + 1;
            if grip.dragged() {
                dragging = Some((boundaries[slot - section.start], moves));
            } else if moves {
                response.actions.push(TrackAction::Move { from: row, slot });
            }
        }
        if movable && grip.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            crate::widgets::drag_bubble(ui.ctx(), rect, track_icon(track.kind), &track.name);
        } else if movable && grip.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let grip = grip.on_hover_text(if movable {
            tr("timeline.track.reorder")
        } else {
            tr("timeline.track.header.help")
        });
        grip.context_menu(|ui| track_menu(ui, model, row, &areas, &mut response));
        let (color, kind) = match track.kind {
            TrackKind::Video => (theme.ports.video, tr("timeline.track.kind.video")),
            TrackKind::Audio => (theme.ports.audio, tr("timeline.track.kind.audio")),
        };
        kind_bar(ui, rect, header_clip, color, kind, ("track-kind", row));
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
    // Where a dragged header would land.
    if let Some((y, moves)) = dragging {
        let y = areas.y(y, view.scroll_y);
        header_painter.line_segment(
            [
                pos2(areas.headers.left(), y),
                pos2(areas.headers.right(), y),
            ],
            Stroke::new(
                2.5,
                if moves {
                    theme.accent
                } else {
                    theme.accent.gamma_multiply(0.3)
                },
            ),
        );
    }
    // An empty section names what to drop in it, across its empty lane too, so the hint isn't cut at
    // the header's edge.
    let hint_painter = ui.painter_at(header_clip.union(body_lanes));
    for section in Section::ALL {
        let empty = match section {
            Section::Graphs => model.graph_tracks.is_empty(),
            _ => areas.sections[section.index()].is_empty(),
        };
        if !empty {
            continue;
        }
        let tail = areas.tail(section, view.scroll_y);
        let text = match section {
            Section::Graphs => tr("timeline.section.graphs.empty"),
            Section::Video => tr("timeline.section.video.empty"),
            Section::Audio => tr("timeline.section.audio.empty"),
        };
        hint_painter.text(
            pos2(tail.left() + 6.0, tail.center().y),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(12.0),
            ui.visuals().weak_text_color(),
        );
    }

    response.layout = TimelineLayout {
        ruler: areas.ruler,
        graph_lanes: (0..model.graph_tracks.len())
            .map(|row| areas.graph_lane(row, view.scroll_y))
            .collect(),
        lanes: (0..model.tracks.len())
            .map(|row| areas.lane(row, view.scroll_y))
            .collect(),
        headers: (0..model.tracks.len())
            .map(|row| areas.header(row, view.scroll_y))
            .collect(),
        tails: Section::ALL
            .iter()
            .map(|&s| areas.tail(s, view.scroll_y))
            .collect(),
    };

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

/// The header's right-click menu: reset the track's height, put a new track above or below it
/// (the submenus list the resources of its kind), and remove it.
fn track_menu(
    ui: &mut Ui,
    model: &TimelineModel,
    row: usize,
    areas: &Areas,
    response: &mut TimelineResponse,
) {
    let track = &model.tracks[row];
    if track.height != LANE_HEIGHT && ui.button(tr("timeline.track.reset_height")).clicked() {
        response
            .actions
            .push(TrackAction::SetHeight(row, LANE_HEIGHT));
        ui.close();
    }
    let section = &areas.sections[Section::of(track.kind).index()];
    debug_assert!(section.contains(&row));
    for (label, slot) in [
        (tr("timeline.track.new_above"), row),
        (tr("timeline.track.new_below"), row + 1),
    ] {
        let resources: Vec<_> = model
            .resources
            .iter()
            .filter(|(_, _, kind)| *kind == track.kind)
            .collect();
        ui.add_enabled_ui(!resources.is_empty(), |ui| {
            ui.menu_button(label, |ui| {
                for (id, name, _) in &resources {
                    if ui.button(name).clicked() {
                        response.actions.push(TrackAction::NewTrack {
                            slot,
                            resource: *id,
                        });
                        ui.close();
                    }
                }
            })
            .response
            .on_disabled_hover_text(tr("timeline.track.new.none"))
            .on_hover_text(tr("timeline.track.new.help"));
        });
    }
    if ui.button(tr("timeline.track.remove")).clicked() {
        response.actions.push(TrackAction::Remove(row));
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

/// The ruler in tempo mode, in two tiers: bar numbers bold along the top and beats small under
/// them, labelled where they have room, with a "bar / beat" key in the `corner` over the
/// headers. Every other bar is shaded down through the `lanes`, and beat lines are fainter than
/// bar lines.
fn tempo_ruler(
    ui: &Ui,
    grid: &RulerGrid,
    ticks: std::ops::RangeInclusive<i64>,
    x: impl Fn(f64) -> f32,
    (corner, ruler): (Rect, Rect),
    lanes: Rect,
) {
    let theme = Theme::of(ui.ctx());
    let Some((minor_beats, per_bar)) = grid.bars else {
        return;
    };
    let painter = ui.painter_at(ruler.union(lanes));
    let (first, last) = (*ticks.start(), *ticks.end());

    let bar_secs = grid.minor / minor_beats * f64::from(per_bar);
    let bar_px = x(grid.origin + bar_secs) - x(grid.origin);
    if bar_px >= MIN_SHADED_BAR {
        let shade = theme.surfaces.text.gamma_multiply(0.022);
        let bar_of = |t: f64| ((t - grid.origin) / bar_secs).floor() as i64;
        for bar in bar_of(grid.time(first))..=bar_of(grid.time(last)) {
            if bar.rem_euclid(2) == 1 {
                let start = grid.origin + bar as f64 * bar_secs;
                let band = Rect::from_x_y_ranges(
                    x(start)..=x(start + bar_secs),
                    lanes.top()..=lanes.bottom(),
                );
                painter.rect_filled(band, CornerRadius::ZERO, shade);
            }
        }
    }

    let minor_px = x(grid.time(1)) - x(grid.time(0));
    for k in ticks {
        let t = grid.time(k);
        let tx = x(t);
        let is_major = k.rem_euclid(i64::from(grid.divisions)) == 0;
        let labelled = t >= -1e-9;
        if grid.on_bar(k) {
            painter.line_segment(
                [
                    pos2(tx, ruler.top()),
                    pos2(tx, ruler.top() + BEAT_TIER - 1.0),
                ],
                Stroke::new(1.0, theme.tick),
            );
            painter.line_segment(
                [pos2(tx, lanes.top()), pos2(tx, lanes.bottom())],
                Stroke::new(1.0, theme.tick.gamma_multiply(0.3)),
            );
            if is_major && labelled {
                painter.text(
                    pos2(tx + 5.0, ruler.top() + 3.0),
                    Align2::LEFT_TOP,
                    grid.label(k),
                    crate::theme::semibold(12.0),
                    theme.surfaces.text,
                );
            }
        } else {
            painter.line_segment(
                [pos2(tx, ruler.top() + BEAT_TIER), pos2(tx, ruler.bottom())],
                Stroke::new(1.0, theme.tick.gamma_multiply(0.7)),
            );
            painter.line_segment(
                [pos2(tx, lanes.top()), pos2(tx, lanes.bottom())],
                Stroke::new(1.0, theme.tick.gamma_multiply(0.1)),
            );
            if (is_major || minor_px >= MIN_BEAT_LABEL_SPACING) && labelled {
                painter.text(
                    pos2(tx + 3.0, ruler.top() + BEAT_TIER),
                    Align2::LEFT_TOP,
                    grid.label(k),
                    FontId::monospace(9.5),
                    theme.tick_label,
                );
            }
        }
    }

    let key = ui.painter_at(corner);
    for (text, y, color) in [
        (tr("timeline.ruler.bar"), 4.0, theme.tick_label),
        (
            tr("timeline.ruler.beat"),
            BEAT_TIER,
            theme.surfaces.text_faint,
        ),
    ] {
        key.text(
            pos2(corner.left() + 10.0, corner.top() + y),
            Align2::LEFT_TOP,
            text.to_uppercase(),
            crate::theme::semibold(9.5),
            color,
        );
    }
}

/// The first beat on the tempo ruler: an amber marker at `x`, between the bar and beat tiers,
/// that names its time on hover.
fn first_beat_marker(ui: &mut Ui, x: f32, seconds: f64, ruler: Rect) {
    if !ruler.x_range().contains(x) {
        return;
    }
    let theme = Theme::of(ui.ctx());
    let top = ruler.top() + BEAT_TIER - 6.0;
    ui.painter_at(ruler).add(egui::Shape::convex_polygon(
        vec![pos2(x - 5.0, top), pos2(x + 5.0, top), pos2(x, top + 7.0)],
        theme.beat,
        Stroke::NONE,
    ));
    let zone = Rect::from_center_size(pos2(x, top + 3.5), vec2(12.0, 10.0));
    let response = ui.interact(zone, ui.id().with("first-beat"), Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, tr("timeline.first_beat"))
    });
    response.on_hover_text(tr_args(
        "timeline.first_beat.help",
        &[("time", &timecode(seconds))],
    ));
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
        painter.rect_filled(lane, CornerRadius::same(3), theme.lane_bg);
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
        if track.empty && !track.missing {
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
            TrackKind::Video if track.thumbnails.is_some() => {
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
        let name = match item.group {
            Some(_) => tr_args("timeline.item.grouped", &[("name", &track.name)]),
            None => track.name.clone(),
        };
        if item_bar(
            ui,
            painter,
            self.clip,
            theme,
            bar,
            (visible.left(), &name, item.muted),
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
        grab.context_menu(|ui| item_menu(ui, model, item, response));
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        grab.on_hover_text(if item.group.is_none() {
            tr("timeline.item.drag")
        } else {
            tr("timeline.item.drag_grouped")
        });
    }
}

/// The handles along an item's two edges, which trim it: `trim` gets the edge and the time it
/// was dragged to (snapped). `ids` are the handles' id parts and the id the drag is remembered
/// under; `targets` lists where a drag snaps to, asked when a drag starts. The handles are made
/// after the item's bar, so they win where they overlap it, and before its mute button, so that
/// keeps its clicks.
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
/// right end. Returns whether the mute button was clicked.
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

/// Where a drag of anything can snap when nothing is left out: the timeline's start, the
/// playhead and the edges of every item.
fn all_snap_targets(model: &TimelineModel) -> Vec<f64> {
    let mut targets = graph_snap_targets(model, None);
    for track in &model.tracks {
        for item in &track.items {
            if let Some((start, end)) = track.item_span(item) {
                targets.extend([start, end]);
            }
        }
    }
    targets
}

/// Where a drag can snap among the graph items: the timeline's start, the playhead and the edges
/// of every graph item but `skip` (graph track row, item).
fn graph_snap_targets(model: &TimelineModel, skip: Option<(usize, usize)>) -> Vec<f64> {
    let mut targets = vec![0.0, model.playhead as f64 / model.frame_rate];
    for (row, track) in model.graph_tracks.iter().enumerate() {
        for (k, item) in track.items.iter().enumerate() {
            if skip != Some((row, k)) {
                targets.extend([item.position, item.position + item.length]);
            }
        }
    }
    targets
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

/// Height of the ghost row drawn where a dropped resource or graph would make a new track.
const GHOST_ROW_HEIGHT: f32 = 26.0;

/// Where the drop ghosts go (screen space, before clipping), each with whether it is a ghost
/// row for a new track, its name and its length: the lane of each track that takes a resource
/// (or a ghost row at the bottom of its section, stacked when several new tracks are made), or
/// the graph track a graph lands on (or a ghost row in the Graphs section).
fn ghost_bands<'a>(
    areas: &Areas,
    view: &TimelineView,
    ghost: &'a DropGhost,
    response: &TimelineResponse,
) -> Vec<(Rect, bool, &'a str, Option<f64>)> {
    let ghost_row = |section: Section, n: usize| {
        // New graph tracks go on top; new video and audio tracks at the ghost line when the
        // pointer is on one in their section, else at the bottom of theirs.
        let first = match section {
            Section::Graphs => areas.y(0.0, view.scroll_y),
            _ => response
                .drop_slot
                .filter(|&slot| {
                    let range = &areas.sections[section.index()];
                    (range.start..=range.end).contains(&slot)
                })
                .and_then(|slot| areas.slot_offset(slot))
                .map_or_else(
                    || areas.tail(section, view.scroll_y).top(),
                    |offset| areas.y(offset, view.scroll_y),
                ),
        };
        let top = first + n as f32 * GHOST_ROW_HEIGHT;
        Rect::from_min_max(
            pos2(areas.lanes.left(), top + 2.0),
            pos2(areas.lanes.right(), top + GHOST_ROW_HEIGHT - 2.0),
        )
    };
    match ghost {
        DropGhost::Media(items) => {
            let mut new = [0usize; 3];
            items
                .iter()
                .map(|item| {
                    let (band, new_row) = match item.row {
                        GhostRow::Track(row) => (areas.lane(row, view.scroll_y), false),
                        GhostRow::New(kind) => {
                            let section = Section::of(kind);
                            let n = &mut new[section.index()];
                            *n += 1;
                            (ghost_row(section, *n - 1), true)
                        }
                    };
                    (band, new_row, item.name.as_str(), item.length)
                })
                .collect()
        }
        DropGhost::Graph { name, length } => {
            let (band, new_row) = match response.graph_drop {
                Some(GraphDrop::Track(row)) => (areas.graph_lane(row, view.scroll_y), false),
                Some(GraphDrop::NewTrackAt(slot)) => {
                    let top = areas.y(areas.graph_slot_offset(slot), view.scroll_y);
                    (
                        Rect::from_min_max(
                            pos2(areas.lanes.left(), top + 2.0),
                            pos2(areas.lanes.right(), top + GHOST_ROW_HEIGHT - 2.0),
                        ),
                        true,
                    )
                }
                _ => (ghost_row(Section::Graphs, 0), true),
            };
            vec![(band, new_row, name.as_str(), Some(*length))]
        }
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
/// those grouped with it).
fn snap_targets(model: &TimelineModel, row: usize, k: usize, with_selection: bool) -> Vec<f64> {
    let group = model.tracks[row].items.get(k).and_then(|i| i.group);
    let mut targets = vec![0.0, model.playhead as f64 / model.frame_rate];
    for (r, track) in model.tracks.iter().enumerate() {
        for (j, item) in track.items.iter().enumerate() {
            let Some((start, end)) = track.item_span(item) else {
                continue;
            };
            let moving = (r == row && j == k)
                || (with_selection && track.selected_items.contains(&j))
                || (group.is_some() && item.group == group);
            if !moving {
                targets.extend([start, end]);
            }
        }
    }
    targets
}

/// The item keys, while the pointer is over the timeline: S splits at the playhead, Delete and
/// Backspace remove the selected items, Ctrl+A selects every item, Ctrl+G groups them and Ctrl+Shift+G ungroups them, and
/// the clipboard events copy, cut and paste them. The
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
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::A)) {
        response.actions.push(TrackAction::SelectAll);
    }
    let (ungroup, group) = ui.input_mut(|i| {
        let shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        (
            i.consume_key(shift, Key::G),
            i.consume_key(egui::Modifiers::COMMAND, Key::G),
        )
    });
    if group {
        response.actions.push(TrackAction::GroupItems);
    }
    if ungroup {
        response.actions.push(TrackAction::UngroupItems);
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

/// An item's right-click menu: the item keys, for the selected items. Group needs two items
/// selected; Ungroup needs a grouped one.
fn item_menu(ui: &mut Ui, model: &TimelineModel, item: &Item, response: &mut TimelineResponse) {
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
    ui.separator();
    let selected = || {
        model
            .tracks
            .iter()
            .flat_map(|t| t.selected_items.iter().filter_map(|&j| t.items.get(j)))
    };
    let can_ungroup = item.group.is_some() || selected().any(|i| i.group.is_some());
    let group_entries = [
        (
            tr("timeline.item.group"),
            "Ctrl+G",
            selected().count() >= 2,
            TrackAction::GroupItems,
        ),
        (
            tr("timeline.item.ungroup"),
            "Ctrl+Shift+G",
            can_ungroup,
            TrackAction::UngroupItems,
        ),
    ];
    for (label, keys, enabled, action) in group_entries {
        if ui
            .add_enabled(enabled, egui::Button::new(label).shortcut_text(keys))
            .on_hover_text(tr("timeline.item.group.help"))
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

/// The glyph a track is marked with while it is dragged.
fn track_icon(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Video => "▣",
        TrackKind::Audio => "♪",
    }
}

/// The glyph graph tracks are marked with.
const GRAPH_ICON: &str = "▤";

/// The widgets of a track's header: its name with the M and S buttons beside it, and under the
/// name the video's size and frame rate, or an audio track's volume and, when the project has
/// several buses (or the track's is gone), its bus. Removing the track is in the header's menu.
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
    let theme = Theme::of(ui.ctx());
    let column = vec2(name_width(ui, 2), NAME_ROWS_HEIGHT);
    ui.allocate_ui_with_layout(column, egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(column.x);
        ui.spacing_mut().item_spacing.y = 3.0;
        let edit = header_name(ui, ui.id().with(("track-name", row)), &track.name);
        let edit_response = edit.response.on_hover_text(tr("timeline.track.name.help"));
        if edit_response.gained_focus() {
            response.actions.push(TrackAction::Select(row));
        }
        if let Some(name) = edit.committed {
            response.actions.push(TrackAction::Rename(row, name));
        }
        ui.horizontal(|ui| match track.kind {
            _ if track.empty => detail(ui, tr("timeline.track.empty")),
            TrackKind::Video => {
                if let Some(details) = &track.details {
                    detail(ui, details);
                }
            }
            TrackKind::Audio => audio_controls(ui, track, buses, row, response),
        });
    });
    ui.add_space(6.0 - ui.spacing().item_spacing.x);
    let mute_help = if track.muted {
        tr("timeline.track.unmute")
    } else {
        tr("timeline.track.mute")
    };
    if track_toggle(ui, track.muted, "M", theme.text_dim)
        .on_hover_text(mute_help)
        .clicked()
    {
        response.actions.push(TrackAction::ToggleMute(row));
    }
    if track_toggle(ui, track.solo, "S", theme.solo)
        .on_hover_text(tr("timeline.track.solo"))
        .clicked()
    {
        response.actions.push(TrackAction::ToggleSolo(row));
    }
}

/// A header's small, faint line under the name.
fn detail(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).weak().small()).truncate());
}

/// The widgets of a graph track's header: its name and the M button. Removing the track is in
/// the header's menu.
fn graph_track_header(
    ui: &mut Ui,
    track: &GraphTrackView,
    row: usize,
    response: &mut TimelineResponse,
) {
    ui.style_mut().interaction.selectable_labels = false;
    let theme = Theme::of(ui.ctx());
    let column = vec2(name_width(ui, 1), NAME_HEIGHT);
    ui.allocate_ui_with_layout(
        column,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            let edit = header_name(ui, ui.id().with(("graph-track-name", row)), &track.name);
            edit.response
                .on_hover_text(tr("timeline.graph_track.name.help"));
            if let Some(name) = edit.committed {
                response.graph_actions.push(GraphAction::Rename(row, name));
            }
        },
    );
    ui.add_space(6.0 - ui.spacing().item_spacing.x);
    let mute_help = if track.muted {
        tr("timeline.graph_track.unmute")
    } else {
        tr("timeline.graph_track.mute")
    };
    if track_toggle(ui, track.muted, "M", theme.text_dim)
        .on_hover_text(mute_help)
        .clicked()
    {
        response.graph_actions.push(GraphAction::ToggleMute(row));
    }
}

/// An audio track's volume bar and level, and its bus when there is a choice to make.
fn audio_controls(
    ui: &mut Ui,
    track: &TrackView,
    buses: &[String],
    row: usize,
    response: &mut TimelineResponse,
) {
    const READOUT: f32 = 32.0;
    const BUS: f32 = 64.0;
    ui.spacing_mut().interact_size.y = 20.0;
    let theme = Theme::of(ui.ctx());
    let with_bus = buses.len() > 1 || !buses.contains(&track.bus);
    let gaps = ui.spacing().item_spacing.x * if with_bus { 2.0 } else { 1.0 };
    let used = READOUT + gaps + if with_bus { BUS + 20.0 } else { 0.0 };
    let width = (ui.available_width() - used).max(24.0);
    if let Some(volume) = volume_bar(ui, track.volume, width) {
        response.actions.push(TrackAction::SetVolume(row, volume));
    }
    let level = format!(
        "{:.0}{}",
        f64::from(track.volume) * 100.0,
        tr("timeline.track.volume.suffix")
    );
    ui.add_sized(
        vec2(READOUT, 14.0),
        egui::Label::new(
            egui::RichText::new(level)
                .monospace()
                .size(crate::theme::SMALL_SIZE)
                .color(theme.text_dim),
        ),
    );
    if with_bus {
        let mut bus = track.bus.clone();
        egui::ComboBox::from_id_salt(("track-bus", row))
            .width(BUS)
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

/// One graph track's lane: its graph items, each with a header bar along its top.
struct GraphLane<'a> {
    track: &'a GraphTrackView,
    row: usize,
    rect: Rect,
    /// The visible part of the lanes.
    clip: Rect,
    /// A graph is being dragged over this lane.
    drop_target: bool,
}

impl GraphLane<'_> {
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
        for (k, item) in self.track.items.iter().enumerate() {
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
        let (track, row) = (self.track, self.row);
        let bar_of = |block: Rect| {
            Rect::from_min_max(block.min, pos2(block.right(), block.top() + ITEM_BAR))
        };
        let grab = ui.interact(
            bar_of(block).intersect(self.clip),
            ui.id().with(("graph-item-bar", row, k)),
            Sense::click_and_drag(),
        );
        let hovered = grab.hovered() || grab.dragged();
        let selected = track.selected_items.contains(&k);
        let ctx = ui.ctx();
        let drag_id = ui.id().with(("graph-item-drag", row, k));
        let landing_id = drag_id.with("landing");
        let select = GraphAction::SelectItem {
            track: row,
            item: k,
        };
        edge_handles(
            ui,
            model,
            view,
            self.clip,
            block,
            Some((item.position, item.position + item.length)),
            (("graph-item-edge", row, k), drag_id),
            &|| graph_snap_targets(model, Some((row, k))),
            tr("timeline.graph_item.edge"),
            |edge, to| {
                response.graph_actions.push(GraphAction::TrimItem {
                    track: row,
                    item: k,
                    edge,
                    to,
                });
            },
        );
        if grab.drag_started_by(PointerButton::Primary) {
            if !selected {
                response.graph_actions.push(select.clone());
            }
            let drag = ItemDrag {
                edge: None,
                origin: item.position,
                length: item.length,
                targets: graph_snap_targets(model, Some((row, k))),
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
                response.graph_actions.push(GraphAction::MoveItem {
                    track: row,
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
        if item.muted || track.muted {
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
            ui.id().with(("graph-item-mute", row, k)),
        ) {
            response.graph_actions.push(GraphAction::ToggleItemMute {
                track: row,
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
            response.graph_actions.push(select);
        }
        if grab.double_clicked() {
            response.graph_actions.push(GraphAction::OpenItem {
                track: row,
                item: k,
            });
        }
        grab.context_menu(|ui| graph_item_menu(ui, model, row, k, response));
        if hovered {
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
        }
        grab.on_hover_text(tr("timeline.graph_item.drag"));
    }
}

/// A graph item's right-click menu: split at the playhead and delete.
fn graph_item_menu(
    ui: &mut Ui,
    model: &TimelineModel,
    track: usize,
    item: usize,
    response: &mut TimelineResponse,
) {
    let at = model.playhead as f64 / model.frame_rate;
    if ui
        .add(egui::Button::new(tr("timeline.item.split")).shortcut_text("S"))
        .clicked()
    {
        response
            .graph_actions
            .push(GraphAction::SplitItem { track, item, at });
        ui.close();
    }
    if ui
        .add(egui::Button::new(tr("timeline.item.delete")).shortcut_text("Del"))
        .clicked()
    {
        response
            .graph_actions
            .push(GraphAction::DeleteItem { track, item });
        ui.close();
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
    let inner = Rect::from_min_max(top.min + vec2(KIND_INSET, 3.0), top.max - vec2(8.0, 3.0));
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(clip.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing.x = 4.0;
    contents(&mut child);
}

/// The bar of a track's kind colour at the left of its header, named `label` for screen readers
/// (and tests). It only senses hovering, so a drag that starts on it reaches the header below.
fn kind_bar(
    ui: &mut Ui,
    rect: Rect,
    clip: Rect,
    color: Color32,
    label: &str,
    id: impl std::hash::Hash + std::fmt::Debug,
) {
    let top = rect.height().min(LANE_HEIGHT - 4.0);
    let height = (top * 0.6).round().clamp(16.0, 34.0);
    let bar = Rect::from_center_size(
        pos2(rect.left() + 9.5, rect.top() + top / 2.0),
        vec2(3.0, height),
    );
    ui.painter_at(clip)
        .rect_filled(bar, CornerRadius::same(2), color);
    let zone = bar.expand2(vec2(3.0, 0.0)).intersect(clip);
    if zone.is_positive() {
        ui.interact(zone, ui.id().with(id), Sense::hover())
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
    }
}

/// A header's name: plain text that is edited in place, outlined while hovered or edited.
fn header_name(ui: &mut Ui, id: egui::Id, name: &str) -> crate::name_edit::NameEdit {
    let width = ui.available_width();
    let edit = name_edit(ui, id, name, |e| {
        e.frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(crate::theme::medium(NAME_SIZE))
            .desired_width(width)
    });
    let theme = Theme::of(ui.ctx());
    let outline = if edit.response.has_focus() {
        Some(theme.accent)
    } else if edit.response.hovered() {
        Some(theme.surfaces.border)
    } else {
        None
    };
    if let Some(color) = outline {
        ui.painter().rect_stroke(
            edit.response.rect.expand2(vec2(3.0, 1.0)),
            3,
            Stroke::new(1.0, color),
            egui::StrokeKind::Outside,
        );
    }
    edit
}

/// A header's M or S button: outlined, and filled with `fill` while on.
fn track_toggle(ui: &mut Ui, on: bool, letter: &str, fill: Color32) -> egui::Response {
    let ink_on = Theme::of(ui.ctx()).surfaces.ground;
    crate::widgets::toggle_box(ui, on, fill, letter, |painter, rect, ink, on| {
        if on {
            painter.rect_filled(rect, 4, fill);
        }
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            letter,
            crate::theme::semibold(11.0),
            if on { ink_on } else { ink },
        );
    })
}

/// The width left for a header's name once `toggles` M and S buttons are beside it.
fn name_width(ui: &Ui, toggles: usize) -> f32 {
    let buttons = toggles as f32 * (TOGGLE_WIDTH + ui.spacing().item_spacing.x);
    (ui.available_width() - buttons - 6.0).max(0.0)
}

/// An audio track's volume as a slim bar: drag or click along it to set the level, double-click
/// for full. Returns the new volume when it changes.
fn volume_bar(ui: &mut Ui, volume: f32, width: f32) -> Option<f32> {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 14.0), Sense::click_and_drag());
    let enabled = ui.is_enabled();
    response.widget_info(|| {
        egui::WidgetInfo::slider(enabled, f64::from(volume), tr("timeline.track.volume"))
    });
    let theme = Theme::of(ui.ctx());
    let groove = Rect::from_center_size(rect.center(), vec2(rect.width(), 4.0));
    let level = groove.with_max_x(groove.left() + groove.width() * volume.clamp(0.0, 1.0));
    let painter = ui.painter();
    painter.rect_filled(groove, 2, theme.surfaces.raised_active);
    painter.rect_filled(level, 2, theme.ports.audio);
    if response.hovered() || response.dragged() {
        painter.circle_filled(level.right_center(), 4.0, theme.surfaces.text);
    }
    let response = response.on_hover_text(tr("timeline.track.volume.bar.help"));
    if response.double_clicked() {
        return (volume != 1.0).then_some(1.0);
    }
    if (response.dragged() || response.clicked())
        && let Some(p) = response.interact_pointer_pos()
    {
        let at = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let at = (at * 100.0).round() / 100.0;
        return (at != volume).then_some(at);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn sections_stack_with_their_add_rows_and_dividers() {
        let tall = TrackView {
            height: 100.0,
            ..TrackView::new("b", TrackKind::Audio)
        };
        let tracks = [TrackView::new("a", TrackKind::Video), tall];
        let areas = Areas::new(
            Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 400.0)),
            RULER_HEIGHT,
            1,
            &tracks,
        );
        let body = RULER_HEIGHT;
        // The graph track, its "+" row and the gap, then the video track.
        let video = GRAPH_LANE_HEIGHT + TAIL_HEIGHT + SECTION_GAP;
        assert_eq!(areas.graph_row_at(0.0, body + 10.0), Some(0));
        assert_eq!(areas.row_at(0.0, body + 10.0), None);
        assert_eq!(areas.row_at(0.0, body + video + 10.0), Some(0));
        // The audio track comes after the video's "+" row and the second gap.
        let audio = video + LANE_HEIGHT + TAIL_HEIGHT + SECTION_GAP;
        assert_eq!(areas.lane(1, 0.0).top(), body + audio + 2.0);
        assert_eq!(areas.lane(1, 0.0).height(), 96.0);
        assert_eq!(areas.row_at(0.0, body + audio - 5.0), None, "in the gap");
        assert_eq!(areas.row_at(0.0, body + audio + 90.0), Some(1));
        assert_eq!(areas.row_at(20.0, body + audio - 10.0), Some(1));
        assert_eq!(areas.sections, [0..0, 0..1, 1..2]);
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
            graph_tracks: Vec::new(),
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
            resources: Vec::new(),
            drop_ghost: None,
        };
        let grid = RulerGrid::new(&model(TimelineMode::Tempo), 360.0);
        // Ticks every quarter beat (0.125 s), starting at the offset.
        assert_eq!(grid.time(0), 0.25);
        assert!((grid.time(4) - 0.75).abs() < 1e-12);
        assert_eq!(grid.label(0), "1");
        assert_eq!(grid.label(4), "1.2");
        assert_eq!(grid.label(16), "2");
        // Bar ticks are told from beat ticks, either side of the first beat.
        assert!(grid.on_bar(0) && grid.on_bar(16) && grid.on_bar(-16));
        assert!(!grid.on_bar(4));
        // Snapping lands on ticks; in time mode, on frames.
        assert!((grid.snap(0.77, 30.0) - 0.75).abs() < 1e-12);
        let time = RulerGrid::new(&model(TimelineMode::Time), 360.0);
        assert!((time.snap(0.77, 30.0) - 23.0 / 30.0).abs() < 1e-12);
        assert!(!time.on_bar(0));
    }

    #[test]
    fn positions_read_as_bars_and_beats() {
        // 120 bpm in 4/4 with the first beat at 0.25 s: a beat is half a second.
        let tempo = Tempo {
            offset_secs: 0.25,
            ..Tempo::default()
        };
        assert_eq!(bar_beat(&tempo, 0.25), "1.1");
        assert_eq!(bar_beat(&tempo, 2.25), "2.1");
        assert_eq!(bar_beat(&tempo, 3.25), "2.3");
        assert_eq!(bar_beat(&tempo, 3.375), "2.3.2");
        assert_eq!(bar_beat(&tempo, 0.0), "0.4.3", "before the first beat");
        assert_eq!(bar_beat_tick(&tempo, 0.25), "1.1.1");
        assert_eq!(bar_beat_tick(&tempo, 3.74), "2.3.4");
        assert_eq!(beats_at(&tempo, 1.25), 2.0);
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
        assert_eq!(short_timecode(3.0), "0:03");
        assert_eq!(short_timecode(125.0), "2:05");
        assert_eq!(short_timecode(1.25), "0:01.25");
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
