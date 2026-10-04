//! Helpers shared by several nodes: unit conversion and warmup.

use crate::PrepareContext;

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
