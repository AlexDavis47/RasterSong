//! Helpers shared by several nodes: unit conversion and warmup.

use crate::desc::GeneratorLayout;
use crate::nodes::{DEFAULT_AUDIO, DEFAULT_VIDEO};
use crate::{Layout, LayoutContext, PrepareContext, ProcessContext, Range, Reading, Tag};

choice! {
    /// A unit for user-facing time and frequency parameters. Each is "how many samples is one of
    /// these": a time multiplies by it, a frequency (cycles per unit) divides by it. Rows, frames,
    /// pixels and note lengths look the same at any resolution or are tempo-locked; seconds and
    /// milliseconds are the signal's own time.
    ///
    /// The note lengths are counted by the parameter's value: 3 sixteenths, 2 bars. A beat and a
    /// bar follow the time signature (an eighth note in 6/8); the other notes are fixed fractions
    /// of a whole note. Each comes straight, dotted (half as long again) or as a triplet (two
    /// thirds as long).
    pub enum Unit {
        /// A pixel of the project's own resolution. A scaled-down preview scales it with the
        /// image, so it looks like a scaled-down version of the full render.
        Pixel = "pixel",
        /// The literal sample of the render in front of you. The same value means something
        /// different at another preview scale.
        Sample = "sample",
        /// Rows of the signal (pixel rows of a frame; slices of an audio block).
        Row = "row",
        Frame = "frame",
        /// Milliseconds of the signal's own time.
        Ms = "ms",
        Second = "second",
        ThirtySecond = "32nd",
        ThirtySecondDotted = "32nd_dotted",
        ThirtySecondTriplet = "32nd_triplet",
        Sixteenth = "16th",
        SixteenthDotted = "16th_dotted",
        SixteenthTriplet = "16th_triplet",
        Eighth = "8th",
        EighthDotted = "8th_dotted",
        EighthTriplet = "8th_triplet",
        /// The note the time signature's denominator names: one metronome click.
        Beat = "beat",
        BeatDotted = "beat_dotted",
        BeatTriplet = "beat_triplet",
        Half = "half",
        HalfDotted = "half_dotted",
        HalfTriplet = "half_triplet",
        /// A bar of the time signature.
        Bar = "bar",
        BarDotted = "bar_dotted",
        BarTriplet = "bar_triplet",
    }
}

/// A note length's base, which a [`Unit`] plays straight, dotted or as a triplet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    ThirtySecond,
    Sixteenth,
    Eighth,
    Beat,
    Half,
    Bar,
}

/// How a note length is played: as written, dotted or as a triplet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feel {
    Straight,
    Dotted,
    Triplet,
}

impl Feel {
    /// How many of the straight note one of these lasts.
    pub fn factor(self) -> f64 {
        match self {
            Self::Straight => 1.0,
            Self::Dotted => 1.5,
            Self::Triplet => 2.0 / 3.0,
        }
    }
}

impl Unit {
    /// The note length this unit is, or `None` for one that isn't tempo-locked.
    pub fn note(self) -> Option<(Note, Feel)> {
        use {Feel::*, Note::*};
        Some(match self {
            Self::Pixel | Self::Sample | Self::Row | Self::Frame | Self::Ms | Self::Second => {
                return None;
            }
            Self::ThirtySecond => (ThirtySecond, Straight),
            Self::ThirtySecondDotted => (ThirtySecond, Dotted),
            Self::ThirtySecondTriplet => (ThirtySecond, Triplet),
            Self::Sixteenth => (Sixteenth, Straight),
            Self::SixteenthDotted => (Sixteenth, Dotted),
            Self::SixteenthTriplet => (Sixteenth, Triplet),
            Self::Eighth => (Eighth, Straight),
            Self::EighthDotted => (Eighth, Dotted),
            Self::EighthTriplet => (Eighth, Triplet),
            Self::Beat => (Beat, Straight),
            Self::BeatDotted => (Beat, Dotted),
            Self::BeatTriplet => (Beat, Triplet),
            Self::Half => (Half, Straight),
            Self::HalfDotted => (Half, Dotted),
            Self::HalfTriplet => (Half, Triplet),
            Self::Bar => (Bar, Straight),
            Self::BarDotted => (Bar, Dotted),
            Self::BarTriplet => (Bar, Triplet),
        })
    }

    /// Whether the unit follows the project tempo.
    pub fn is_tempo(self) -> bool {
        self.note().is_some()
    }

    /// Samples in one of this unit.
    pub fn samples(self, ctx: &PrepareContext) -> f64 {
        let Some((note, feel)) = self.note() else {
            return match self {
                Self::Pixel => ctx.samples_per_pixel(),
                Self::Sample => 1.0,
                Self::Row => ctx.samples_per_row() as f64,
                Self::Frame => ctx.samples_per_frame() as f64,
                Self::Ms => ms_to_samples(1.0, ctx),
                _ => ctx.sample_rate(),
            };
        };
        let quarter = ctx.samples_per_quarter();
        let straight = match note {
            Note::ThirtySecond => quarter / 8.0,
            Note::Sixteenth => quarter / 4.0,
            Note::Eighth => quarter / 2.0,
            Note::Beat => ctx.samples_per_beat(),
            Note::Half => quarter * 2.0,
            Note::Bar => ctx.samples_per_bar(),
        };
        straight * feel.factor()
    }

    /// Cycles per sample for `value` cycles per this unit.
    pub fn per_sample(self, value: f64, ctx: &PrepareContext) -> f64 {
        value / self.samples(ctx).max(1e-9)
    }
}

/// What a length or rate parameter's number is counted in: its own unit and, for a rate, whether
/// the number is cycles per unit or a period, one cycle every so many units. Read with
/// [`crate::Params::measure_at`], then [`Self::prepare`]d so a value converts with one multiply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measure {
    pub unit: Unit,
    pub reading: Reading,
    /// Samples in one unit, set by `prepare`.
    samples: f64,
}

impl Measure {
    pub fn new(unit: Unit, reading: Reading) -> Self {
        Self {
            unit,
            reading,
            samples: 1.0,
        }
    }

    /// Works out how long the unit is in this context.
    pub fn prepare(&mut self, ctx: &PrepareContext) {
        self.samples = self.unit.samples(ctx).max(1e-9);
    }

    /// Samples in one unit.
    pub fn unit_samples(&self) -> f64 {
        self.samples
    }

    /// A length of `value` units, in samples.
    pub fn samples(&self, value: f64) -> f64 {
        value * self.samples
    }

    /// Cycles per sample of a rate of `value`. Read as a period, a period of zero or less is
    /// still: no cycles at all.
    pub fn per_sample(&self, value: f64) -> f64 {
        match self.reading {
            Reading::Per => value / self.samples,
            Reading::Every if value > 0.0 => 1.0 / (value * self.samples),
            Reading::Every => 0.0,
        }
    }

    /// The slowest rate, in cycles per sample, parameter `index` (at `base`) can reach under
    /// modulation: its lowest value, or read as a period, its longest.
    pub fn slowest(&self, ctx: &PrepareContext, index: usize, base: f64) -> f64 {
        match self.reading {
            Reading::Per => self.per_sample(ctx.param_min(index, base)),
            Reading::Every => self.per_sample(ctx.param_max(index, base)),
        }
    }

    /// The fastest rate, in cycles per sample, parameter `index` (at `base`) can reach under
    /// modulation: its highest value, or read as a period, its shortest.
    pub fn fastest(&self, ctx: &PrepareContext, index: usize, base: f64) -> f64 {
        match self.reading {
            Reading::Per => self.per_sample(ctx.param_max(index, base)),
            Reading::Every => self.per_sample(ctx.param_min(index, base).max(1e-6)),
        }
    }
}

impl GeneratorLayout {
    /// The range of the host signal this choice names: what a generator's values usually mean.
    pub fn nominal(self) -> Range {
        match self {
            Self::Video => Range::Unipolar,
            Self::Audio => Range::Bipolar,
        }
    }

    /// The shape of the host signal this choice names, tagged as that kind with the generator's
    /// `range`.
    pub fn output_layouts(self, ctx: &LayoutContext, range: Range) -> Result<Vec<Layout>, String> {
        let (name, tag) = match self {
            Self::Video => (DEFAULT_VIDEO, Tag::VIDEO),
            Self::Audio => (DEFAULT_AUDIO, Tag::AUDIO),
        };
        let tag = Tag { range, ..tag };
        ctx.sources
            .get(name)
            .map(|&layout| {
                vec![layout.with_tag(tag.fit(layout.samples_per_pixel)); ctx.output_count]
            })
            .ok_or_else(|| format!("the host provides no `{name}` source to take a layout from"))
    }
}

/// Where a generator is in its stream, counted in samples from the start of the render.
///
/// The first block after a reset starts at `frame × block length`, so a render that begins at a
/// seek position (after its warmup) produces exactly what a render from the start does. After
/// that the generator counts its own samples, so it doesn't depend on how the stream is cut.
#[derive(Debug, Clone, Copy, Default)]
pub struct SampleClock {
    next: Option<u64>,
}

impl SampleClock {
    /// Index of the first sample of this block; advances past the block's `len` samples.
    pub fn begin(&mut self, ctx: &ProcessContext, len: usize) -> u64 {
        let start = self.next.unwrap_or(ctx.frame * len as u64);
        self.next = Some(start + len as u64);
        start
    }

    pub fn reset(&mut self) {
        self.next = None;
    }
}

/// What [`Node::warmup_frames`](crate::Node::warmup_frames) reports for a node that never settles
/// (feedback at or above unity, say). The host decides how many frames it will pre-render.
pub const UNBOUNDED_WARMUP: u32 = u32::MAX;

/// Frames for `samples` of settling time, at least one. A node reports its real length: the
/// host, not the node, limits how much of it is pre-rendered after a seek.
pub fn settle_frames(samples: f64, ctx: &PrepareContext) -> u32 {
    if !samples.is_finite() {
        return UNBOUNDED_WARMUP;
    }
    ((samples / ctx.samples_per_frame().max(1) as f64).ceil() as u32).max(1)
}

/// Samples of the main signal in `ms` milliseconds of its own time.
pub fn ms_to_samples(ms: f64, ctx: &PrepareContext) -> f64 {
    ms / 1000.0 * ctx.sample_rate()
}

// Conversions between video (`0..=1`) and audio (`-1..=1`) both model writing to and reading from
// an 8-bit file, so values are clipped to the range.
//
// The `bugged` mapping reproduces the original prototype's glitch: pixels were written as signed
// 8-bit samples (`pixel - 127`) into an 8-bit WAV, which audio software reads as unsigned. The
// misread flips the sign bit, so the audio wraps around at mid-gray: black and white are near
// silence, and the darker and brighter halves of the image sit at opposite extremes. Effects
// applied in between push samples across that seam, and on the way back to video they wrap to
// the other end of the brightness range.
choice! {
    /// How samples map between the video and audio ranges.
    pub enum Mapping {
        /// Black is -1, white is 1.
        Accurate = "accurate",
        /// Signed samples read as unsigned: the range wraps around at mid-gray.
        Bugged = "bugged",
    }
}

impl Mapping {
    /// The parameters of both conversion nodes.
    pub const MAPPING_PARAM: crate::ParamSpec =
        crate::ParamSpec::choice("mapping", Self::OPTIONS, "accurate");
}

/// The largest signed 8-bit sample, 127/128.
pub const SIGNED_MAX: f32 = 127.0 / 128.0;

/// Reading a signed sample as unsigned (or the reverse): offsets it by half the range, wrapping
/// the top half to the bottom. Its own inverse for samples in `-1..1`.
pub fn flip_sign_bit(a: f32) -> f32 {
    if a < 0.0 { a + 1.0 } else { a - 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Tempo;

    /// A context for a mono signal of `len` samples per frame at 30 frames a second.
    fn with_context<R>(len: u32, tempo: Tempo, f: impl FnOnce(&PrepareContext) -> R) -> R {
        let layout = Layout::mono(len, 1);
        f(&PrepareContext {
            frame_rate: 30.0,
            tempo,
            inputs: &[layout],
            outputs: &[layout],
            connected: &[true],
            modulated: &[],
            pixel_scale: 1.0,
            span: crate::Span::default(),
        })
    }

    fn tempo(bpm: f64, beats_per_bar: u32) -> Tempo {
        Tempo {
            bpm,
            beats_per_bar,
            beat_unit: 4,
            offset_secs: 0.0,
        }
    }

    #[test]
    fn beats_and_bars_follow_the_time_signature_and_notes_do_not() {
        // 120 eighth notes a minute in 6/8: 30000 samples a second.
        let six_eight = Tempo {
            beat_unit: 8,
            ..tempo(120.0, 6)
        };
        with_context(1000, six_eight, |ctx| {
            assert_eq!(Unit::Beat.samples(ctx), 15_000.0);
            assert_eq!(Unit::Eighth.samples(ctx), 15_000.0);
            assert_eq!(Unit::Bar.samples(ctx), 90_000.0);
            assert_eq!(Unit::Half.samples(ctx), 60_000.0);
        });
    }

    #[test]
    fn note_lengths_follow_the_tempo() {
        // 30000 samples a second.
        with_context(1000, tempo(120.0, 4), |ctx| {
            assert_eq!(Unit::Second.samples(ctx), 30_000.0);
            assert_eq!(Unit::Beat.samples(ctx), 15_000.0);
            assert_eq!(Unit::Bar.samples(ctx), 60_000.0);
            assert_eq!(Unit::Half.samples(ctx), 30_000.0);
            assert_eq!(Unit::Eighth.samples(ctx), 7_500.0);
            assert_eq!(Unit::Sixteenth.samples(ctx), 3_750.0);
            assert_eq!(Unit::ThirtySecond.samples(ctx), 1_875.0);
            assert_eq!(Unit::EighthDotted.samples(ctx), 11_250.0);
            assert_eq!(Unit::BeatTriplet.samples(ctx), 10_000.0);
        });
        with_context(1000, tempo(90.0, 3), |ctx| {
            assert!((Unit::Beat.samples(ctx) - 20_000.0).abs() < 1e-6);
            assert!((Unit::Bar.samples(ctx) - 60_000.0).abs() < 1e-6);
        });
    }

    #[test]
    fn time_units_agree_with_each_other() {
        for (bpm, len) in [(90.0, 800), (120.0, 1000), (133.0, 1470)] {
            with_context(len, tempo(bpm, 4), |ctx| {
                let ms = Unit::Ms.samples(ctx);
                assert!((Unit::Second.samples(ctx) - ms * 1000.0).abs() < 1e-6);
                let beat_ms = 60_000.0 / bpm;
                let beat = Unit::Beat.samples(ctx);
                assert!((beat - ms * beat_ms).abs() < 1e-6 * ms * beat_ms);
                assert!((Unit::Bar.samples(ctx) - 4.0 * beat).abs() < 1e-6);
            });
        }
    }

    #[test]
    fn frequencies_are_cycles_per_unit() {
        use crate::nodes::Choice;
        for bpm in [90.0, 120.0, 133.0] {
            with_context(1000, tempo(bpm, 4), |ctx| {
                for &name in Unit::OPTIONS {
                    let unit = Unit::from_option(name).unwrap();
                    let per_sample = unit.per_sample(1.0, ctx);
                    assert!(
                        (per_sample * unit.samples(ctx) - 1.0).abs() < 1e-9,
                        "{name} at {bpm} bpm"
                    );
                }
            });
        }
    }

    #[test]
    fn pixels_follow_the_preview_scale_and_samples_do_not() {
        let samples = |layout: Layout, pixel_scale: f64, unit: Unit| {
            unit.samples(&PrepareContext {
                frame_rate: 30.0,
                tempo: Tempo::default(),
                inputs: &[layout],
                outputs: &[layout],
                connected: &[true],
                modulated: &[],
                pixel_scale,
                span: crate::Span::default(),
            })
        };
        let video = Layout::video(8, 4);
        // Three samples (R, G, B) make a pixel; at half size, half of that is a project pixel.
        assert_eq!(samples(video, 1.0, Unit::Pixel), 3.0);
        assert_eq!(samples(video, 0.5, Unit::Pixel), 1.5);
        assert_eq!(samples(video, 0.5, Unit::Sample), 1.0);
        // A row, as a whole, keeps its own meaning.
        assert_eq!(samples(video, 0.5, Unit::Row), 24.0);
        // Audio has no preview scale: a pixel is one frame of its channels.
        assert_eq!(
            samples(Layout::audio_channels(100, 2), 0.5, Unit::Pixel),
            2.0
        );
    }

    #[test]
    fn unit_names_are_the_same_for_times_and_frequencies() {
        use crate::nodes::Choice;
        for name in [
            "pixel",
            "sample",
            "row",
            "frame",
            "ms",
            "second",
            "16th_dotted",
            "bar",
        ] {
            assert!(Unit::from_option(name).is_some(), "{name}");
        }
    }

    #[test]
    fn beat_offset_is_in_samples() {
        let t = Tempo {
            offset_secs: 0.5,
            ..Tempo::default()
        };
        with_context(1000, t, |ctx| {
            assert_eq!(ctx.beat_offset_samples(), 15_000.0)
        });
    }
}
