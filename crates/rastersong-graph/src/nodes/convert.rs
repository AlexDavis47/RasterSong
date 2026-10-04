//! Conversions between the video range (`0..=1`) and the audio range (`-1..=1`).
//!
//! Both model writing to and reading from an 8-bit file, so values are clipped to the range.
//!
//! The `bugged` mapping reproduces the original prototype's glitch: pixels were written as signed
//! 8-bit samples (`pixel - 127`) into an 8-bit WAV, which audio software reads as unsigned. The
//! misread flips the sign bit, so the audio wraps around at mid-gray: black and white are near
//! silence, and the darker and brighter halves of the image sit at opposite extremes. Effects
//! applied in between push samples across that seam, and on the way back to video they wrap to
//! the other end of the brightness range.

use crate::{InputSpec, Node, ParamSpec, Params, ProcessContext, Signal};

/// The largest signed 8-bit sample, 127/128.
const SIGNED_MAX: f32 = 127.0 / 128.0;

/// Reading a signed sample as unsigned (or the reverse): offsets it by half the range, wrapping
/// the top half to the bottom. Its own inverse for samples in `-1..1`.
fn flip_sign_bit(a: f32) -> f32 {
    if a < 0.0 { a + 1.0 } else { a - 1.0 }
}

/// How samples map between the video and audio ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mapping {
    /// Black is -1, white is 1.
    Accurate,
    /// Signed samples read as unsigned: the range wraps around at mid-gray.
    Bugged,
}

impl Mapping {
    const PARAMS: &[ParamSpec] = &[ParamSpec::choice(
        "mapping",
        "Mapping",
        &["accurate", "bugged"],
        "accurate",
        "accurate maps black to -1 and white to 1; bugged reproduces the signed/unsigned misread, wrapping at mid-gray",
    )];

    fn read(params: &Params) -> Result<Self, String> {
        Ok(match params.choice("mapping")? {
            "bugged" => Self::Bugged,
            _ => Self::Accurate,
        })
    }
}

/// Video (`0..=1`) to audio (`-1..=1`).
#[derive(Debug)]
pub struct ToAudio {
    mapping: Mapping,
}

impl ToAudio {
    pub const PARAMS: &[ParamSpec] = Mapping::PARAMS;

    pub fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            mapping: Mapping::read(params)?,
        })
    }

    pub fn convert(mapping: Mapping, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match mapping {
            Mapping::Accurate => 2.0 * x - 1.0,
            // The prototype's `pixel - 127`, in 8-bit steps. Signed 8-bit tops out one step
            // below 1, which keeps white clear of black after the flip.
            Mapping::Bugged => flip_sign_bit(((255.0 * x - 127.0) / 128.0).min(SIGNED_MAX)),
        }
    }
}

impl Node for ToAudio {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        for (out, &x) in outputs[0].data.iter_mut().zip(&inputs[0].data) {
            *out = Self::convert(self.mapping, x);
        }
    }
}

/// Audio (`-1..=1`) back to video (`0..=1`). `bugged` misreads the audio again, which undoes the
/// first misread; samples that crossed the seam wrap to the other end of the brightness range.
#[derive(Debug)]
pub struct ToVideo {
    mapping: Mapping,
}

impl ToVideo {
    pub const PARAMS: &[ParamSpec] = Mapping::PARAMS;

    pub fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            mapping: Mapping::read(params)?,
        })
    }

    pub fn convert(mapping: Mapping, a: f32) -> f32 {
        match mapping {
            Mapping::Accurate => (a.clamp(-1.0, 1.0) + 1.0) / 2.0,
            Mapping::Bugged => {
                let signed = flip_sign_bit(a.clamp(-1.0, SIGNED_MAX));
                ((128.0 * signed + 127.0) / 255.0).max(0.0)
            }
        }
    }
}

impl Node for ToVideo {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        for (out, &a) in outputs[0].data.iter_mut().zip(&inputs[0].data) {
            *out = Self::convert(self.mapping, a);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The prototype's bugged round trip on 8-bit values: pixel → signed byte → read as unsigned
    /// 8-bit audio, and audio → unsigned byte → read as signed → pixel.
    fn prototype_to_audio(pixel: u8) -> f32 {
        let signed = (i16::from(pixel) - 127).clamp(-127, 127) as i8;
        (f32::from(signed as u8) - 128.0) / 128.0
    }

    fn prototype_to_video(audio: f32) -> u8 {
        let unsigned = (audio * 128.0 + 128.0).round().clamp(0.0, 255.0) as u8;
        (i16::from(unsigned as i8) + 127).clamp(0, 255) as u8
    }

    #[test]
    fn bugged_matches_the_prototype_to_audio() {
        for pixel in 0..=255u8 {
            let ours = ToAudio::convert(Mapping::Bugged, f32::from(pixel) / 255.0);
            let theirs = prototype_to_audio(pixel);
            assert!(
                (ours - theirs).abs() < 1e-6,
                "pixel {pixel}: {ours} vs {theirs}"
            );
        }
    }

    #[test]
    fn bugged_matches_the_prototype_to_video() {
        for step in -160..=160 {
            let audio = step as f32 / 128.0;
            let ours = ToVideo::convert(Mapping::Bugged, audio);
            let theirs = f32::from(prototype_to_video(audio)) / 255.0;
            assert!(
                (ours - theirs).abs() < 1e-6,
                "audio {audio}: {ours} vs {theirs}"
            );
        }
    }

    #[test]
    fn bugged_wraps_at_mid_gray() {
        let to_audio = |pixel: u8| ToAudio::convert(Mapping::Bugged, f32::from(pixel) / 255.0);
        let to_video = |audio: f32| ToVideo::convert(Mapping::Bugged, audio) * 255.0;
        // Black and white sit next to silence, either side of it; the seam is between 126 and 127.
        assert_eq!(to_audio(0), 1.0 / 128.0);
        assert_eq!(to_audio(255), -1.0 / 128.0);
        assert_eq!(to_audio(126), SIGNED_MAX);
        assert_eq!(to_audio(127), -1.0);
        // Turning the gain up pushes samples into clipping, which reads back as mid-gray.
        assert!((to_video(3.0) - 126.0).abs() < 1e-4);
        assert!((to_video(-3.0) - 127.0).abs() < 1e-4);
        // A small negative swing on black wraps to near white.
        assert!(to_video(-0.02) > 250.0);
    }

    #[test]
    fn round_trips_are_lossless_below_white() {
        for mapping in [Mapping::Accurate, Mapping::Bugged] {
            for i in 0..=255 {
                let x = i as f32 / 255.0;
                let back = ToVideo::convert(mapping, ToAudio::convert(mapping, x));
                // Bugged loses white's top step to the signed range, as the prototype did.
                let tolerance = if i == 255 { 1.001 / 255.0 } else { 1e-6 };
                assert!((back - x).abs() <= tolerance, "{mapping:?} {x} -> {back}");
            }
        }
    }

    #[test]
    fn accurate_is_linear_and_clips() {
        assert_eq!(ToAudio::convert(Mapping::Accurate, 0.0), -1.0);
        assert_eq!(ToAudio::convert(Mapping::Accurate, 1.0), 1.0);
        assert_eq!(ToAudio::convert(Mapping::Accurate, 1.5), 1.0);
        assert_eq!(ToVideo::convert(Mapping::Accurate, 0.0), 0.5);
        assert_eq!(ToVideo::convert(Mapping::Accurate, 2.0), 1.0);
    }
}
