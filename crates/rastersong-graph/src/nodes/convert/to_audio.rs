use crate::nodes::support::{Mapping, SIGNED_MAX, flip_sign_bit};
use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{InputSpec, Node, OutputSpec, Params, ProcessContext, Signal, TagRule};

/// Video (`0..=1`) to audio (`-1..=1`).
#[derive(Debug)]
pub struct ToAudio {
    mapping: Mapping,
}

params! { ToAudio {
    MAPPING: Mapping::MAPPING_PARAM,
} }

impl NodeKind for ToAudio {
    const KIND: &'static str = "to_audio";
    const SPEC: NodeSpec = NodeSpec::new(Category::Convert)
        .params(Self::PARAMS)
        .inputs(&[InputSpec::required("in")])
        .outputs(&[OutputSpec::new("out").tag(TagRule::AUDIO)]);
    const TEST_CONFIGS: &'static [&'static str] =
        &[r#"{ "mapping": "bugged" }"#, r#"{ "mapping": "accurate" }"#];
    const BENCH: Option<&'static str> = Some(r#"{ "mapping": "bugged" }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            mapping: params.choice_as(Self::MAPPING)?,
        })
    }
}

impl ToAudio {
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
    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        for (out, &x) in outputs[0].data.iter_mut().zip(&inputs[0].data) {
            *out = Self::convert(self.mapping, x);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::ToVideo;

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
