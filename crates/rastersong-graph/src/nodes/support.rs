//! Helpers shared by several nodes: unit conversion and warmup.

use crate::nodes::{DEFAULT_AUDIO, DEFAULT_VIDEO};
use crate::{Layout, LayoutContext, PrepareContext, ProcessContext};

choice! {
    /// A length unit for user-facing parameters.
    pub enum LengthUnit {
        Rows = "rows",
        Frames = "frames",
    }
}

impl LengthUnit {
    /// Samples in one of this unit.
    pub fn samples(self, ctx: &PrepareContext) -> f64 {
        match self {
            Self::Rows => ctx.samples_per_row() as f64,
            Self::Frames => ctx.samples_per_frame() as f64,
        }
    }
}

choice! {
    /// A unit of time for user-facing parameters: the signal's own milliseconds, or rows or
    /// frames, which look the same at any resolution.
    pub enum TimeUnit {
        Ms = "ms",
        Rows = "rows",
        Frames = "frames",
    }
}

impl TimeUnit {
    /// Samples in one of this unit.
    pub fn samples(self, ctx: &PrepareContext) -> f64 {
        match self {
            Self::Ms => ms_to_samples(1.0, ctx),
            Self::Rows => ctx.samples_per_row() as f64,
            Self::Frames => ctx.samples_per_frame() as f64,
        }
    }
}

choice! {
    /// A frequency unit for user-facing parameters.
    pub enum FreqUnit {
        /// Cycles per row: the pattern looks the same at any resolution.
        Row = "cycles/row",
        /// Cycles per frame.
        Frame = "cycles/frame",
        /// Cycles per second of the signal's own time.
        Hz = "Hz",
    }
}

impl FreqUnit {
    /// Cycles per sample for `value` of this unit.
    pub fn per_sample(self, value: f64, ctx: &PrepareContext) -> f64 {
        match self {
            Self::Row => value / ctx.samples_per_row().max(1) as f64,
            Self::Frame => value / ctx.samples_per_frame().max(1) as f64,
            Self::Hz => value / ctx.sample_rate().max(1.0),
        }
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

    /// The layout of the host signal this choice names.
    pub fn output_layouts(self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let name = match self {
            Self::Video => DEFAULT_VIDEO,
            Self::Audio => DEFAULT_AUDIO,
        };
        ctx.sources
            .get(name)
            .map(|&layout| vec![layout; ctx.output_count])
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

/// Most frames a node with long or infinite memory asks the host to render before a seek.
pub const MAX_WARMUP_FRAMES: u32 = 120;

/// Frames for `samples` of settling time, at least one and at most [`MAX_WARMUP_FRAMES`].
pub fn settle_frames(samples: f64, ctx: &PrepareContext) -> u32 {
    if !samples.is_finite() {
        return MAX_WARMUP_FRAMES;
    }
    ((samples / ctx.samples_per_frame().max(1) as f64).ceil() as u32).clamp(1, MAX_WARMUP_FRAMES)
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

/// Fails unless `layout` is an RGB signal.
pub fn expect_rgb(layout: crate::Layout) -> Result<(), String> {
    if layout.samples_per_pixel == 3 {
        Ok(())
    } else {
        Err(format!("expects an RGB signal, got {layout}"))
    }
}
