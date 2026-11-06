//! The project's timeline: its timebase, and tracks of items placed on it.
//!
//! The project is the clock, like a Premiere sequence: it has its own resolution and frame rate,
//! and every track is conformed to that grid on its way into a graph. A track holds items of
//! resources of its kind (graphs, video or audio); an item places a stretch of its resource,
//! between its in and out points, at a position on the timeline, played at a rate. Positions and
//! in/out points are times in seconds, so they survive a change of frame rate. Items on a track
//! never overlap, and gaps between them read zeros.

use rastersong_graph::nodes::DEFAULT_BUS;
use rastersong_media::Rational;
use serde::{Deserialize, Serialize};

use crate::project::ResourceId;

/// Seconds over which audio fades in at the start of each item and out at its end, so cuts
/// don't click. Fixed for now; never more than half the item.
pub const EDGE_FADE: f64 = 0.005;

/// The project's picture size and frame rate. One block of every graph is one project frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timebase {
    pub width: u32,
    pub height: u32,
    /// Frames a second, saved as `"30000/1001"`.
    #[serde(with = "rational")]
    pub frame_rate: Rational,
}

impl Timebase {
    /// The timebase of a project with no video to take one from.
    pub const DEFAULT: Self = Self {
        width: 1920,
        height: 1080,
        frame_rate: Rational::new(30, 1),
    };

    pub fn fps(&self) -> f64 {
        self.frame_rate.as_f64()
    }

    /// Whether the timebase can be rendered: a size and a positive frame rate.
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.frame_rate.num > 0
            && self.frame_rate.den > 0
            && self.fps().is_finite()
    }

    /// The frames `seconds` of timeline cover: every frame that starts before the end.
    pub fn frames_in(&self, seconds: f64) -> usize {
        (seconds * self.fps() - 1e-6).ceil().max(0.0) as usize
    }
}

/// What a resource is, and so what a track of its resources holds. Tracks are listed in this
/// order, a section of each: the graphs on top, then the video, then the audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    /// A graph.
    Graph,
    /// One video stream of a file.
    Video,
    /// One audio stream of a file, any channel count.
    Audio,
}

impl TrackKind {
    /// Whether its resources have a length of their own (a file's), which items can't play
    /// past. A graph runs for as long as its item lasts.
    pub fn has_duration(self) -> bool {
        self.reads_file()
    }

    /// Whether its items can play at another rate than their resource's own.
    pub fn stretches(self) -> bool {
        self.reads_file()
    }

    /// Whether its resources read a linked file, which can go missing and be relocated.
    pub fn reads_file(self) -> bool {
        matches!(self, Self::Video | Self::Audio)
    }

    /// Whether its tracks have a level and an output bus in the mix.
    pub fn has_level(self) -> bool {
        self == Self::Audio
    }

    /// Whether its items can warm up as if they had been playing before their left edge.
    pub fn pre_rolls(self) -> bool {
        self == Self::Graph
    }

    /// Whether its items open their resource in the editor: a graph's, in the node graph.
    pub fn opens(self) -> bool {
        self == Self::Graph
    }

    /// Whether a resource of this kind can be duplicated: a graph, which the project holds, can;
    /// a stream of a file is the same stream however many times it is listed.
    pub fn duplicates(self) -> bool {
        self == Self::Graph
    }

    /// How a track of this kind goes over what is below it where one of its items plays.
    pub fn stacks(self) -> Stack {
        match self {
            Self::Graph => Stack::PictureAndSound,
            Self::Video => Stack::Picture,
            Self::Audio => Stack::Sum,
        }
    }
}

/// How a track's output goes over what is below it: the one rule the video mix and the graph
/// tracks share. Where a track plays nothing, what is below shows through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stack {
    /// Its picture covers the picture below, where it has one; it has no sound (video).
    Picture,
    /// Its picture and its sound replace what is below (graphs).
    PictureAndSound,
    /// Its sound is added to what is below, at its level; it has no picture (audio).
    Sum,
}

/// A stretch of a resource placed on a track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    /// What it plays: a resource of its track's kind.
    pub resource: ResourceId,
    /// Seconds into the project where the item starts.
    #[serde(default)]
    pub position: f64,
    /// Seconds into the resource where the item starts: its in point.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub start: f64,
    /// Seconds into the resource where the item ends: its out point. `None` plays to the end of
    /// the resource, whatever its length; a graph item always has one, as a graph has no end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
    /// Seconds of resource played per second of timeline: 2 plays twice as fast. Video holds or
    /// skips frames; audio is resampled, so its pitch moves like tape. Always 1 for kinds that
    /// don't [stretch](TrackKind::stretches).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub rate: f64,
    /// A muted item reads as a gap.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
    /// Items with the same group move, trim, split and delete together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<u32>,
    /// For kinds that [pre-roll](TrackKind::pre_rolls): whether the graph warms up as if it had
    /// been running before the item, so trimming its left edge never changes what follows.
    /// Off, it starts cold at the edge.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub pre_roll: bool,
}

/// A label colour for a resource, picked from a fixed set so every theme can draw it legibly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelColor {
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Blue,
    Violet,
    Pink,
}

impl LabelColor {
    /// Every colour, in the order menus list them.
    pub const ALL: [Self; 8] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Teal,
        Self::Blue,
        Self::Violet,
        Self::Pink,
    ];
}

/// What an input node of a graph reads. A Video Input or Audio Input names it in its `source`
/// setting ([`SOURCE_PARAM`](rastersong_graph::nodes::SOURCE_PARAM)), a Named Input in its
/// `track` and `name`. Sources are matched by kind: a node reading pictures gets the source's
/// picture, one reading sound its sound; a Named Input gets the signal as it was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The main output of the next active graph below, or the timeline's mix with none.
    Below,
    /// The video mix (the top video track with an item) or the audio mix.
    Mix,
    /// One track on its own (its stem), before its volume and mute.
    Track(String),
    /// The main output of the graph track named, which must be below the item's: where nothing
    /// plays on it, what is below it.
    Graph(String),
    /// The Named Output `name` of the graph track `track`, which must be below the item's: zeros
    /// where none of its items sends one.
    Named { track: String, name: String },
    /// Zeros.
    Nothing,
}

impl Source {
    /// The source a Video Input or Audio Input with `source` setting `source` reads.
    pub fn from_setting(source: &str) -> Self {
        if source == rastersong_graph::nodes::BELOW {
            Self::Below
        } else if source == "mix" {
            Self::Mix
        } else if let Some(track) = source.strip_prefix("track:") {
            Self::Track(track.to_owned())
        } else if let Some(track) = source.strip_prefix("graph:") {
            Self::Graph(track.to_owned())
        } else {
            Self::Nothing
        }
    }

    /// The `source` setting of a Video Input or Audio Input reading it: the reverse of
    /// [`Self::from_setting`]. A Named Output reads as nothing there.
    pub fn setting(&self) -> String {
        match self {
            Self::Below => rastersong_graph::nodes::BELOW.to_owned(),
            Self::Mix => "mix".into(),
            Self::Track(track) => format!("track:{track}"),
            Self::Graph(track) => format!("graph:{track}"),
            Self::Named { .. } | Self::Nothing => String::new(),
        }
    }

    /// The graph track it reads, if any.
    pub fn graph_track(&self) -> Option<&str> {
        match self {
            Self::Graph(track) | Self::Named { track, .. } => Some(track),
            _ => None,
        }
    }
}

fn yes() -> bool {
    true
}

fn is_yes(v: &bool) -> bool {
    *v
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

fn one() -> f64 {
    1.0
}

fn is_one(v: &f64) -> bool {
    *v == 1.0
}

impl Item {
    /// The whole of resource `resource` at `position`, at its own speed.
    pub fn whole(resource: ResourceId, position: f64) -> Self {
        Self {
            resource,
            position,
            start: 0.0,
            end: None,
            rate: 1.0,
            muted: false,
            group: None,
            pre_roll: true,
        }
    }

    /// Graph `graph` from `position` for `length` seconds, from the start of its own time.
    pub fn graph(graph: ResourceId, position: f64, length: f64) -> Self {
        Self {
            end: Some(length),
            ..Self::whole(graph, position)
        }
    }

    /// The item with nonsense (non-finite or negative times, a rate that isn't positive)
    /// replaced by what it was most likely meant to be.
    pub fn sanitized(self) -> Self {
        let finite = |v: f64, default: f64| if v.is_finite() { v } else { default };
        let start = finite(self.start, 0.0).max(0.0);
        Self {
            position: finite(self.position, 0.0).max(0.0),
            start,
            end: self.end.filter(|e| e.is_finite()).map(|e| e.max(start)),
            rate: Some(self.rate)
                .filter(|r| r.is_finite() && *r > 0.0)
                .unwrap_or(1.0),
            ..self
        }
    }

    /// The out point, for a resource `duration` seconds long.
    pub fn out(&self, duration: f64) -> f64 {
        self.end.unwrap_or(duration).max(self.start)
    }

    /// How long the item lasts on the timeline, in seconds.
    pub fn length(&self, duration: f64) -> f64 {
        (self.out(duration) - self.start) / self.rate
    }

    /// Where the item ends on the timeline, in seconds.
    pub fn timeline_end(&self, duration: f64) -> f64 {
        self.position + self.length(duration)
    }

    /// The resource time played at timeline time `t`, which may lie outside the item.
    pub fn source_time(&self, t: f64) -> f64 {
        self.start + (t - self.position) * self.rate
    }

    /// How long the item's audio takes to fade in at its start, and out at its end, in seconds:
    /// [`EDGE_FADE`], or half the item when it is shorter than two fades.
    pub fn fade(&self, duration: f64) -> f64 {
        EDGE_FADE.min(self.length(duration) / 2.0)
    }

    /// The gain of the item's audio at timeline time `t`: 0 outside the item or when muted,
    /// rising linearly to 1 over the fade at its start and falling back to 0 over the fade at
    /// its end. Items on a track never overlap, so a track's items are summed.
    pub fn gain_at(&self, t: f64, duration: f64) -> f64 {
        let end = self.timeline_end(duration);
        if self.muted || t < self.position || t >= end {
            return 0.0;
        }
        let fade = self.fade(duration);
        if fade <= 0.0 {
            return 1.0;
        }
        ((t - self.position).min(end - t) / fade).min(1.0)
    }

    /// The resource time played at timeline time `t`, if the item plays then.
    pub fn plays_at(&self, t: f64, duration: f64) -> Option<f64> {
        (!self.muted && t >= self.position && t < self.timeline_end(duration))
            .then(|| self.source_time(t))
    }
}

/// The item of `items` that plays at timeline time `t`, and the resource time it plays. Each
/// item's resource lasts `duration(item)` seconds; an item whose length isn't known (`None`)
/// plays nothing. Items on a track never overlap; if they do (a file written by hand), the last
/// one listed wins.
pub fn item_at(
    items: &[Item],
    t: f64,
    duration: impl Fn(&Item) -> Option<f64>,
) -> Option<(&Item, f64)> {
    items
        .iter()
        .rev()
        .find_map(|item| item.plays_at(t, duration(item)?).map(|s| (item, s)))
}

/// Where the last of `items` ends on the timeline, in seconds (0 with none).
pub fn items_end(items: &[Item], duration: f64) -> f64 {
    items
        .iter()
        .map(|item| item.timeline_end(duration))
        .fold(0.0, f64::max)
}

/// The most channels a bus can have.
pub const MAX_BUS_CHANNELS: u32 = 8;

/// An output bus: a named set of channels that audio tracks are summed into. The first bus of a
/// project is the master, which the preview plays, the export writes and the graphs' Audio Output
/// writes to; an Audio Bus Output writes to one of the others.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bus {
    pub name: String,
    /// 1 for mono, 2 for stereo, 6 for 5.1, … up to [`MAX_BUS_CHANNELS`].
    pub channels: u32,
    /// Whether it is the project's master bus, the first. Not saved: it follows the order.
    #[serde(skip)]
    pub master: bool,
}

impl Default for Bus {
    fn default() -> Self {
        Self::main()
    }
}

impl Bus {
    /// The bus a project starts with: Main, stereo.
    pub fn main() -> Self {
        Self {
            name: DEFAULT_BUS.to_owned(),
            channels: 2,
            master: true,
        }
    }

    /// A bus other than the master.
    pub fn new(name: impl Into<String>, channels: u32) -> Self {
        Self {
            name: name.into(),
            channels,
            master: false,
        }
    }

    /// Marks the first of `buses` as the master and the others not.
    pub fn mark_master(buses: &mut [Bus]) {
        for (i, bus) in buses.iter_mut().enumerate() {
            bus.master = i == 0;
        }
    }

    /// The bus with its channel count limited to `1..=MAX_BUS_CHANNELS`.
    pub fn sanitized(self) -> Self {
        Self {
            channels: self.channels.clamp(1, MAX_BUS_CHANNELS),
            ..self
        }
    }
}

/// Serializes a [`Rational`] as `"num/den"`.
mod rational {
    use rastersong_media::Rational;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(rate: &Rational, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{}/{}", rate.num, rate.den))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Rational, D::Error> {
        let text = String::deserialize(d)?;
        let parse = |v: &str| v.trim().parse::<i32>().ok().filter(|&n| n > 0);
        let (num, den) = match text.split_once('/') {
            Some((num, den)) => (parse(num), parse(den)),
            None => (parse(&text), Some(1)),
        };
        match (num, den) {
            (Some(num), Some(den)) => Ok(Rational::new(num, den)),
            _ => Err(D::Error::custom(format!(
                "`{text}` is not a frame rate such as `30` or `30000/1001`"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_map_timeline_time_to_resource_time() {
        let item = Item {
            position: 2.0,
            start: 1.0,
            end: Some(5.0),
            rate: 2.0,
            muted: false,
            ..Item::whole(ResourceId(1), 0.0)
        };
        // 4 s of resource at double speed lasts 2 s.
        assert_eq!(item.length(10.0), 2.0);
        assert_eq!(item.timeline_end(10.0), 4.0);
        assert_eq!(item.plays_at(1.9, 10.0), None);
        assert_eq!(item.plays_at(2.0, 10.0), Some(1.0));
        assert_eq!(item.plays_at(3.0, 10.0), Some(3.0));
        assert_eq!(item.plays_at(4.0, 10.0), None);
        assert_eq!(
            Item {
                muted: true,
                ..item
            }
            .plays_at(3.0, 10.0),
            None
        );
        // Without an out point the item plays to the end of the resource.
        assert_eq!(Item::whole(ResourceId(1), 1.5).timeline_end(3.0), 4.5);
        // A graph item lasts as long as it is placed for, whatever the resource's length.
        assert_eq!(Item::graph(ResourceId(2), 1.0, 2.0).timeline_end(1.0), 3.0);
    }

    #[test]
    fn audio_fades_in_and_out_at_item_edges() {
        let item = Item::whole(ResourceId(1), 1.0);
        // A 2 s resource: fades of EDGE_FADE at both ends.
        assert_eq!(item.gain_at(0.999, 2.0), 0.0);
        assert_eq!(item.gain_at(1.0, 2.0), 0.0);
        assert!((item.gain_at(1.0 + EDGE_FADE / 2.0, 2.0) - 0.5).abs() < 1e-9);
        assert_eq!(item.gain_at(1.5, 2.0), 1.0);
        assert!((item.gain_at(3.0 - EDGE_FADE / 4.0, 2.0) - 0.25).abs() < 1e-9);
        assert_eq!(item.gain_at(3.0, 2.0), 0.0);
        // A very short item fades over half its length each way.
        assert_eq!(item.fade(0.002), 0.001);
        assert!((item.gain_at(1.0005, 0.002) - 0.5).abs() < 1e-9);
        let muted = Item {
            muted: true,
            ..item
        };
        assert_eq!(muted.gain_at(1.5, 2.0), 0.0);
    }

    #[test]
    fn later_items_win_where_items_overlap() {
        let items = [
            Item::whole(ResourceId(1), 0.0),
            Item::whole(ResourceId(1), 1.0),
        ];
        let two = |_: &Item| Some(2.0);
        assert_eq!(item_at(&items, 0.5, two).map(|(_, s)| s), Some(0.5));
        assert_eq!(item_at(&items, 1.5, two).map(|(_, s)| s), Some(0.5));
        assert_eq!(item_at(&items, 3.5, two), None);
        // An item of a resource whose length isn't known plays nothing.
        assert_eq!(item_at(&items, 0.5, |_| None), None);
        assert_eq!(items_end(&items, 2.0), 3.0);
        assert_eq!(items_end(&[], 2.0), 0.0);
    }

    #[test]
    fn nonsense_items_are_sanitized() {
        let item = Item {
            position: -1.0,
            start: f64::NAN,
            end: Some(-3.0),
            rate: 0.0,
            muted: false,
            ..Item::whole(ResourceId(1), 0.0)
        }
        .sanitized();
        assert_eq!(
            item,
            Item {
                position: 0.0,
                start: 0.0,
                end: Some(0.0),
                rate: 1.0,
                muted: false,
                ..Item::whole(ResourceId(1), 0.0)
            }
        );
    }

    #[test]
    fn frames_cover_the_timeline() {
        let tb = Timebase::DEFAULT;
        assert_eq!(tb.frames_in(1.0), 30);
        assert_eq!(tb.frames_in(1.01), 31);
        assert_eq!(tb.frames_in(0.0), 0);
    }

    #[test]
    fn timebases_round_trip_with_readable_frame_rates() {
        let tb = Timebase {
            width: 640,
            height: 360,
            frame_rate: Rational::new(30_000, 1001),
        };
        let json = serde_json::to_string(&tb).unwrap();
        assert_eq!(
            json,
            r#"{"width":640,"height":360,"frame_rate":"30000/1001"}"#
        );
        assert_eq!(serde_json::from_str::<Timebase>(&json).unwrap(), tb);
        let whole: Timebase =
            serde_json::from_str(r#"{"width":2,"height":2,"frame_rate":"25"}"#).unwrap();
        assert_eq!(whole.frame_rate, Rational::new(25, 1));
        assert!(
            serde_json::from_str::<Timebase>(r#"{"width":2,"height":2,"frame_rate":"0/1"}"#)
                .is_err()
        );
    }
}
