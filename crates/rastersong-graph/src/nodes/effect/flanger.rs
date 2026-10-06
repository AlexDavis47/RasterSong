use crate::dsp::{DelayLine, mix};
use crate::nodes::{Category, NodeKind, NodeSpec, TimeUnit};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// Flanger: a very short delay with feedback, mixed with the dry signal, which carves a comb of
/// notches into the spectrum. It has no oscillator of its own: wire an Oscillator into `time` to
/// sweep the notches. Negative feedback moves the notches halfway between the positive ones.
#[derive(Debug)]
pub struct Flanger {
    time: f64,
    unit: TimeUnit,
    feedback: f32,
    mix: f32,
    /// Set in `prepare`: samples per unit, the longest delay, the strongest feedback.
    unit_samples: f64,
    max_delay: f64,
    max_feedback: f32,
    line: DelayLine,
}

params! { Flanger {
    TIME: ParamSpec::number(
        "time",
        "Time",
        2.0,
        0.0,
        10.0,
        "Delay length; wire an oscillator in here to sweep the comb",
    )
    .exposed()
    .limits(0.0, 1000.0),
    UNIT: TimeUnit::param("ms", "Unit for the time"),
    FEEDBACK: ParamSpec::number(
        "feedback",
        "Feedback",
        0.5,
        -0.95,
        0.95,
        "How much of the delayed signal is fed back in; negative flips its sign",
    ),
    MIX: ParamSpec::number(
        "mix",
        "Mix",
        0.5,
        0.0,
        1.0,
        "0 is the dry input, 1 is only the delayed signal",
    ),
} }

impl NodeKind for Flanger {
    const KIND: &'static str = "flanger";
    const SPEC: NodeSpec = NodeSpec::new("Flanger", Category::Effect)
        .describe(
            "A short delay with feedback that combs the signal; modulate the time to sweep it",
        )
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "time": 0.25, "unit": "rows" }"#,
        r#"{ "time": 0.5, "unit": "rows", "feedback": -0.8, "mix": 0.7 }"#,
        r#"{ "time": 3, "feedback": 0 }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            time: params.number_at(Self::TIME)?,
            unit: params.choice_as(Self::UNIT)?,
            feedback: params.float_at(Self::FEEDBACK)?,
            mix: params.float_at(Self::MIX)?,
            unit_samples: 0.0,
            max_delay: 0.0,
            max_feedback: 0.0,
            line: DelayLine::default(),
        })
    }
}

impl Node for Flanger {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.unit_samples = self.unit.samples(ctx);
        self.max_delay = ctx.param_max(Self::TIME, self.time) * self.unit_samples;
        self.max_feedback = ctx
            .param_max(Self::FEEDBACK, f64::from(self.feedback))
            .abs()
            .max(
                ctx.param_min(Self::FEEDBACK, f64::from(self.feedback))
                    .abs(),
            ) as f32;
        self.line = DelayLine::new(self.max_delay.ceil() as usize + 1);
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let time = ctx.value(Self::TIME, self.time);
        let feedback = ctx.value(Self::FEEDBACK, f64::from(self.feedback));
        let amount = ctx.value(Self::MIX, f64::from(self.mix));
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            // The newest sample can't be read before it is written, so the delay is at least one.
            let delay = (time.at64(i) * self.unit_samples).max(1.0);
            let delayed = self.line.read(delay - 1.0);
            self.line.push(x + feedback.at(i) * delayed);
            *out = mix(x, delayed, amount.at(i));
        }
    }

    fn reset(&mut self) {
        self.line.reset();
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        let delay_frames = self.max_delay.max(1.0) / ctx.samples_per_frame() as f64;
        // Count delay periods until the feedback has decayed by 60 dB.
        let repeats = if self.max_feedback > 0.0 {
            (0.001f64.ln() / f64::from(self.max_feedback).ln()).ceil()
        } else {
            1.0
        };
        ((delay_frames * repeats).ceil() as u32).max(1)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    #[test]
    fn feedback_zero_mix_one_is_a_one_sample_minimum_delay() {
        // A quarter row of 8 samples is 2 samples.
        let mut n = node(
            "flanger",
            r#"{ "time": 0.25, "unit": "rows", "feedback": 0, "mix": 1 }"#,
            8,
            8.0,
            &[true],
        );
        let input: Vec<f32> = (1..=8).map(|i| i as f32).collect();
        let out = process_one(n.as_mut(), &[input]);
        assert_eq!(out, [0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn negative_feedback_flips_the_echo() {
        let mut n = node(
            "flanger",
            r#"{ "time": 0.25, "unit": "rows", "feedback": -0.5, "mix": 1 }"#,
            8,
            8.0,
            &[true],
        );
        let mut input = vec![0.0; 8];
        input[0] = 1.0;
        let out = process_one(n.as_mut(), &[input]);
        assert_eq!(out[2], 1.0);
        assert_eq!(out[4], -0.5);
        assert_eq!(out[6], 0.25);
    }

    #[test]
    fn mix_zero_is_the_dry_signal() {
        let mut n = node("flanger", r#"{ "mix": 0 }"#, 4, 4.0, &[true]);
        let input = vec![0.1, 0.2, 0.3, 0.4];
        assert_eq!(process_one(n.as_mut(), std::slice::from_ref(&input)), input);
    }
}
