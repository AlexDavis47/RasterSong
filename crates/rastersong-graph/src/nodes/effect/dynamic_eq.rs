use std::f64::consts::PI;

use super::compressor::Compressor;
use super::equalizer::Shape;
use crate::dsp::{AttackRelease, Biquad, BiquadKind, gain_to_db};
use crate::nodes::support::settle_frames;
use crate::nodes::{Category, Meter, NodeKind, NodeSpec, Unit};
use crate::{InputSpec, Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// An equaliser band whose gain follows the level of the signal (or of a sidechain). Above the
/// threshold the band moves toward its `gain` by the compressor's curve, but never further than
/// `gain`: a negative gain turns the band down when the signal is loud (a de-esser, a tamed
/// resonance), a positive gain turns it up (an expander for one range).
#[derive(Debug)]
pub struct DynamicEq {
    shape: Shape,
    unit: Unit,
    frequency: f64,
    q: f64,
    gain: f64,
    threshold: f64,
    ratio: f64,
    attack_time: f64,
    release_time: f64,
    /// Set in `prepare`.
    per_sample: f64,
    times: AttackRelease,
    unit_samples: f64,
    slowest_samples: f64,
    slowest_frequency: f64,
    sidechain: bool,
    /// Whether the frequency or Q is modulated, so the band is redesigned every sample.
    moving: bool,
    /// The gain in dB the band is at now, and the one its filter was last designed with.
    current: f64,
    designed: f64,
    section: Biquad,
}

params! { DynamicEq {
    BAND: ParamSpec::choice("band", &["peak", "low_shelf", "high_shelf"], "peak"),
    FREQUENCY: ParamSpec::number("frequency", 1000.0, 20.0, 20000.0)
        .exposed()
        .limits(1e-06, 1e9),
    UNIT: Unit::freq_param("second"),
    Q: ParamSpec::number("q", 1.0, 0.1, 20.0)
        .limits(0.05, 100.0),
    GAIN: ParamSpec::number("gain", -9.0, -24.0, 24.0)
        .unit("dB")
        .exposed()
        .limits(-48.0, 48.0),
    THRESHOLD: ParamSpec::number("threshold", -18.0, -60.0, 0.0)
        .unit("dB")
        .exposed()
        .limits(-200.0, 60.0),
    RATIO: ParamSpec::number("ratio", 4.0, 1.0, 20.0)
        .limits(1.0, 1000.0),
    ATTACK: ParamSpec::number("attack", 0.01, 0.0001, 1.0)
        .limits(0.0, 1e6),
    RELEASE: ParamSpec::number("release", 0.1, 0.001, 5.0)
        .limits(0.0, 1e6),
} }

impl NodeKind for DynamicEq {
    const KIND: &'static str = "dynamic_eq";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .inputs(&[InputSpec::required("in"), InputSpec::optional("sidechain")])
        .per_channel()
        .meters(&[Meter::gain_reduction("reduction")])
        .expects(crate::Range::Bipolar);
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "band": "peak", "frequency": 0.9, "gain": -12, "threshold": -24 }"#,
        r#"{ "band": "high_shelf", "frequency": 1.2, "gain": 9, "ratio": 8 }"#,
        r#"{ "band": "low_shelf", "frequency": 0.4, "gain": -6, "attack": 0.001, "release": 0.01 }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            shape: params.choice_as(Self::BAND)?,
            unit: params.choice_as(Self::UNIT)?,
            frequency: params.number_at(Self::FREQUENCY)?,
            q: params.number_at(Self::Q)?,
            gain: params.number_at(Self::GAIN)?,
            threshold: params.number_at(Self::THRESHOLD)?,
            ratio: params.number_at(Self::RATIO)?,
            attack_time: params.number_at(Self::ATTACK)?,
            release_time: params.number_at(Self::RELEASE)?,
            per_sample: 0.0,
            times: AttackRelease::default(),
            unit_samples: 1.0,
            slowest_samples: 0.0,
            slowest_frequency: 0.0,
            sidechain: false,
            moving: false,
            current: 0.0,
            designed: 0.0,
            section: Biquad::default(),
        })
    }
}

impl DynamicEq {
    /// The band at `frequency` (this node's unit), quality factor `q` and gain `gain_db`.
    fn design(&self, frequency: f64, q: f64, gain_db: f64) -> Biquad {
        let kind = match self.shape {
            Shape::LowShelf => BiquadKind::LowShelf { gain_db },
            Shape::HighShelf => BiquadKind::HighShelf { gain_db },
            _ => BiquadKind::Peak { gain_db },
        };
        Biquad::design(kind, frequency * self.per_sample, q)
    }

    /// How far toward `gain` the band is for a level (dB): the compressor's curve, held to
    /// `gain`'s size and sign.
    fn target(level: f64, threshold: f64, ratio: f64, gain: f64) -> f64 {
        let reduction = -Compressor::reduction(level, threshold, ratio, 0.0);
        gain.signum() * reduction.min(gain.abs())
    }
}

impl Node for DynamicEq {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.per_sample = self.unit.per_sample(1.0, ctx);
        self.unit_samples = self.unit.samples(ctx);
        self.times = AttackRelease::new(self.attack_time, self.release_time, self.unit_samples);
        self.slowest_samples = ctx
            .param_max(Self::ATTACK, self.attack_time)
            .max(ctx.param_max(Self::RELEASE, self.release_time))
            * self.unit_samples;
        self.slowest_frequency = ctx.param_min(Self::FREQUENCY, self.frequency) * self.per_sample;
        self.sidechain = ctx.connected[1];
        self.moving = ctx.modulation(Self::FREQUENCY).is_some() || ctx.modulation(Self::Q).is_some();
        self.reset();
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = &inputs[0].data;
        let detector = if self.sidechain {
            &inputs[1].data
        } else {
            input
        };
        let frequency = ctx.value(Self::FREQUENCY, self.frequency);
        let q = ctx.value(Self::Q, self.q);
        let gain = ctx.value(Self::GAIN, self.gain);
        let threshold = ctx.value(Self::THRESHOLD, self.threshold);
        let ratio = ctx.value(Self::RATIO, self.ratio);
        let (attack, release) = (ctx.param(Self::ATTACK), ctx.param(Self::RELEASE));
        for (i, ((out, &x), &d)) in outputs[0]
            .data
            .iter_mut()
            .zip(input)
            .zip(detector)
            .enumerate()
        {
            let target = Self::target(
                gain_to_db(f64::from(d.abs())),
                threshold.at64(i),
                ratio.at64(i),
                gain.at64(i),
            );
            // Moving further from flat is the attack; coming back is the release.
            let c = if target.abs() > self.current.abs() {
                self.times.attack(attack, i)
            } else {
                self.times.release(release, i)
            };
            self.current = target + c * (self.current - target);
            if self.moving || (self.current - self.designed).abs() > 0.005 {
                self.section
                    .retune(self.design(frequency.at64(i), q.at64(i), self.current));
                self.designed = self.current;
            }
            *out = self.section.process(f64::from(x)) as f32;
        }
    }

    fn meters(&self, out: &mut [f32]) {
        out[0] = (-self.current).max(0.0) as f32;
    }

    fn reset(&mut self) {
        self.current = 0.0;
        self.designed = 0.0;
        self.section = self.design(self.frequency, self.q, 0.0);
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        // The envelope settles in about seven time constants of the slower side; the filter
        // rings for about seven of Q / (π f).
        let ring = 7.0 * self.q.max(1.0) / (PI * self.slowest_frequency.max(1e-9));
        settle_frames((7.0 * self.slowest_samples).max(ring), ctx)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use crate::testing::{node, process_one};

    /// Peak of a sine of `amplitude` at 30 cycles per 256-sample row, after settling.
    fn peak(params: &str, amplitude: f64) -> f32 {
        let len = 256;
        let mut node = node("dynamic_eq", params, len, len as f64, &[true, false]);
        let sine: Vec<f32> = (0..len)
            .map(|i| (amplitude * (TAU * 30.0 * i as f64 / len as f64).sin()) as f32)
            .collect();
        for _ in 0..30 {
            process_one(node.as_mut(), std::slice::from_ref(&sine));
        }
        let out = process_one(node.as_mut(), &[sine]);
        out.iter().fold(0.0f32, |p, &x| p.max(x.abs()))
    }

    const CUT: &str = r#"{ "frequency": 30, "gain": -12, "threshold": -30, "ratio": 20, "attack": 0.0001, "release": 1000, "q": 2 }"#;

    #[test]
    fn a_quiet_signal_is_left_alone() {
        let quiet = 0.01; // -40 dB, under the threshold
        assert!((peak(CUT, quiet) / quiet as f32 - 1.0).abs() < 0.05);
    }

    #[test]
    fn a_loud_signal_in_the_band_is_turned_down_by_up_to_the_gain() {
        let loud = 0.9; // about -1 dB, far over the threshold
        let ratio = peak(CUT, loud) / loud as f32;
        // -12 dB is about x0.25: the band reaches its full cut but no further.
        assert!(ratio < 0.4 && ratio > 0.2, "{ratio}");
    }
}
