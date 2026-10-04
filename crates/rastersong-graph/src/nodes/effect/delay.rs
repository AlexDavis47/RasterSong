use super::{LengthUnit, MAX_WARMUP_FRAMES};
use crate::dsp::DelayLine;
use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// A delay line, in rows or frames. Delaying by a fraction of a row and modulating the time with
/// a bass line bends rows into waves.
#[derive(Debug)]
pub struct Delay {
    time: f64,
    unit: LengthUnit,
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

impl Delay {
    const TIME: usize = 0;
    const FEEDBACK: usize = 2;
    const MIX: usize = 3;

    pub const SPEC: NodeSpec = NodeSpec::new("Delay", Category::Effect)
        .describe("Delays the signal by rows or frames; modulating the time bends rows into waves")
        .params(Self::PARAMS)
        .per_channel();

    pub const PARAMS: &[ParamSpec] = &[
        ParamSpec::number(
            "time",
            "Time",
            0.05,
            0.001,
            100.0,
            "Delay length, in rows or frames. Small fractions of a row give the finest waves",
        )
        .exposed()
        .limits(0.0, 1000.0),
        ParamSpec::choice(
            "unit",
            "Unit",
            &["rows", "frames"],
            "rows",
            "Unit for the time",
        ),
        ParamSpec::number(
            "feedback",
            "Feedback",
            0.0,
            0.0,
            0.99,
            "How much of the delayed signal is fed back in",
        )
        .exposed(),
        ParamSpec::number(
            "mix",
            "Mix",
            1.0,
            0.0,
            1.0,
            "0 is the dry input, 1 is only the delayed signal",
        ),
    ];

    pub fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            time: params.number("time")?,
            unit: LengthUnit::read(params)?,
            feedback: params.number("feedback")? as f32,
            mix: params.number("mix")? as f32,
            unit_samples: 0.0,
            max_delay: 0.0,
            max_feedback: 0.0,
            feeds_back: false,
            line: DelayLine::default(),
        })
    }
}

impl Node for Delay {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn prepare(&mut self, ctx: &PrepareContext) {
        self.unit_samples = self.unit.samples(ctx);
        self.max_delay = ctx.param_max(Self::TIME, self.time) * self.unit_samples;
        self.max_feedback = ctx.param_max(Self::FEEDBACK, f64::from(self.feedback)) as f32;
        self.feeds_back = self.max_feedback > 0.0;
        self.line = DelayLine::new(self.max_delay.ceil() as usize + 1);
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let time = ctx.param(Self::TIME);
        let (feedback, mix) = (ctx.param(Self::FEEDBACK), ctx.param(Self::MIX));
        // With feedback, the newest sample can't be read before it is written, so the delay is at
        // least one sample.
        let min_delay = if self.feeds_back { 1.0 } else { 0.0 };
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let time = time.map_or(self.time, |t| f64::from(t[i]));
            let delay = (time * self.unit_samples).max(min_delay);
            let delayed = if self.feeds_back {
                let feedback = feedback.map_or(self.feedback, |f| f[i]);
                let delayed = self.line.read(delay - 1.0);
                self.line.push(x + feedback * delayed);
                delayed
            } else {
                self.line.push(x);
                self.line.read(delay)
            };
            let mix = mix.map_or(self.mix, |m| m[i]);
            *out = x + (delayed - x) * mix;
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
    use super::*;

    #[test]
    fn parameter_indices_match_the_specs() {
        let names = [
            (Delay::TIME, "time"),
            (Delay::FEEDBACK, "feedback"),
            (Delay::MIX, "mix"),
        ];
        for (index, name) in names {
            assert_eq!(Delay::PARAMS[index].name, name);
        }
    }
}
