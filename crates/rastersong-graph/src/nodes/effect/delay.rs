use crate::dsp::{DelayLine, mix};
use crate::nodes::support::MAX_WARMUP_FRAMES;
use crate::nodes::{Category, NodeKind, NodeSpec, TimeUnit};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// A delay line, in rows or frames. Delaying by a fraction of a row and modulating the time with
/// a bass line bends rows into waves.
#[derive(Debug)]
pub struct Delay {
    time: f64,
    unit: TimeUnit,
    feedback: f32,
    mix: f32,
    /// Set in `prepare`: samples per unit, the largest delay and feedback modulation can reach,
    /// and whether the feedback path runs.
    unit_samples: f64,
    max_delay: f64,
    max_feedback: f32,
    feeds_back: bool,
    line: DelayLine,
}

params! { Delay {
    TIME: ParamSpec::number(
        "time",
        "Time",
        0.05,
        0.0,
        100.0,
        "Delay length, in rows or frames. Small fractions of a row give the finest waves",
    )
    .exposed()
    .limits(0.0, 1000.0),
    UNIT: TimeUnit::param("rows", "Unit for the time"),
    FEEDBACK: ParamSpec::number(
        "feedback",
        "Feedback",
        0.0,
        0.0,
        0.99,
        "How much of the delayed signal is fed back in",
    )
    .exposed(),
    MIX: ParamSpec::number(
        "mix",
        "Mix",
        1.0,
        0.0,
        1.0,
        "0 is the dry input, 1 is only the delayed signal",
    ),
} }

impl NodeKind for Delay {
    const KIND: &'static str = "delay";
    const SPEC: NodeSpec = NodeSpec::new("Delay", Category::Effect)
        .describe("Delays the signal by rows or frames; modulating the time bends rows into waves")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "time": 1.5 }"#,
        r#"{ "time": 0.5, "unit": "frames" }"#,
        r#"{ "time": 2.25, "feedback": 0.6, "mix": 0.7 }"#,
        r#"{ "time": 0.3, "feedback": 0.5 }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "time": 1.5, "feedback": 0.5, "mix": 0.5 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            time: params.number_at(Self::TIME)?,
            unit: params.choice_as(Self::UNIT)?,
            feedback: params.float_at(Self::FEEDBACK)?,
            mix: params.float_at(Self::MIX)?,
            unit_samples: 0.0,
            max_delay: 0.0,
            max_feedback: 0.0,
            feeds_back: false,
            line: DelayLine::default(),
        })
    }
}

impl Node for Delay {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.unit_samples = self.unit.samples(ctx);
        self.max_delay = ctx.param_max(Self::TIME, self.time) * self.unit_samples;
        self.max_feedback = ctx.param_max(Self::FEEDBACK, f64::from(self.feedback)) as f32;
        self.feeds_back = self.max_feedback > 0.0;
        self.line = DelayLine::new(self.max_delay.ceil() as usize + 1);
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let time = ctx.value(Self::TIME, self.time);
        let feedback = ctx.value(Self::FEEDBACK, f64::from(self.feedback));
        let amount = ctx.value(Self::MIX, f64::from(self.mix));
        // With feedback, the newest sample can't be read before it is written, so the delay is at
        // least one sample.
        let min_delay = if self.feeds_back { 1.0 } else { 0.0 };
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let delay = (time.at64(i) * self.unit_samples).max(min_delay);
            let delayed = if self.feeds_back {
                let delayed = self.line.read(delay - 1.0);
                self.line.push(x + feedback.at(i) * delayed);
                delayed
            } else {
                self.line.push(x);
                self.line.read(delay)
            };
            *out = mix(x, delayed, amount.at(i));
        }
    }

    fn reset(&mut self) {
        self.line.reset();
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        let delay_frames = self.max_delay / ctx.samples_per_frame() as f64;
        // Feedback repeats every delay period; count periods until it has decayed by 60 dB.
        let repeats = if self.max_feedback > 0.0 {
            (0.001f64.ln() / f64::from(self.max_feedback).ln()).ceil()
        } else {
            1.0
        };
        ((delay_frames * repeats).ceil() as u32).min(MAX_WARMUP_FRAMES)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    #[test]
    fn delays_by_whole_samples() {
        // One row is 8 samples here, so a time of 0.25 rows is 2 samples.
        let mut delay = node("delay", r#"{ "time": 0.25 }"#, 8, 8.0, &[true]);
        let input: Vec<f32> = (1..=8).map(|i| i as f32).collect();
        let out = process_one(delay.as_mut(), &[input]);
        assert_eq!(out, [0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn feedback_repeats_the_input() {
        let mut delay = node(
            "delay",
            r#"{ "time": 0.25, "feedback": 0.5 }"#,
            8,
            8.0,
            &[true],
        );
        let mut input = vec![0.0; 8];
        input[0] = 1.0;
        let out = process_one(delay.as_mut(), &[input]);
        assert_eq!(out[2], 1.0);
        assert_eq!(out[4], 0.5);
        assert_eq!(out[6], 0.25);
    }

    #[test]
    fn mix_zero_is_the_dry_signal() {
        let mut delay = node("delay", r#"{ "time": 0.5, "mix": 0 }"#, 4, 4.0, &[true]);
        let input = vec![0.1, 0.2, 0.3, 0.4];
        assert_eq!(
            process_one(delay.as_mut(), std::slice::from_ref(&input)),
            input
        );
    }
}
