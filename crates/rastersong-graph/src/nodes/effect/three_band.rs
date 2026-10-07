use super::filter::Slope;
use crate::dsp::{Biquad, MAX_STAGES, butterworth_cascade};
use crate::nodes::{Category, NodeKind, NodeSpec, Unit};
use crate::{
    Node, OutputSpec, ParamSpec, Params, Part, PrepareContext, ProcessContext, Signal, TagRule,
};
use rastersong_lang::tr_args;

/// Splits a signal into low, mid and high bands. Crossovers are in Hz of the input signal's own
/// sample rate (for an audio input, ordinary Hz) unless `unit` says otherwise. Mid is what remains after removing low and high,
/// so the three bands always add back up to the input.
#[derive(Debug)]
pub struct ThreeBand {
    low_hz: f64,
    high_hz: f64,
    unit: Unit,
    slope: Slope,
    /// The slowest the low crossover can get, in cycles per sample, for warmup: set in `prepare`.
    low_cycles: f64,
    /// Cycles per sample of one unit, set in `prepare`.
    scale: f64,
    low: [Biquad; MAX_STAGES],
    high: [Biquad; MAX_STAGES],
}

params! { ThreeBand {
    LOW_HZ: ParamSpec::number("low_hz", 250.0,
        1.0,
        100_000.0)
    .limits(0.001, 1e9),
    HIGH_HZ: ParamSpec::number("high_hz", 4000.0,
        1.0,
        100_000.0)
    .limits(0.001, 1e9),
    UNIT: Unit::freq_param("second"),
    SLOPE: ParamSpec::choice("slope", &["12", "24", "48"], "12"),
} }

impl NodeKind for ThreeBand {
    const KIND: &'static str = "three_band";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .outputs(&[
            OutputSpec::new("low").tag(TagRule::INHERIT.part(Part::Low)),
            OutputSpec::new("mid").tag(TagRule::INHERIT.part(Part::Mid)),
            OutputSpec::new("high").tag(TagRule::INHERIT.part(Part::High)),
        ])
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "low_hz": 300, "high_hz": 3000 }"#,
        r#"{ "low_hz": 2, "high_hz": 9, "unit": "beat" }"#,
        r#"{ "low_hz": 300, "high_hz": 3000, "slope": "24" }"#,
        r#"{ "low_hz": 300, "high_hz": 3000, "slope": "48" }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        let low_hz = params.number_at(Self::LOW_HZ)?;
        let high_hz = params.number_at(Self::HIGH_HZ)?;
        if low_hz >= high_hz {
            return Err(tr_args(
                "error.three_band.order",
                &[("low", &low_hz.to_string()), ("high", &high_hz.to_string())],
            ));
        }
        Ok(Self {
            low_hz,
            high_hz,
            unit: params.choice_as(Self::UNIT)?,
            slope: params.choice_as(Self::SLOPE)?,
            low_cycles: 0.0,
            scale: 1.0,
            low: [Biquad::default(); MAX_STAGES],
            high: [Biquad::default(); MAX_STAGES],
        })
    }
}

impl ThreeBand {
    /// The sections of a crossover filter at `hz` (in this node's unit).
    fn cascade(&self, hz: f64, high_pass: bool) -> [Biquad; MAX_STAGES] {
        butterworth_cascade(
            high_pass,
            self.slope.stages(),
            hz * self.scale,
            std::f64::consts::FRAC_1_SQRT_2,
        )
    }

    /// Runs `x` through the first `stages` sections.
    fn run(sections: &mut [Biquad], stages: usize, x: f64) -> f64 {
        sections
            .iter_mut()
            .take(stages)
            .fold(x, |y, s| s.process(y))
    }
}

impl Node for ThreeBand {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.low_cycles = self
            .unit
            .per_sample(ctx.param_min(Self::LOW_HZ, self.low_hz), ctx);
        self.scale = self.unit.per_sample(1.0, ctx);
        self.low = self.cascade(self.low_hz, false);
        self.high = self.cascade(self.high_hz, true);
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let [low, mid, high] = outputs else {
            unreachable!()
        };
        let (low_hz, high_hz) = (ctx.param(Self::LOW_HZ), ctx.param(Self::HIGH_HZ));
        let stages = self.slope.stages();
        for (i, &x) in inputs[0].data.iter().enumerate() {
            // A moved crossover retunes its filter without losing the filter's state.
            if let Some(hz) = low_hz {
                let new = self.cascade(f64::from(hz[i]), false);
                for (section, new) in self.low.iter_mut().zip(new) {
                    section.retune(new);
                }
            }
            if let Some(hz) = high_hz {
                let new = self.cascade(f64::from(hz[i]), true);
                for (section, new) in self.high.iter_mut().zip(new) {
                    section.retune(new);
                }
            }
            let x = f64::from(x);
            let l = Self::run(&mut self.low, stages, x);
            let h = Self::run(&mut self.high, stages, x);
            low.data[i] = l as f32;
            high.data[i] = h as f32;
            mid.data[i] = (x - l - h) as f32;
        }
    }

    fn reset(&mut self) {
        for section in self.low.iter_mut().chain(&mut self.high) {
            section.reset();
        }
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        // The low band settles slowest: allow ten periods of the low crossover.
        let settle_samples =
            10.0 * self.slope.stages() as f64 / self.low_cycles.max(f64::MIN_POSITIVE);
        ((settle_samples / ctx.samples_per_frame() as f64).ceil() as u32).max(1)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use crate::testing::{node, process};

    /// Peak amplitude of each band for a sine of `hz` at a 48 kHz rate, after settling.
    fn band_levels(hz: f64) -> Vec<f32> {
        band_levels_with("{}", hz)
    }

    fn band_levels_with(params: &str, hz: f64) -> Vec<f32> {
        let len = 9600;
        let mut node = node("three_band", params, len, 48_000.0, &[true]);
        let sine: Vec<f32> = (0..len)
            .map(|i| (TAU * hz * i as f64 / 48_000.0).sin() as f32)
            .collect();
        process(node.as_mut(), 3, &[sine])
            .iter()
            .map(|band| band[len / 2..].iter().fold(0.0f32, |m, &x| m.max(x.abs())))
            .collect()
    }

    #[test]
    fn a_low_tone_lands_in_the_low_band() {
        let [low, mid, high] = band_levels(30.0)[..] else {
            unreachable!()
        };
        assert!(low > 0.9 && mid < 0.2 && high < 0.01, "{low} {mid} {high}");
    }

    #[test]
    fn a_high_tone_lands_in_the_high_band() {
        let [low, mid, high] = band_levels(12_000.0)[..] else {
            unreachable!()
        };
        // The mid band is the remainder, so the filters' phase shift leaves some of the tone in it.
        assert!(high > 0.9 && mid < 0.5 && low < 0.01, "{low} {mid} {high}");
    }

    #[test]
    fn a_tone_between_the_crossovers_lands_in_the_mid_band() {
        let [low, mid, high] = band_levels(1000.0)[..] else {
            unreachable!()
        };
        assert!(mid > 0.8 && low < 0.1 && high < 0.1, "{low} {mid} {high}");
    }

    #[test]
    fn a_steeper_slope_keeps_more_of_a_tone_out_of_the_far_band() {
        let low_leak =
            |slope: &str| band_levels_with(&format!(r#"{{ "slope": "{slope}" }}"#), 2000.0)[0];
        assert!(low_leak("48") < low_leak("12") / 10.0);
    }

    #[test]
    fn crossovers_must_be_ordered() {
        let params = serde_json::from_str(r#"{ "low_hz": 5000, "high_hz": 100 }"#).unwrap();
        let registry = crate::Registry::shared();
        assert!(registry.create("three_band", &params).unwrap().is_err());
    }
}
