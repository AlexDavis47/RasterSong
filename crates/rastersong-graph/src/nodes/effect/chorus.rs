use crate::dsp::{DelayLine, mix};
use crate::nodes::support::settle_frames;
use crate::nodes::{Category, NodeKind, NodeSpec, Unit};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// Chorus: the signal mixed with a few slightly delayed copies. It has no oscillator of its own:
/// wire an Oscillator into `time` to make the copies drift, which is what turns doubling into
/// chorus.
#[derive(Debug)]
pub struct Chorus {
    time: f64,
    unit: Unit,
    voices: usize,
    spread: f64,
    mix: f32,
    /// Samples per unit and the longest delay any voice can reach, set in `prepare`.
    unit_samples: f64,
    max_delay: f64,
    line: DelayLine,
}

params! { Chorus {
    TIME: ParamSpec::number(
        "time",
        "Time",
        20.0,
        0.0,
        50.0,
        "Delay of the copies; wire an oscillator in here to make them drift",
    )
    .exposed()
    .limits(0.0, 1000.0),
    UNIT: Unit::time_param("ms", "Unit for the time"),
    VOICES: ParamSpec::number("voices", "Voices", 2.0, 1.0, 4.0, "How many delayed copies are mixed in")
        .fixed(),
    SPREAD: ParamSpec::number(
        "spread",
        "Spread",
        0.3,
        0.0,
        0.6,
        "How far apart the copies' delays are, as a fraction of the time",
    )
    .fixed(),
    MIX: ParamSpec::number(
        "mix",
        "Mix",
        0.5,
        0.0,
        1.0,
        "0 is the dry input, 1 is only the copies",
    ),
} }

impl NodeKind for Chorus {
    const KIND: &'static str = "chorus";
    const SPEC: NodeSpec = NodeSpec::new("Chorus", Category::Effect)
        .describe("Thickens the signal with delayed copies; modulate the time to make them drift")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "time": 0.5, "unit": "row", "voices": 3 }"#,
        r#"{ "time": 12, "voices": 4, "spread": 0.6, "mix": 0.8 }"#,
        r#"{ "time": 1, "unit": "row", "voices": 1 }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "voices": 3 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            time: params.number_at(Self::TIME)?,
            unit: params.choice_as(Self::UNIT)?,
            voices: params.number_at(Self::VOICES)?.round().clamp(1.0, 4.0) as usize,
            spread: params.number_at(Self::SPREAD)?,
            mix: params.float_at(Self::MIX)?,
            unit_samples: 0.0,
            max_delay: 0.0,
            line: DelayLine::default(),
        })
    }
}

impl Chorus {
    /// The factor voice `k` multiplies the time by: evenly spread around 1.
    fn factor(&self, k: usize) -> f64 {
        1.0 + self.spread * (k as f64 - (self.voices - 1) as f64 / 2.0)
    }
}

impl Node for Chorus {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.unit_samples = self.unit.samples(ctx);
        let widest = self.factor(self.voices - 1).max(1.0);
        self.max_delay = ctx.param_max(Self::TIME, self.time) * self.unit_samples * widest;
        self.line = DelayLine::new(self.max_delay.ceil() as usize + 1);
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let time = ctx.value(Self::TIME, self.time);
        let amount = ctx.value(Self::MIX, f64::from(self.mix));
        let weight = 1.0 / self.voices as f32;
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            self.line.push(x);
            let base = time.at64(i) * self.unit_samples;
            let wet: f32 = (0..self.voices)
                .map(|k| self.line.read(base * self.factor(k)))
                .sum::<f32>()
                * weight;
            *out = mix(x, wet, amount.at(i));
        }
    }

    fn reset(&mut self) {
        self.line.reset();
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        settle_frames(self.max_delay, ctx)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    #[test]
    fn mix_zero_is_the_dry_signal() {
        let mut n = node("chorus", r#"{ "mix": 0 }"#, 4, 4.0, &[true]);
        let input = vec![0.1, 0.2, 0.3, 0.4];
        assert_eq!(process_one(n.as_mut(), std::slice::from_ref(&input)), input);
    }

    #[test]
    fn a_single_voice_is_a_plain_delay() {
        // One row is the 8 sample block, so a quarter row is 2 samples.
        let mut n = node(
            "chorus",
            r#"{ "time": 0.25, "unit": "row", "voices": 1, "mix": 1 }"#,
            8,
            8.0,
            &[true],
        );
        let input: Vec<f32> = (1..=8).map(|i| i as f32).collect();
        let out = process_one(n.as_mut(), &[input]);
        assert_eq!(out, [0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn voices_average_their_delays() {
        // Two voices at 1 and 3 samples (time 2, spread 0.5): the average of both delays.
        let mut n = node(
            "chorus",
            r#"{ "time": 0.25, "unit": "row", "voices": 2, "spread": 0.5, "mix": 1 }"#,
            8,
            8.0,
            &[true],
        );
        let input: Vec<f32> = (1..=8).map(|i| i as f32).collect();
        let out = process_one(n.as_mut(), &[input]);
        // Sample 5 (value 6) averages the values 1 and 3 samples back: 5 and 3.
        assert_eq!(out[5], 4.0);
    }
}
