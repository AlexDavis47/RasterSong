use super::{LengthUnit, MAX_WARMUP_FRAMES};
use crate::dsp::DelayLine;
use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// A delay line whose time can be modulated: `time + depth × modulation`, in rows or frames.
/// Delaying by a fraction of a row and modulating it with a bass line bends rows into waves.
#[derive(Debug)]
pub struct Delay {
    time: f64,
    depth: f64,
    unit: LengthUnit,
    feedback: f32,
    mix: f32,
    /// Samples per unit, set in `prepare`.
    unit_samples: f64,
    line: DelayLine,
}

impl Delay {
    pub const SPEC: NodeSpec = NodeSpec::new("Delay", Category::Effect)
        .describe("Delays the signal by rows or frames; modulating the time bends rows into waves")
        .params(Self::PARAMS)
        .per_channel();

    pub const PARAMS: &[ParamSpec] = &[
        ParamSpec::number(
            "time",
            "Time",
            1.0,
            0.0,
            1000.0,
            "Delay length, in rows or frames",
        ),
        ParamSpec::number(
            "depth",
            "Depth",
            0.0,
            -1000.0,
            1000.0,
            "How far the modulation input moves the delay time, in rows or frames per unit",
        ),
        ParamSpec::choice(
            "unit",
            "Unit",
            &["rows", "frames"],
            "rows",
            "Unit for time and depth",
        ),
        ParamSpec::number(
            "feedback",
            "Feedback",
            0.0,
            0.0,
            0.99,
            "How much of the delayed signal is fed back in",
        ),
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
            depth: params.number("depth")?,
            unit: LengthUnit::read(params)?,
            feedback: params.number("feedback")? as f32,
            mix: params.number("mix")? as f32,
            unit_samples: 0.0,
            line: DelayLine::default(),
        })
    }

    fn max_delay_samples(&self) -> f64 {
        (self.time + self.depth.abs()) * self.unit_samples
    }
}

impl Node for Delay {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] =
            &[InputSpec::required("in"), InputSpec::optional("modulation")];
        INPUTS
    }

    fn prepare(&mut self, ctx: &PrepareContext) {
        self.unit_samples = self.unit.samples(ctx);
        self.line = DelayLine::new(self.max_delay_samples().ceil() as usize + 1);
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let (input, modulation) = (&inputs[0].data, &inputs[1].data);
        // With feedback, the newest sample can't be read before it is written, so the delay is at
        // least one sample.
        let min_delay = if self.feedback > 0.0 { 1.0 } else { 0.0 };
        for ((out, &x), &m) in outputs[0].data.iter_mut().zip(input).zip(modulation) {
            let delay =
                ((self.time + self.depth * f64::from(m)) * self.unit_samples).max(min_delay);
            let delayed = if self.feedback > 0.0 {
                let delayed = self.line.read(delay - 1.0);
                self.line.push(x + self.feedback * delayed);
                delayed
            } else {
                self.line.push(x);
                self.line.read(delay)
            };
            *out = x + (delayed - x) * self.mix;
        }
    }

    fn reset(&mut self) {
        self.line.reset();
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        let delay_frames = self.max_delay_samples() / ctx.samples_per_frame() as f64;
        // Feedback repeats every delay period; count periods until it has decayed by 60 dB.
        let repeats = if self.feedback > 0.0 {
            (0.001f64.ln() / f64::from(self.feedback).ln()).ceil()
        } else {
            1.0
        };
        ((delay_frames * repeats).ceil() as u32).min(MAX_WARMUP_FRAMES)
    }
}
