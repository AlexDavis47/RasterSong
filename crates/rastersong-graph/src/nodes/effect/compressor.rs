use crate::dsp::{AttackRelease, db_to_gain, gain_to_db};
use crate::nodes::support::settle_frames;
use crate::nodes::{Category, Meter, NodeKind, NodeSpec, Unit};
use crate::{InputSpec, Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// A feed-forward compressor: turns the signal down by `ratio` above `threshold`, following the
/// level of the input (or of the sidechain, when connected). Times are milliseconds of the
/// signal's own time by default, so a compressor on a video carrier works over the same fraction
/// of a frame at any resolution; `unit` can make them rows, frames, seconds, beats or bars.
#[derive(Debug)]
pub struct Compressor {
    threshold: f64,
    ratio: f64,
    /// Attack and release, in `unit`.
    attack_time: f64,
    release_time: f64,
    unit: Unit,
    knee: f64,
    makeup: f64,
    /// Set in `prepare`.
    times: AttackRelease,
    /// Samples in one `unit`.
    unit_samples: f64,
    /// The slowest attack or release modulation can reach, in samples, for warmup.
    slowest_samples: f64,
    sidechain: bool,
    /// Current gain reduction in dB (zero or negative).
    reduction: f64,
}

params! { Compressor {
    THRESHOLD: ParamSpec::number("threshold", -18.0,
        -60.0,
        0.0)
    .unit("dB")
    .exposed()
    .limits(-200.0, 60.0),
    RATIO: ParamSpec::number("ratio", 4.0,
        1.0,
        20.0)
    .limits(1.0, 1000.0),
    ATTACK: ParamSpec::number("attack", 10.0,
        0.01,
        1000.0)
    .limits(0.0, 1e6),
    RELEASE: ParamSpec::number("release", 100.0,
        0.1,
        5000.0)
    .limits(0.0, 1e6),
    UNIT: Unit::time_param("ms"),
    KNEE: ParamSpec::number("knee", 6.0,
        0.0,
        24.0)
    .unit("dB")
    .limits(0.0, 100.0),
    MAKEUP: ParamSpec::number("makeup", 0.0,
        -24.0,
        24.0)
    .unit("dB")
    .limits(-96.0, 96.0),
} }

impl NodeKind for Compressor {
    const KIND: &'static str = "compressor";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .inputs(&[InputSpec::required("in"), InputSpec::optional("sidechain")])
        .per_channel()
        .meters(&[Meter::gain_reduction("reduction")])
        .expects(crate::Range::Bipolar);
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "threshold": -12, "ratio": 6, "attack": 2, "release": 20 }"#,
        r#"{ "threshold": -30, "knee": 0, "makeup": 6 }"#,
        r#"{ "threshold": -12, "unit": "bar", "attack": 0.001, "release": 0.02 }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            threshold: params.number_at(Self::THRESHOLD)?,
            ratio: params.number_at(Self::RATIO)?,
            attack_time: params.number_at(Self::ATTACK)?,
            release_time: params.number_at(Self::RELEASE)?,
            unit: params.choice_as(Self::UNIT)?,
            knee: params.number_at(Self::KNEE)?,
            makeup: params.number_at(Self::MAKEUP)?,
            times: AttackRelease::default(),
            unit_samples: 1.0,
            slowest_samples: 0.0,
            sidechain: false,
            reduction: 0.0,
        })
    }
}

impl Compressor {
    /// Static gain reduction in dB for an input level in dB.
    pub fn curve(&self, level: f64) -> f64 {
        Self::reduction(level, self.threshold, self.ratio, self.knee)
    }

    /// Gain reduction in dB for an input level in dB, with a soft knee `knee` dB wide.
    fn reduction(level: f64, threshold: f64, ratio: f64, knee: f64) -> f64 {
        let over = level - threshold;
        let slope = 1.0 / ratio.max(1.0) - 1.0;
        if knee > 0.0 && 2.0 * over.abs() <= knee {
            slope * (over + knee / 2.0).powi(2) / (2.0 * knee)
        } else if over > 0.0 {
            slope * over
        } else {
            0.0
        }
    }
}

impl Node for Compressor {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.unit_samples = self.unit.samples(ctx);
        self.times = AttackRelease::new(self.attack_time, self.release_time, self.unit_samples);
        self.slowest_samples = ctx
            .param_max(Self::ATTACK, self.attack_time)
            .max(ctx.param_max(Self::RELEASE, self.release_time))
            * self.unit_samples;
        self.sidechain = ctx.connected[1];
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = &inputs[0].data;
        let detector = if self.sidechain {
            &inputs[1].data
        } else {
            input
        };
        let threshold = ctx.value(Self::THRESHOLD, self.threshold);
        let ratio = ctx.value(Self::RATIO, self.ratio);
        let knee = ctx.value(Self::KNEE, self.knee);
        let makeup = ctx.value(Self::MAKEUP, self.makeup);
        let (attack, release) = (ctx.param(Self::ATTACK), ctx.param(Self::RELEASE));
        let mut reduction = self.reduction;
        for (i, ((out, &x), &d)) in outputs[0]
            .data
            .iter_mut()
            .zip(input)
            .zip(detector)
            .enumerate()
        {
            let target = Self::reduction(
                gain_to_db(f64::from(d.abs())),
                threshold.at64(i),
                ratio.at64(i),
                knee.at64(i),
            );
            // More reduction is the attack; less is the release.
            let c = if target < reduction {
                self.times.attack(attack, i)
            } else {
                self.times.release(release, i)
            };
            reduction = target + c * (reduction - target);
            *out = (f64::from(x) * db_to_gain(reduction + makeup.at64(i))) as f32;
        }
        self.reduction = reduction;
    }

    fn meters(&self, out: &mut [f32]) {
        out[0] = (-self.reduction).max(0.0) as f32;
    }

    fn reset(&mut self) {
        self.reduction = 0.0;
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        // About seven time constants of the slower side to settle within 0.1%.
        settle_frames(7.0 * self.slowest_samples, ctx)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    #[test]
    fn turns_a_steady_loud_signal_down_by_the_ratio() {
        // A hard knee at -20 dB, 4:1: a constant 0 dB signal settles 15 dB down.
        let params = r#"{ "threshold": -20, "ratio": 4, "knee": 0, "attack": 0.1, "release": 1 }"#;
        let mut compressor = node("compressor", params, 4800, 48_000.0, &[true, false]);
        let out = process_one(compressor.as_mut(), &[vec![1.0; 4800], vec![0.0; 4800]]);
        let settled = f64::from(*out.last().unwrap());
        assert!(
            (settled - 10f64.powf(-15.0 / 20.0)).abs() < 1e-3,
            "{settled}"
        );
        // Quiet signals pass untouched.
        compressor.reset();
        let quiet = process_one(compressor.as_mut(), &[vec![0.01; 4800], vec![0.0; 4800]]);
        assert!((quiet[4799] - 0.01).abs() < 1e-6);
    }

    #[test]
    fn the_sidechain_drives_the_gain() {
        let params = r#"{ "threshold": -20, "ratio": 20, "knee": 0, "attack": 0.1 }"#;
        let mut compressor = node("compressor", params, 4800, 48_000.0, &[true, true]);
        // A quiet input ducked by a loud sidechain.
        let out = process_one(compressor.as_mut(), &[vec![0.05; 4800], vec![1.0; 4800]]);
        assert!(out[4799] < 0.01, "{}", out[4799]);
    }

    #[test]
    fn makeup_gain_applies_after_compression() {
        let params = r#"{ "threshold": 0, "makeup": 6 }"#;
        let mut compressor = node("compressor", params, 480, 48_000.0, &[true, false]);
        let out = process_one(compressor.as_mut(), &[vec![0.1; 480], vec![0.0; 480]]);
        // Below the threshold only the makeup gain acts: +6 dB is a factor of about 2.
        assert!((out[479] - 0.1 * 10f32.powf(6.0 / 20.0)).abs() < 1e-5);
    }

    #[test]
    fn soft_knee_is_continuous() {
        let values = Default::default();
        let params = crate::Params::new(super::Compressor::PARAMS, &values).unwrap();
        let c = <super::Compressor as crate::NodeKind>::new(&params).unwrap();
        let (low, high) = (c.threshold - c.knee / 2.0, c.threshold + c.knee / 2.0);
        assert!(c.curve(low - 1e-9).abs() < 1e-6);
        let hard = (1.0 / c.ratio - 1.0) * (high - c.threshold);
        assert!((c.curve(high) - hard).abs() < 1e-6);
    }
}
