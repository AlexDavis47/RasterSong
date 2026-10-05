use crate::dsp::Biquad;
use crate::nodes::support::MAX_WARMUP_FRAMES;
use crate::nodes::{Category, FreqUnit, NodeKind, NodeSpec};
use crate::{
    Node, OutputSpec, ParamSpec, Params, Part, PrepareContext, ProcessContext, Signal, TagRule,
};

/// Splits a signal into low, mid and high bands. Crossovers are in Hz of the input signal's own
/// sample rate (for an audio input, ordinary Hz) unless `unit` says otherwise. Mid is what remains after removing low and high,
/// so the three bands always add back up to the input.
#[derive(Debug)]
pub struct ThreeBand {
    low_hz: f64,
    high_hz: f64,
    unit: FreqUnit,
    /// The low crossover in cycles per sample, set in `prepare`.
    low_cycles: f64,
    low: Biquad,
    high: Biquad,
}

params! { ThreeBand {
    LOW_HZ: ParamSpec::number(
        "low_hz",
        "Low / mid",
        250.0,
        1.0,
        100_000.0,
        "Crossover between the low and mid bands",
    )
    .fixed()
    .limits(0.001, 1e9),
    HIGH_HZ: ParamSpec::number(
        "high_hz",
        "Mid / high",
        4000.0,
        1.0,
        100_000.0,
        "Crossover between the mid and high bands",
    )
    .fixed()
    .limits(0.001, 1e9),
    UNIT: FreqUnit::param("Hertz", "Unit for the crossovers"),
} }

impl NodeKind for ThreeBand {
    const KIND: &'static str = "three_band";
    const SPEC: NodeSpec = NodeSpec::new("Three-Band Split", Category::Effect)
        .describe("Low, mid and high frequency bands that add back up to the input")
        .params(Self::PARAMS)
        .outputs(&[
            OutputSpec::new("low", "Everything below the low crossover").tag(TagRule::INHERIT.part(Part::Low)),
            OutputSpec::new("mid", "What is left between the crossovers").tag(TagRule::INHERIT.part(Part::Mid)),
            OutputSpec::new("high", "Everything above the high crossover").tag(TagRule::INHERIT.part(Part::High)),
        ])
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "low_hz": 300, "high_hz": 3000 }"#,
        r#"{ "low_hz": 2, "high_hz": 9, "unit": "Beat" }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        let low_hz = params.number_at(Self::LOW_HZ)?;
        let high_hz = params.number_at(Self::HIGH_HZ)?;
        if low_hz >= high_hz {
            return Err(format!(
                "`low_hz` ({low_hz}) must be below `high_hz` ({high_hz})"
            ));
        }
        Ok(Self {
            low_hz,
            high_hz,
            unit: params.choice_as(Self::UNIT)?,
            low_cycles: 0.0,
            low: Biquad::default(),
            high: Biquad::default(),
        })
    }
}

impl Node for ThreeBand {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.low_cycles = self.unit.per_sample(self.low_hz, ctx);
        self.low = Biquad::butterworth(self.low_cycles, false);
        self.high = Biquad::butterworth(self.unit.per_sample(self.high_hz, ctx), true);
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let [low, mid, high] = outputs else {
            unreachable!()
        };
        for (i, &x) in inputs[0].data.iter().enumerate() {
            let x = f64::from(x);
            let l = self.low.process(x);
            let h = self.high.process(x);
            low.data[i] = l as f32;
            high.data[i] = h as f32;
            mid.data[i] = (x - l - h) as f32;
        }
    }

    fn reset(&mut self) {
        self.low.reset();
        self.high.reset();
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        // The low band settles slowest: allow ten periods of the low crossover.
        let settle_samples = 10.0 / self.low_cycles.max(f64::MIN_POSITIVE);
        ((settle_samples / ctx.samples_per_frame() as f64).ceil() as u32)
            .clamp(1, MAX_WARMUP_FRAMES)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use crate::testing::{node, process};

    /// Peak amplitude of each band for a sine of `hz` at a 48 kHz rate, after settling.
    fn band_levels(hz: f64) -> Vec<f32> {
        let len = 9600;
        let mut node = node("three_band", "{}", len, 48_000.0, &[true]);
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
    fn crossovers_must_be_ordered() {
        let params = serde_json::from_str(r#"{ "low_hz": 5000, "high_hz": 100 }"#).unwrap();
        let registry = crate::Registry::shared();
        assert!(registry.create("three_band", &params).unwrap().is_err());
    }
}
