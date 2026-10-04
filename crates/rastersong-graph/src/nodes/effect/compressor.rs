use super::{ms_to_samples, settle_frames};
use crate::dsp::{db_to_gain, gain_to_db, smoothing_coefficient};
use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// A feed-forward compressor: turns the signal down by `ratio` above `threshold`, following the
/// level of the input (or of the sidechain, when connected). Times are milliseconds of the
/// signal's own time, so a compressor on a video carrier works over the same fraction of a frame
/// at any resolution.
#[derive(Debug)]
pub struct Compressor {
    threshold: f64,
    ratio: f64,
    attack_ms: f64,
    release_ms: f64,
    knee: f64,
    makeup: f64,
    /// Set in `prepare`.
    attack: f64,
    release: f64,
    sample_rate: f64,
    /// The slowest attack or release modulation can reach, in ms, for warmup.
    slowest_ms: f64,
    sidechain: bool,
    /// Current gain reduction in dB (zero or negative).
    reduction: f64,
}

impl Compressor {
    const THRESHOLD: usize = 0;
    const RATIO: usize = 1;
    const ATTACK: usize = 2;
    const RELEASE: usize = 3;
    const KNEE: usize = 4;
    const MAKEUP: usize = 5;

    pub const PARAMS: &[ParamSpec] = &[
        ParamSpec::number(
            "threshold",
            "Threshold",
            -18.0,
            -60.0,
            0.0,
            "Level above which the signal is turned down",
        )
        .unit("dB")
        .exposed()
        .limits(-200.0, 60.0),
        ParamSpec::number(
            "ratio",
            "Ratio",
            4.0,
            1.0,
            20.0,
            "How much is taken off above the threshold: 4 lets 1 dB through for every 4 dB over",
        )
        .limits(1.0, 1000.0),
        ParamSpec::number(
            "attack",
            "Attack",
            10.0,
            0.01,
            1000.0,
            "How quickly the compressor turns the signal down once it goes over",
        )
        .unit("ms")
        .limits(0.0, 1e6),
        ParamSpec::number(
            "release",
            "Release",
            100.0,
            0.1,
            5000.0,
            "How quickly it lets go once the signal falls back",
        )
        .unit("ms")
        .limits(0.0, 1e6),
        ParamSpec::number(
            "knee",
            "Knee",
            6.0,
            0.0,
            24.0,
            "Width of the soft transition around the threshold; 0 is a hard knee",
        )
        .unit("dB")
        .limits(0.0, 100.0),
        ParamSpec::number(
            "makeup",
            "Makeup",
            0.0,
            -24.0,
            24.0,
            "Gain applied after compression",
        )
        .unit("dB")
        .limits(-96.0, 96.0),
    ];

    pub const SPEC: NodeSpec = NodeSpec::new("Compressor", Category::Effect)
        .describe("Turns loud parts down, following the input or a sidechain")
        .params(Self::PARAMS)
        .per_channel();

    pub fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            threshold: params.number("threshold")?,
            ratio: params.number("ratio")?,
            attack_ms: params.number("attack")?,
            release_ms: params.number("release")?,
            knee: params.number("knee")?,
            makeup: params.number("makeup")?,
            attack: 0.0,
            release: 0.0,
            sample_rate: 1.0,
            slowest_ms: 0.0,
            sidechain: false,
            reduction: 0.0,
        })
    }

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

    /// The smoothing coefficient for a time of `ms`.
    fn coefficient(&self, ms: f64) -> f64 {
        smoothing_coefficient(ms / 1000.0 * self.sample_rate)
    }
}

impl Node for Compressor {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in"), InputSpec::optional("sidechain")];
        INPUTS
    }

    fn prepare(&mut self, ctx: &PrepareContext) {
        self.sample_rate = ctx.sample_rate();
        self.attack = self.coefficient(self.attack_ms);
        self.release = self.coefficient(self.release_ms);
        self.slowest_ms = ctx
            .param_max(Self::ATTACK, self.attack_ms)
            .max(ctx.param_max(Self::RELEASE, self.release_ms));
        self.sidechain = ctx.connected[1];
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = &inputs[0].data;
        let detector = if self.sidechain {
            &inputs[1].data
        } else {
            input
        };
        let value = |index: usize, constant: f64, i: usize| {
            ctx.param(index).map_or(constant, |p| f64::from(p[i]))
        };
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
                value(Self::THRESHOLD, self.threshold, i),
                value(Self::RATIO, self.ratio, i),
                value(Self::KNEE, self.knee, i),
            );
            // More reduction is the attack; less is the release.
            let c = if target < reduction {
                ctx.param(Self::ATTACK)
                    .map_or(self.attack, |a| self.coefficient(f64::from(a[i])))
            } else {
                ctx.param(Self::RELEASE)
                    .map_or(self.release, |r| self.coefficient(f64::from(r[i])))
            };
            reduction = target + c * (reduction - target);
            let makeup = value(Self::MAKEUP, self.makeup, i);
            *out = (f64::from(x) * db_to_gain(reduction + makeup)) as f32;
        }
        self.reduction = reduction;
    }

    fn reset(&mut self) {
        self.reduction = 0.0;
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        // About seven time constants of the slower side to settle within 0.1%.
        settle_frames(7.0 * ms_to_samples(self.slowest_ms, ctx), ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{node, process};

    #[test]
    fn turns_a_steady_loud_signal_down_by_the_ratio() {
        // A hard knee at -20 dB, 4:1: a constant 0 dB signal settles 15 dB down.
        let params = r#"{ "threshold": -20, "ratio": 4, "knee": 0, "attack": 0.1, "release": 1 }"#;
        let mut compressor = node("compressor", params, 4800, 48_000.0, &[true, false]);
        let out = process(compressor.as_mut(), &[vec![1.0; 4800], vec![0.0; 4800]]);
        let settled = f64::from(*out.last().unwrap());
        assert!(
            (settled - 10f64.powf(-15.0 / 20.0)).abs() < 1e-3,
            "{settled}"
        );
        // Quiet signals pass untouched.
        compressor.reset();
        let quiet = process(compressor.as_mut(), &[vec![0.01; 4800], vec![0.0; 4800]]);
        assert!((quiet[4799] - 0.01).abs() < 1e-6);
    }

    #[test]
    fn the_sidechain_drives_the_gain() {
        let params = r#"{ "threshold": -20, "ratio": 20, "knee": 0, "attack": 0.1 }"#;
        let mut compressor = node("compressor", params, 4800, 48_000.0, &[true, true]);
        // A quiet input ducked by a loud sidechain.
        let out = process(compressor.as_mut(), &[vec![0.05; 4800], vec![1.0; 4800]]);
        assert!(out[4799] < 0.01, "{}", out[4799]);
    }

    #[test]
    fn parameter_indices_match_the_specs() {
        use super::Compressor as C;
        let names = [
            (C::THRESHOLD, "threshold"),
            (C::RATIO, "ratio"),
            (C::ATTACK, "attack"),
            (C::RELEASE, "release"),
            (C::KNEE, "knee"),
            (C::MAKEUP, "makeup"),
        ];
        for (index, name) in names {
            assert_eq!(C::PARAMS[index].name, name);
        }
    }

    #[test]
    fn soft_knee_is_continuous() {
        let values = Default::default();
        let params = crate::Params::new(super::Compressor::PARAMS, &values).unwrap();
        let c = super::Compressor::new(&params).unwrap();
        let (low, high) = (c.threshold - c.knee / 2.0, c.threshold + c.knee / 2.0);
        assert!(c.curve(low - 1e-9).abs() < 1e-6);
        let hard = (1.0 / c.ratio - 1.0) * (high - c.threshold);
        assert!((c.curve(high) - hard).abs() < 1e-6);
    }
}
