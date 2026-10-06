//! Helpers shared by several nodes: unit conversion and warmup.

use crate::nodes::{DEFAULT_AUDIO, DEFAULT_VIDEO};
use crate::{Layout, LayoutContext, PrepareContext, ProcessContext, Range, Tag};

choice! {
    /// A unit of time for user-facing parameters. Rows and frames look the same at any
    /// resolution; beats and bars follow the project tempo.
    pub enum TimeUnit {
        /// Rows of the signal (pixel rows of a frame; slices of an audio block).
        Rows = "rows",
        Frames = "frames",
        /// Milliseconds of the signal's own time.
        Ms = "ms",
        Seconds = "seconds",
        /// Beats at the project tempo.
        Beats = "beats",
        /// Bars at the project tempo and time signature.
        Bars = "bars",
    }
}

impl TimeUnit {
    /// The `unit` parameter of a node with time parameters.
    pub const fn param(default: &'static str, help: &'static str) -> crate::ParamSpec {
        crate::ParamSpec::choice("unit", "Unit", Self::OPTIONS, default, help)
    }

    /// Samples in one of this unit.
    pub fn samples(self, ctx: &PrepareContext) -> f64 {
        match self {
            Self::Rows => ctx.samples_per_row() as f64,
            Self::Frames => ctx.samples_per_frame() as f64,
            Self::Ms => ms_to_samples(1.0, ctx),
            Self::Seconds => ctx.sample_rate(),
            Self::Beats => ctx.samples_per_beat(),
            Self::Bars => ctx.samples_per_bar(),
        }
    }
}

choice! {
    /// A frequency unit for user-facing parameters: the inverse of a [`TimeUnit`].
    pub enum FreqUnit {
        /// Cycles per row: the pattern looks the same at any resolution.
        Row = "Row",
        /// Cycles per frame.
        Frame = "Frame",
        /// Cycles per second of the signal's own time.
        Hz = "Hertz",
        /// Cycles per beat at the project tempo.
        Beat = "Beat",
        /// Cycles per bar at the project tempo and time signature.
        Bar = "Bar",
    }
}

impl FreqUnit {
    /// The `unit` parameter of a node with frequency parameters.
    pub const fn param(default: &'static str, help: &'static str) -> crate::ParamSpec {
        crate::ParamSpec::choice("unit", "Unit", Self::OPTIONS, default, help)
    }

    /// Cycles per sample for `value` of this unit.
    pub fn per_sample(self, value: f64, ctx: &PrepareContext) -> f64 {
        let per = match self {
            Self::Row => ctx.samples_per_row() as f64,
            Self::Frame => ctx.samples_per_frame() as f64,
            Self::Hz => ctx.sample_rate(),
            Self::Beat => ctx.samples_per_beat(),
            Self::Bar => ctx.samples_per_bar(),
        };
        value / per.max(1.0)
    }
}

choice! {
    /// Which host signal a generator takes its layout (resolution or sample count) from.
    pub enum GeneratorLayout {
        /// The video's layout: RGB pixels in rows.
        Video = "video",
        /// The audio track's layout.
        Audio = "audio",
    }
}

impl GeneratorLayout {
    /// The `layout` parameter every generator has.
    pub const PARAM: crate::ParamSpec = crate::ParamSpec::choice(
        "layout",
        "Layout",
        Self::OPTIONS,
        "video",
        "video makes a signal shaped like the video (RGB, rows); audio makes one shaped like the audio track",
    );

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
    pub const MAPPING_PARAM: crate::ParamSpec = crate::ParamSpec::choice(
        "mapping",
        "Mapping",
        Self::OPTIONS,
        "accurate",
        "accurate maps black to -1 and white to 1; bugged reproduces the signed/unsigned misread, wrapping at mid-gray",
    );
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
        })
    }

    fn tempo(bpm: f64, beats_per_bar: u32) -> Tempo {
        Tempo {
            bpm,
            beats_per_bar,
            offset_secs: 0.0,
        }
    }

    #[test]
    fn beats_and_bars_follow_the_tempo() {
        // 30000 samples a second.
        with_context(1000, tempo(120.0, 4), |ctx| {
            assert_eq!(TimeUnit::Seconds.samples(ctx), 30_000.0);
            assert_eq!(TimeUnit::Beats.samples(ctx), 15_000.0);
            assert_eq!(TimeUnit::Bars.samples(ctx), 60_000.0);
        });
        with_context(1000, tempo(90.0, 3), |ctx| {
            assert!((TimeUnit::Beats.samples(ctx) - 20_000.0).abs() < 1e-6);
            assert!((TimeUnit::Bars.samples(ctx) - 60_000.0).abs() < 1e-6);
        });
    }

    #[test]
    fn time_units_agree_with_each_other() {
        for (bpm, len) in [(90.0, 800), (120.0, 1000), (133.0, 1470)] {
            with_context(len, tempo(bpm, 4), |ctx| {
                let ms = TimeUnit::Ms.samples(ctx);
                assert!((TimeUnit::Seconds.samples(ctx) - ms * 1000.0).abs() < 1e-6);
                let beat_ms = 60_000.0 / bpm;
                assert!((TimeUnit::Beats.samples(ctx) - ms * beat_ms).abs() < 1e-6 * ms * beat_ms);
                assert!(
                    (TimeUnit::Bars.samples(ctx) - 4.0 * TimeUnit::Beats.samples(ctx)).abs() < 1e-6
                );
            });
        }
    }

    #[test]
    fn frequency_units_invert_the_time_units() {
        let pairs = [
            (FreqUnit::Row, TimeUnit::Rows),
            (FreqUnit::Frame, TimeUnit::Frames),
            (FreqUnit::Hz, TimeUnit::Seconds),
            (FreqUnit::Beat, TimeUnit::Beats),
            (FreqUnit::Bar, TimeUnit::Bars),
        ];
        for bpm in [90.0, 120.0, 133.0] {
            with_context(1000, tempo(bpm, 4), |ctx| {
                for (freq, time) in pairs {
                    let per_sample = freq.per_sample(1.0, ctx);
                    assert!(
                        (per_sample * time.samples(ctx) - 1.0).abs() < 1e-9,
                        "{freq:?} vs {time:?} at {bpm} bpm"
                    );
                }
            });
        }
    }

    #[test]
    fn saved_unit_names_still_parse() {
        use crate::nodes::Choice;
        for name in ["rows", "frames", "ms"] {
            assert!(TimeUnit::from_option(name).is_some(), "{name}");
        }
        for name in ["Row", "Frame", "Hertz", "Beat", "Bar"] {
            assert!(FreqUnit::from_option(name).is_some(), "{name}");
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
