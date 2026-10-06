use crate::dsp::smoothing_coefficient;
use crate::nodes::support::settle_frames;
use crate::nodes::{Category, NodeKind, NodeSpec, Unit};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

choice! {
    /// How the signal's level is measured.
    pub enum Detector {
        /// The magnitude of each sample: follows fast transients.
        Peak = "peak",
        /// The root of the average power: follows perceived loudness, and is smoother.
        Rms = "rms",
    }
}

/// An envelope follower: turns a signal into a smooth, unsigned curve of how strong it is. Wire
/// the output into another node's parameter to make it follow the input's level.
#[derive(Debug)]
pub struct Envelope {
    detector: Detector,
    attack: f64,
    release: f64,
    unit: Unit,
    /// Samples in one unit, and the smoothing coefficients of the constant times, set in `prepare`.
    unit_samples: f64,
    attack_coefficient: f32,
    release_coefficient: f32,
    /// Longest time constant in samples, for warmup.
    slowest: f64,
    level: f32,
}

params! { Envelope {
    DETECTOR: ParamSpec::choice(
        "detector",
        "Detector",
        Detector::OPTIONS,
        "peak",
        "peak follows each sample's magnitude, rms follows average power and is smoother",
    ),
    ATTACK: ParamSpec::number("attack", "Attack", 5.0, 0.0, 1000.0, "How quickly the output rises when the input gets stronger")
        .limits(0.0, 1e6),
    RELEASE: ParamSpec::number("release", "Release", 50.0, 0.0, 5000.0, "How quickly the output falls when the input gets weaker")
        .limits(0.0, 1e6),
    UNIT: Unit::time_param("ms", "Unit for attack and release"),
} }

impl NodeKind for Envelope {
    const KIND: &'static str = "envelope";
    const SPEC: NodeSpec = NodeSpec::new("Envelope", Category::Effect)
        .describe("Follows how strong the signal is, as a smooth curve from 0 up")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "detector": "rms", "attack": 2, "release": 10 }"#,
        r#"{ "attack": 0.5, "release": 0.5, "unit": "row" }"#,
        r#"{ "attack": 0, "release": 0 }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            detector: params.choice_as(Self::DETECTOR)?,
            attack: params.number_at(Self::ATTACK)?,
            release: params.number_at(Self::RELEASE)?,
            unit: params.choice_as(Self::UNIT)?,
            unit_samples: 1.0,
            attack_coefficient: 0.0,
            release_coefficient: 0.0,
            slowest: 0.0,
            level: 0.0,
        })
    }
}

impl Node for Envelope {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.unit_samples = self.unit.samples(ctx);
        let (attack, release) = (self.attack * self.unit_samples, self.release * self.unit_samples);
        self.attack_coefficient = smoothing_coefficient(attack) as f32;
        self.release_coefficient = smoothing_coefficient(release) as f32;
        // The slowest the times can get, when a signal moves them.
        self.slowest = (ctx.param_max(Self::ATTACK, self.attack))
            .max(ctx.param_max(Self::RELEASE, self.release))
            * self.unit_samples;
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let mut level = self.level;
        // A modulated time gets its coefficient from every sample's value.
        let attack = ctx.param(Self::ATTACK);
        let release = ctx.param(Self::RELEASE);
        let coefficient = |stream: Option<&[f32]>, i: usize, constant: f32| {
            stream.map_or(constant, |s| {
                smoothing_coefficient(f64::from(s[i]).max(0.0) * self.unit_samples) as f32
            })
        };
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            // RMS smooths the power and takes the root afterwards.
            let target = match self.detector {
                Detector::Peak => x.abs(),
                Detector::Rms => x * x,
            };
            let c = if target > level {
                coefficient(attack, i, self.attack_coefficient)
            } else {
                coefficient(release, i, self.release_coefficient)
            };
            level = target + c * (level - target);
            *out = match self.detector {
                Detector::Peak => level,
                Detector::Rms => level.sqrt(),
            };
        }
        self.level = level;
    }

    fn reset(&mut self) {
        self.level = 0.0;
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        // About 7 time constants to settle within 0.1%.
        settle_frames(self.slowest * 7.0, ctx)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn envelope(params: &str, input: &[f32]) -> Vec<f32> {
        // 1000 samples per second, so a millisecond is a sample.
        let mut node = node("envelope", params, input.len(), 1000.0, &[true]);
        process_one(node.as_mut(), &[input.to_vec()])
    }

    #[test]
    fn instant_peak_follows_the_magnitude() {
        let out = envelope(r#"{ "attack": 0, "release": 0 }"#, &[0.5, -1.0, 0.25, 0.0]);
        assert_eq!(out, [0.5, 1.0, 0.25, 0.0]);
    }

    #[test]
    fn attack_and_release_are_gradual_and_unsigned() {
        let mut input = vec![-1.0; 40];
        input.extend([0.0; 40]);
        let out = envelope(r#"{ "attack": 4, "release": 8 }"#, &input);
        assert!(out[0] > 0.0 && out[0] < 1.0);
        assert!(out[39] > 0.99, "settles near the input's magnitude");
        assert!(out[40] < 1.0 && out[40] > 0.8, "falls slowly");
        assert!(out[79] < 0.05);
        assert!(out.iter().all(|&x| x >= 0.0));
    }

    #[test]
    fn rms_of_a_constant_is_its_magnitude() {
        let out = envelope(
            r#"{ "detector": "rms", "attack": 0, "release": 0 }"#,
            &[-0.5; 4],
        );
        assert!(out.iter().all(|&x| (x - 0.5).abs() < 1e-6));
    }
}
