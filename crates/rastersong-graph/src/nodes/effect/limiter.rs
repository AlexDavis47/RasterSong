use crate::dsp::{db_to_gain, smoothing_coefficient};
use crate::nodes::support::settle_frames;
use crate::nodes::{Category, NodeKind, NodeSpec, Unit};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// A peak limiter: the gain drops at once so no sample passes the ceiling, then recovers over the
/// release time. It reacts instantly and so has no latency, at the price of a hard edge where it
/// engages; use the Compressor for a gentler squeeze.
#[derive(Debug)]
pub struct Limiter {
    /// The ceiling as a gain.
    ceiling: f32,
    release: f64,
    unit: Unit,
    /// Release smoothing coefficient, set in `prepare`.
    coefficient: f32,
    slowest: f64,
    gain: f32,
}

params! { Limiter {
    CEILING: ParamSpec::number(
        "ceiling",
        "Ceiling",
        -6.0,
        -48.0,
        0.0,
        "The loudest any sample may get: 0 dB is full scale, 1.0",
    )
    .unit("dB")
    .exposed()
    .limits(-120.0, 24.0),
    RELEASE: ParamSpec::number(
        "release",
        "Release",
        50.0,
        0.0,
        1000.0,
        "How slowly the gain recovers after a peak; longer is smoother",
    )
    .fixed()
    .limits(0.0, 1e6),
    UNIT: Unit::time_param("ms", "Unit for the release"),
} }

impl NodeKind for Limiter {
    const KIND: &'static str = "limiter";
    const SPEC: NodeSpec = NodeSpec::new("Limiter", Category::Effect)
        .describe("Stops the signal from passing a ceiling by pulling the gain down")
        .params(Self::PARAMS)
        .per_channel()
        .expects(crate::Range::Bipolar);
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "ceiling": -12, "release": 10 }"#,
        r#"{ "ceiling": 0, "release": 0 }"#,
        r#"{ "ceiling": -3, "release": 0.5, "unit": "row" }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            ceiling: db_to_gain(params.number_at(Self::CEILING)?) as f32,
            release: params.number_at(Self::RELEASE)?,
            unit: params.choice_as(Self::UNIT)?,
            coefficient: 0.0,
            slowest: 0.0,
            gain: 1.0,
        })
    }
}

impl Node for Limiter {
    fn prepare(&mut self, ctx: &PrepareContext) {
        let release = self.release * self.unit.samples(ctx);
        self.coefficient = smoothing_coefficient(release) as f32;
        // About 7 time constants to recover to within 0.1%.
        self.slowest = 7.0 * release;
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let ceiling = ctx.value(Self::CEILING, f64::from(self.ceiling));
        let mut gain = self.gain;
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let ceiling = ceiling.at_with(i, |db| db_to_gain(f64::from(db)) as f32);
            let peak = x.abs();
            let allowed = if peak > ceiling { ceiling / peak } else { 1.0 };
            // Recover toward 1, but never above what this sample allows.
            let recovered = 1.0 + (gain - 1.0) * self.coefficient;
            gain = recovered.min(allowed);
            *out = x * gain;
        }
        self.gain = gain;
    }

    fn reset(&mut self) {
        self.gain = 1.0;
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        settle_frames(self.slowest, ctx)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn limit(params: &str, input: &[f32]) -> Vec<f32> {
        let mut n = node("limiter", params, input.len(), input.len() as f64, &[true]);
        process_one(n.as_mut(), &[input.to_vec()])
    }

    #[test]
    fn nothing_passes_the_ceiling() {
        let input: Vec<f32> = (0..64).map(|i| ((i * 7) % 13) as f32 / 6.0 - 1.0).collect();
        let out = limit(r#"{ "ceiling": -6, "release": 5 }"#, &input);
        let ceiling = 10f32.powf(-6.0 / 20.0);
        assert!(out.iter().all(|x| x.abs() <= ceiling + 1e-6));
    }

    #[test]
    fn quiet_signals_are_untouched() {
        let input = [0.1, -0.2, 0.05];
        assert_eq!(limit(r#"{ "ceiling": 0 }"#, &input), input);
    }

    #[test]
    fn the_gain_recovers_after_a_peak() {
        // Release of 2 samples: after the peak the quiet tail climbs back toward full level.
        let out = limit(
            r#"{ "ceiling": -6, "release": 2, "unit": "row" }"#,
            &[1.0, 0.1, 0.1, 0.1],
        );
        // One row is the whole 4 sample block here, so the release is 8 samples.
        assert!(out[1] < 0.1 && out[3] > out[1]);
    }
}
