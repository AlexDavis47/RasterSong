use rastersong_lang::{tr, tr_args};

/// Describes how a signal's samples map onto an image, and what they are meant to be. Effect nodes
/// ignore it; structural nodes and unit conversion (e.g. "one row") use the shape, and editors and
/// compile warnings use the [`Tag`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    /// 1 for a single channel, 2 for interleaved stereo, 3 for interleaved RGB.
    pub samples_per_pixel: u32,
    /// What the samples are meant to be. Advisory: processing never looks at it.
    pub tag: Tag,
}

impl Layout {
    pub const EMPTY: Self = Self::new(0, 0, 0);

    /// An untagged layout.
    pub const fn new(width: u32, height: u32, samples_per_pixel: u32) -> Self {
        Self {
            width,
            height,
            samples_per_pixel,
            tag: Tag::UNKNOWN,
        }
    }

    /// A single-channel image.
    pub const fn mono(width: u32, height: u32) -> Self {
        Self::new(width, height, 1)
    }

    /// An image with interleaved R, G, B samples.
    pub const fn rgb(width: u32, height: u32) -> Self {
        Self::new(width, height, 3)
    }

    /// RGB video in `0..=1`, tagged as such.
    pub const fn video(width: u32, height: u32) -> Self {
        Self::rgb(width, height).with_tag(Tag::VIDEO)
    }

    /// One frame's worth of mono audio in `-1..=1`: a single row of `samples`.
    pub const fn audio(samples: u32) -> Self {
        Self::audio_channels(samples, 1)
    }

    /// One frame's worth of audio with `channels` interleaved channels (L, R, L, R, … for
    /// stereo): a single row of `frames` pixels.
    pub const fn audio_channels(frames: u32, channels: u32) -> Self {
        let layout = Self::new(frames, 1, channels);
        layout.with_tag(Tag::AUDIO.fit(channels))
    }

    pub const fn with_tag(mut self, tag: Tag) -> Self {
        self.tag = tag;
        self
    }

    /// The same shape with another width, height and channel count, keeping what it's meant to
    /// be (kind and range). The channel meaning is worked out again for the new channel count.
    pub const fn reshaped(self, width: u32, height: u32, samples_per_pixel: u32) -> Self {
        Self {
            width,
            height,
            samples_per_pixel,
            tag: self.tag.fit(samples_per_pixel),
        }
    }

    /// Whether both have the same width, height and channel count, whatever their tags say.
    pub const fn same_shape(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.samples_per_pixel == other.samples_per_pixel
    }

    /// Samples in one frame-sized block.
    pub const fn len(&self) -> usize {
        self.width as usize * self.height as usize * self.samples_per_pixel as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn pixels(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Samples in one row; the unit behind "rows" and "cycles per row" parameters.
    pub const fn samples_per_row(&self) -> usize {
        self.width as usize * self.samples_per_pixel as usize
    }
}

impl std::fmt::Display for Layout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.samples_per_pixel {
            1 => write!(f, "{}×{} mono", self.width, self.height),
            2 => write!(f, "{}×{} stereo", self.width, self.height),
            3 => write!(f, "{}×{} RGB", self.width, self.height),
            n => write!(f, "{}×{}×{}", self.width, self.height, n),
        }
    }
}

/// What a signal is meant to be: its kind, what its interleaved channels are, which part of a
/// whole it is, and its nominal range. Worked out when the graph compiles and carried in every
/// [`Layout`]. **Advisory only**: it colours wires and produces warnings, but no node may refuse
/// or convert a signal because of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Tag {
    pub kind: Kind,
    pub channels: ChannelMap,
    pub part: Part,
    pub range: Range,
}

impl Tag {
    pub const UNKNOWN: Self = Self {
        kind: Kind::Unknown,
        channels: ChannelMap::Unknown,
        part: Part::Whole,
        range: Range::Unknown,
    };

    /// RGB video in `0..=1`.
    pub const VIDEO: Self = Self {
        kind: Kind::Video,
        channels: ChannelMap::Rgb,
        part: Part::Whole,
        range: Range::Unipolar,
    };

    /// Audio in `-1..=1`; the channel meaning comes from the channel count (see [`Self::fit`]).
    pub const AUDIO: Self = Self {
        kind: Kind::Audio,
        channels: ChannelMap::Unknown,
        part: Part::Whole,
        range: Range::Bipolar,
    };

    /// This tag for a signal with `samples_per_pixel` channels: a channel meaning that doesn't
    /// match the count (stereo on three channels, say) is replaced by the usual one for it. An
    /// unknown meaning is filled in once the kind is known.
    pub const fn fit(mut self, samples_per_pixel: u32) -> Self {
        let counts = match self.channels.count() {
            Some(n) => n == samples_per_pixel,
            None => {
                samples_per_pixel > 0
                    && match self.channels {
                        ChannelMap::Numbered => true,
                        // An untagged signal stays untagged.
                        _ => matches!(self.kind, Kind::Unknown),
                    }
            }
        };
        if !counts || !self.channels.suits(self.kind) {
            self.channels = ChannelMap::usual(samples_per_pixel, self.kind);
        }
        self
    }

    /// The name of channel `index` of a signal with this tag: `R`, `G`, `B`, `L`, `R`, or a
    /// number counting from 1.
    pub fn channel_name(&self, index: usize) -> String {
        match (self.channels, index) {
            (ChannelMap::Rgb, 0..=2) => ["R", "G", "B"][index].to_owned(),
            (ChannelMap::Stereo, 0..=1) => ["L", "R"][index].to_owned(),
            _ => (index + 1).to_string(),
        }
    }

    /// The part channel `index` of a signal with this tag is, once split off.
    pub fn channel_part(&self, index: usize) -> Part {
        match (self.channels, index) {
            (ChannelMap::Rgb, 0) => Part::Red,
            (ChannelMap::Rgb, 1) => Part::Green,
            (ChannelMap::Rgb, 2) => Part::Blue,
            (ChannelMap::Stereo, 0) => Part::Left,
            (ChannelMap::Stereo, 1) => Part::Right,
            _ => Part::Channel(index.min(u8::MAX as usize) as u8),
        }
    }
}

impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut words = Vec::new();
        if self.kind != Kind::Unknown {
            words.push(self.kind.label().to_owned());
        }
        if let Some(label) = self.channels.label() {
            words.push(label.to_owned());
        }
        if let Some(label) = self.part.label() {
            words.push(label);
        }
        if let Some(label) = self.range.label() {
            words.push(label.to_owned());
        }
        if words.is_empty() {
            f.write_str(tr("signal.untagged"))
        } else {
            f.write_str(&words.join(", "))
        }
    }
}

/// Whether a signal is meant as a picture or as sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Kind {
    #[default]
    Unknown,
    Video,
    Audio,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => tr("signal.kind.unknown"),
            Self::Video => tr("signal.kind.video"),
            Self::Audio => tr("signal.kind.audio"),
        }
    }
}

/// What a signal's interleaved channels are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ChannelMap {
    #[default]
    Unknown,
    /// One channel.
    Mono,
    /// Left and right: L, R, L, R, …
    Stereo,
    /// Red, green and blue: R, G, B, R, G, B, …
    Rgb,
    /// Any number of channels without names.
    Numbered,
}

impl ChannelMap {
    /// How many channels this meaning has, or `None` for any number.
    pub const fn count(self) -> Option<u32> {
        match self {
            Self::Mono => Some(1),
            Self::Stereo => Some(2),
            Self::Rgb => Some(3),
            Self::Unknown | Self::Numbered => None,
        }
    }

    /// Whether the meaning makes sense for a signal of this kind (stereo video doesn't).
    const fn suits(self, kind: Kind) -> bool {
        !matches!(
            (self, kind),
            (Self::Stereo, Kind::Video) | (Self::Rgb, Kind::Audio)
        )
    }

    /// The meaning a signal of `kind` with `samples_per_pixel` channels usually has.
    pub const fn usual(samples_per_pixel: u32, kind: Kind) -> Self {
        match (samples_per_pixel, kind) {
            (0, _) => Self::Unknown,
            (1, _) => Self::Mono,
            (2, Kind::Video) | (3, Kind::Audio) => Self::Numbered,
            (2, _) => Self::Stereo,
            (3, _) => Self::Rgb,
            _ => Self::Numbered,
        }
    }

    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Unknown => None,
            Self::Mono => Some(tr("signal.channels.mono")),
            Self::Stereo => Some(tr("signal.channels.stereo")),
            Self::Rgb => Some(tr("signal.channels.rgb")),
            Self::Numbered => Some(tr("signal.channels.numbered")),
        }
    }
}

/// Which part of a whole signal this is: one channel split off, or one frequency band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Part {
    #[default]
    Whole,
    Red,
    Green,
    Blue,
    Left,
    Right,
    /// An unnamed channel, counting from 0.
    Channel(u8),
    Low,
    Mid,
    High,
}

impl Part {
    pub fn label(self) -> Option<String> {
        Some(match self {
            Self::Whole => return None,
            Self::Red => tr("signal.part.red").into(),
            Self::Green => tr("signal.part.green").into(),
            Self::Blue => tr("signal.part.blue").into(),
            Self::Left => tr("signal.part.left").into(),
            Self::Right => tr("signal.part.right").into(),
            Self::Channel(i) => tr_args(
                "signal.part.channel",
                &[("number", &(u32::from(i) + 1).to_string())],
            ),
            Self::Low => tr("signal.part.low").into(),
            Self::Mid => tr("signal.part.mid").into(),
            Self::High => tr("signal.part.high").into(),
        })
    }
}

/// The range a signal's values nominally span. Nothing enforces it; nodes may declare the range
/// they expect, which only produces a warning when it differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Range {
    #[default]
    Unknown,
    /// `0..=1`, the video range.
    Unipolar,
    /// `-1..=1`, the audio range.
    Bipolar,
}

impl Range {
    /// The range values from `lo` to `hi` fit: `0..1` if they're inside it, else `-1..1` if
    /// they're inside that, else unknown.
    pub fn from_bounds(lo: f64, hi: f64) -> Self {
        const EPS: f64 = 1e-6;
        if !(lo.is_finite() && hi.is_finite()) {
            return Self::Unknown;
        }
        let (lo, hi) = (lo.min(hi), lo.max(hi));
        if lo >= -EPS && hi <= 1.0 + EPS {
            Self::Unipolar
        } else if lo >= -1.0 - EPS && hi <= 1.0 + EPS {
            Self::Bipolar
        } else {
            Self::Unknown
        }
    }

    /// The lowest and highest nominal values, or `None` when unknown.
    pub fn bounds(self) -> Option<(f64, f64)> {
        match self {
            Self::Unknown => None,
            Self::Unipolar => Some((0.0, 1.0)),
            Self::Bipolar => Some((-1.0, 1.0)),
        }
    }

    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Unknown => None,
            Self::Unipolar => Some(tr("signal.range.unipolar")),
            Self::Bipolar => Some(tr("signal.range.bipolar")),
        }
    }
}

/// How an output's [`Tag`] is set: each field that is `Some` replaces that part of the tag the
/// node produced (by default, its main input's); `None` leaves it. Static per output port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TagRule {
    pub kind: Option<Kind>,
    pub part: Option<Part>,
    pub range: Option<Range>,
}

impl TagRule {
    /// Everything from what the node produced (most effects pass their main input's tag).
    pub const INHERIT: Self = Self {
        kind: None,
        part: None,
        range: None,
    };
    /// Video in `0..=1`; a part (one channel, one band) stays that part.
    pub const VIDEO: Self = Self::INHERIT.kind(Kind::Video).range(Range::Unipolar);
    /// Audio in `-1..=1`; a part stays that part.
    pub const AUDIO: Self = Self::INHERIT.kind(Kind::Audio).range(Range::Bipolar);

    pub const fn kind(mut self, kind: Kind) -> Self {
        self.kind = Some(kind);
        self
    }

    pub const fn part(mut self, part: Part) -> Self {
        self.part = Some(part);
        self
    }

    pub const fn range(mut self, range: Range) -> Self {
        self.range = Some(range);
        self
    }

    pub fn is_inherit(&self) -> bool {
        *self == Self::INHERIT
    }

    /// `tag` with this rule's fields replaced, fitted to `samples_per_pixel` channels.
    pub fn apply(&self, tag: Tag, samples_per_pixel: u32) -> Tag {
        let kind = self.kind.unwrap_or(tag.kind);
        // Channels that only had the usual meaning for the old kind get the new kind's.
        let usual = ChannelMap::usual(samples_per_pixel, tag.kind);
        let channels = if kind != tag.kind && tag.channels == usual {
            ChannelMap::Unknown
        } else {
            tag.channels
        };
        Tag {
            kind,
            channels,
            part: self.part.unwrap_or(tag.part),
            range: self.range.unwrap_or(tag.range),
        }
        .fit(samples_per_pixel)
    }
}

/// One frame of a signal: a block of samples plus its layout.
///
/// Video signals are nominally in `0.0..=1.0` (black to full intensity); audio signals in
/// `-1.0..=1.0`. Nothing clamps values between nodes, only the output does.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Signal {
    pub data: Vec<f32>,
    pub layout: Layout,
}

impl Signal {
    pub const EMPTY: Self = Self {
        data: Vec::new(),
        layout: Layout::EMPTY,
    };

    pub fn zeros(layout: Layout) -> Self {
        Self {
            data: vec![0.0; layout.len()],
            layout,
        }
    }

    pub fn from_data(layout: Layout, data: Vec<f32>) -> Self {
        assert_eq!(
            data.len(),
            layout.len(),
            "data does not match layout {layout}"
        );
        Self { data, layout }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_meaning_follows_the_channel_count() {
        assert_eq!(Layout::video(2, 2).tag.channels, ChannelMap::Rgb);
        assert_eq!(
            Layout::audio_channels(4, 2).tag.channels,
            ChannelMap::Stereo
        );
        assert_eq!(Layout::audio(4).tag.channels, ChannelMap::Mono);
        // Three audio channels aren't RGB; two video channels aren't stereo.
        assert_eq!(
            Layout::audio_channels(4, 3).tag.channels,
            ChannelMap::Numbered
        );
        assert_eq!(
            Layout::video(2, 2).reshaped(2, 2, 2).tag.channels,
            ChannelMap::Numbered
        );
        // Reshaping keeps the kind and range.
        let mono = Layout::video(2, 2).reshaped(6, 2, 1);
        assert_eq!(mono.tag.kind, Kind::Video);
        assert_eq!(mono.tag.range, Range::Unipolar);
        assert_eq!(mono.tag.channels, ChannelMap::Mono);
    }

    #[test]
    fn shapes_compare_without_tags() {
        assert!(Layout::video(2, 2).same_shape(&Layout::rgb(2, 2)));
        assert_ne!(Layout::video(2, 2), Layout::rgb(2, 2));
        assert!(!Layout::rgb(2, 2).same_shape(&Layout::mono(6, 2)));
    }

    #[test]
    fn ranges_come_from_bounds() {
        assert_eq!(Range::from_bounds(0.0, 1.0), Range::Unipolar);
        assert_eq!(Range::from_bounds(0.25, 0.5), Range::Unipolar);
        assert_eq!(Range::from_bounds(-0.5, 0.5), Range::Bipolar);
        assert_eq!(Range::from_bounds(1.0, -1.0), Range::Bipolar);
        assert_eq!(Range::from_bounds(0.0, 2.0), Range::Unknown);
        assert_eq!(Range::from_bounds(f64::NAN, 0.0), Range::Unknown);
    }

    #[test]
    fn rules_replace_only_what_they_set() {
        let tag = Layout::video(2, 2).tag;
        let audio = TagRule::AUDIO.apply(tag, 3);
        assert_eq!(audio.kind, Kind::Audio);
        assert_eq!(audio.range, Range::Bipolar);
        assert_eq!(audio.channels, ChannelMap::Numbered);
        let low = TagRule::INHERIT.part(Part::Low).apply(tag, 3);
        assert_eq!(
            low,
            Tag {
                part: Part::Low,
                ..tag
            }
        );
        assert_eq!(TagRule::INHERIT.apply(tag, 3), tag);
    }

    #[test]
    fn channels_are_named_by_their_meaning() {
        let rgb = Layout::video(1, 1).tag;
        let stereo = Layout::audio_channels(1, 2).tag;
        assert_eq!(rgb.channel_name(1), "G");
        assert_eq!(stereo.channel_name(1), "R");
        assert_eq!(stereo.channel_part(0), Part::Left);
        assert_eq!(Tag::UNKNOWN.channel_name(3), "4");
        assert_eq!(Tag::UNKNOWN.channel_part(3), Part::Channel(3));
        assert_eq!(rgb.to_string(), "video, RGB, 0 to 1");
    }
}
