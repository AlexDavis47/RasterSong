use super::MAX_WARMUP_FRAMES;
use crate::dsp::Biquad;
use crate::nodes::{Category, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, PortHint, PrepareContext, ProcessContext, Signal};

/// Splits a signal into low, mid and high bands. Crossovers are in Hz of the input signal's own
/// sample rate (for an audio input, ordinary Hz). Mid is what remains after removing low and high,
/// so the three bands always add back up to the input.
#[derive(Debug)]
pub struct ThreeBand {
    low_hz: f64,
    high_hz: f64,
    low: Biquad,
    high: Biquad,
}

impl ThreeBand {
    pub const SPEC: NodeSpec = NodeSpec::new("Three-Band Split", Category::Effect)
        .describe("Low, mid and high frequency bands that add back up to the input")
        .params(Self::PARAMS)
        .per_channel();

    pub const PARAMS: &[ParamSpec] = &[
        ParamSpec::number(
            "low_hz",
            "Low / mid",
            250.0,
            1.0,
            100_000.0,
            "Crossover between the low and mid bands, in Hz",
        )
        .unit("Hz")
        .fixed()
        .limits(0.001, 1e9),
        ParamSpec::number(
            "high_hz",
            "Mid / high",
            4000.0,
            1.0,
            100_000.0,
            "Crossover between the mid and high bands, in Hz",
        )
        .unit("Hz")
        .fixed()
        .limits(0.001, 1e9),
    ];

    pub fn new(params: &Params) -> Result<Self, String> {
        let low_hz = params.number("low_hz")?;
        let high_hz = params.number("high_hz")?;
        if low_hz >= high_hz {
            return Err(format!(
                "`low_hz` ({low_hz}) must be below `high_hz` ({high_hz})"
            ));
        }
        Ok(Self {
            low_hz,
            high_hz,
            low: Biquad::default(),
            high: Biquad::default(),
        })
    }
}

impl Node for ThreeBand {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn outputs(&self) -> &'static [&'static str] {
        &["low", "mid", "high"]
    }

    fn output_hints(&self) -> &'static [PortHint] {
        &[PortHint::Low, PortHint::Mid, PortHint::High]
    }

    fn prepare(&mut self, ctx: &PrepareContext) {
        let rate = ctx.sample_rate();
        self.low = Biquad::butterworth(self.low_hz / rate, false);
        self.high = Biquad::butterworth(self.high_hz / rate, true);
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
        let settle_samples = 10.0 * ctx.sample_rate() / self.low_hz;
        ((settle_samples / ctx.samples_per_frame() as f64).ceil() as u32)
            .clamp(1, MAX_WARMUP_FRAMES)
    }
}
