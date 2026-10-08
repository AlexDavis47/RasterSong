//! The project's timeline: its timebase, and tracks of items placed on it.
//!
//! The project is the clock, like a Premiere sequence: it has its own resolution and frame rate,
//! and every track is conformed to that grid on its way into a graph. A track holds items of one
//! resource (a video or an audio file); an item places a stretch of the resource, between its in
//! and out points, at a position on the timeline, played at a rate. Positions and in/out points
//! are times in seconds, so they survive a change of frame rate. Gaps between items read zeros.

use std::path::PathBuf;

use rastersong_media::Rational;
use serde::{Deserialize, Serialize};

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

/// What a track holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
}

/// A stretch of a track's resource placed on the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    /// Seconds into the project where the item starts.
    #[serde(default)]
    pub position: f64,
    /// Seconds into the resource where the item starts: its in point.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub start: f64,
    /// Seconds into the resource where the item ends: its out point. `None` plays to the end of
    /// the resource, whatever its length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
    /// Seconds of resource played per second of timeline: 2 plays twice as fast. Video holds or
    /// skips frames; audio is resampled, so its pitch moves like tape.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub rate: f64,
    /// A muted item reads as a gap.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
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
    /// The whole resource at `position`, at its own speed.
    pub fn whole(position: f64) -> Self {
        Self {
            position,
            start: 0.0,
            end: None,
            rate: 1.0,
            muted: false,
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
            muted: self.muted,
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

    /// The resource time played at timeline time `t`, if the item plays then.
    pub fn plays_at(&self, t: f64, duration: f64) -> Option<f64> {
        (!self.muted && t >= self.position && t < self.timeline_end(duration))
            .then(|| self.source_time(t))
    }
}

/// The item of `items` that plays at timeline time `t`, and the resource time it plays: the last
/// one listed when items overlap.
pub fn item_at(items: &[Item], t: f64, duration: f64) -> Option<(&Item, f64)> {
    items
        .iter()
        .rev()
        .find_map(|item| item.plays_at(t, duration).map(|s| (item, s)))
}

/// Where the last of `items` ends on the timeline, in seconds (0 with none).
pub fn items_end(items: &[Item], duration: f64) -> f64 {
    items
        .iter()
        .map(|item| item.timeline_end(duration))
        .fold(0.0, f64::max)
}

/// A track as the engine renders it.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackSpec {
    /// The name the graph's input nodes select it by.
    pub name: String,
    pub kind: TrackKind,
    pub path: PathBuf,
    pub items: Vec<Item>,
}

/// What the engine renders: the timebase and the tracks.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Timeline {
    /// The project's timebase. `None` takes it from the first video track, or
    /// [`Timebase::DEFAULT`] when there is none.
    pub timebase: Option<Timebase>,
    pub tracks: Vec<TrackSpec>,
}

impl Timeline {
    /// The first video track, which a project without a timebase of its own takes it from.
    pub fn first_video(&self) -> Option<&TrackSpec> {
        self.tracks.iter().find(|t| t.kind == TrackKind::Video)
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
        assert_eq!(Item::whole(1.5).timeline_end(3.0), 4.5);
    }

    #[test]
    fn later_items_win_where_items_overlap() {
        let items = [Item::whole(0.0), Item::whole(1.0)];
        assert_eq!(item_at(&items, 0.5, 2.0).map(|(_, s)| s), Some(0.5));
        assert_eq!(item_at(&items, 1.5, 2.0).map(|(_, s)| s), Some(0.5));
        assert_eq!(item_at(&items, 3.5, 2.0), None);
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
