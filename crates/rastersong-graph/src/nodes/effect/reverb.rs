use crate::dsp::{DelayLine, mix};
use crate::nodes::support::{UNBOUNDED_WARMUP, settle_frames};
use crate::nodes::{Category, NodeKind, NodeSpec, Unit};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// Delay lengths of Freeverb's eight parallel comb filters and four series all-pass filters, in
/// samples at 44.1 kHz. They are scaled by the signal's sample rate.
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
/// Freeverb's input gain, which keeps the summed combs near the input's level.
const INPUT_GAIN: f32 = 0.015;
const ALLPASS_FEEDBACK: f32 = 0.5;

/// A feedback comb with a low-passed (damped) feedback path.
#[derive(Debug, Clone, Default)]
struct Comb {
    buffer: Vec<f32>,
    position: usize,
    damped: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)],
            position: 0,
            damped: 0.0,
        }
    }

    fn process(&mut self, x: f32, feedback: f32, damping: f32) -> f32 {
        let out = self.buffer[self.position];
        self.damped = out * (1.0 - damping) + self.damped * damping;
        self.buffer[self.position] = x + self.damped * feedback;
        self.position = (self.position + 1) % self.buffer.len();
        out
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.position = 0;
        self.damped = 0.0;
    }
}

/// A Schroeder all-pass diffuser.
#[derive(Debug, Clone, Default)]
struct Allpass {
    buffer: Vec<f32>,
    position: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len.max(1)],
            position: 0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let delayed = self.buffer[self.position];
        self.buffer[self.position] = x + delayed * ALLPASS_FEEDBACK;
        self.position = (self.position + 1) % self.buffer.len();
        delayed - x
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.position = 0;
    }
}

/// A Freeverb-style reverb: eight damped comb filters in parallel into four all-pass diffusers,
/// after an optional pre-delay. The filter lengths scale with the signal's sample rate, so the
/// tail has the same duration at any resolution; on high-resolution video that is a lot of
/// memory (about a frame and a half of samples per comb).
#[derive(Debug)]
pub struct Reverb {
    size: f64,
    damping: f32,
    predelay: f64,
    unit: Unit,
    mix: f32,
    /// Set in `prepare`: samples in one unit, the longest the pre-delay gets, and how long the
    /// tail rings at its longest.
    unit_samples: f64,
    tail_samples: f64,
    predelay_line: DelayLine,
    longest_predelay: f64,
    combs: Vec<Comb>,
    allpasses: Vec<Allpass>,
}

params! { Reverb {
    SIZE: ParamSpec::number("size", "Size", 0.5, 0.0, 1.0, "How long the tail rings: higher is longer"),
    DAMPING: ParamSpec::number("damping", "Damping", 0.5, 0.0, 1.0, "How quickly the tail loses its fast detail: higher is duller"),
    PREDELAY: ParamSpec::number("predelay", "Pre-delay", 0.0, 0.0, 100.0, "Gap before the reverb starts")
        .limits(0.0, 10_000.0),
    UNIT: Unit::time_param("ms", "Unit for the pre-delay"),
    MIX: ParamSpec::mix().exposed(),
} }

impl NodeKind for Reverb {
    const KIND: &'static str = "reverb";
    const SPEC: NodeSpec = NodeSpec::new("Reverb", Category::Effect)
        .describe("A dense decaying wash of echoes")
        .doc("Uses 12 internal delay lines whose lengths follow the signal's sample rate. On video the tail is long in samples, so the node uses a lot of memory and asks the host to render up to 120 frames of warmup before a seek.")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "size": 0.8, "damping": 0.2, "mix": 0.5 }"#,
        r#"{ "size": 0.2, "predelay": 0.5, "unit": "row", "mix": 1 }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "size": 0.7, "mix": 0.4 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            size: params.number_at(Self::SIZE)?,
            damping: params.float_at(Self::DAMPING)?,
            predelay: params.number_at(Self::PREDELAY)?,
            unit: params.choice_as(Self::UNIT)?,
            mix: params.float_at(Self::MIX)?,
            unit_samples: 1.0,
            tail_samples: 0.0,
            predelay_line: DelayLine::default(),
            longest_predelay: 0.0,
            combs: Vec::new(),
            allpasses: Vec::new(),
        })
    }
}

/// The comb filters' feedback for a size from 0 to 1: how long the tail rings.
fn feedback_for(size: f64) -> f32 {
    (0.7 + 0.28 * size.clamp(0.0, 1.0)) as f32
}

impl Node for Reverb {
    fn prepare(&mut self, ctx: &PrepareContext) {
        let scale = ctx.sample_rate() / 44_100.0;
        let scaled = |len: usize| (len as f64 * scale).round() as usize;
        self.combs = COMBS.iter().map(|&len| Comb::new(scaled(len))).collect();
        self.allpasses = ALLPASSES
            .iter()
            .map(|&len| Allpass::new(scaled(len)))
            .collect();
        self.unit_samples = self.unit.samples(ctx);
        // The longest pre-delay and the longest tail the settings (or a signal moving them) allow.
        self.longest_predelay = ctx.param_max(Self::PREDELAY, self.predelay) * self.unit_samples;
        self.predelay_line = DelayLine::new(self.longest_predelay.ceil() as usize + 1);
        // The slowest comb loses 60 dB after ln(0.001) / ln(feedback) trips around its loop.
        let longest = scaled(COMBS[COMBS.len() - 1]) as f64;
        let feedback = feedback_for(ctx.param_max(Self::SIZE, self.size));
        let trips = 0.001f64.ln() / f64::from(feedback).ln();
        self.tail_samples = longest * trips + self.longest_predelay;
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let amount = ctx.value(Self::MIX, f64::from(self.mix));
        let size = ctx.value(Self::SIZE, self.size);
        let damping = ctx.value(Self::DAMPING, f64::from(self.damping));
        let predelay = ctx.value(Self::PREDELAY, self.predelay);
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let fed = if self.longest_predelay > 0.0 {
                self.predelay_line.push(x);
                self.predelay_line
                    .read((predelay.at64(i) * self.unit_samples).clamp(0.0, self.longest_predelay))
            } else {
                x
            };
            let (feedback, damping) = (feedback_for(size.at64(i)), damping.at(i).clamp(0.0, 1.0));
            let input = fed * INPUT_GAIN;
            let mut wet: f32 = self
                .combs
                .iter_mut()
                .map(|c| c.process(input, feedback, damping))
                .sum();
            for allpass in &mut self.allpasses {
                wet = allpass.process(wet);
            }
            *out = mix(x, wet, amount.at(i));
        }
    }

    fn reset(&mut self) {
        self.predelay_line.reset();
        self.combs.iter_mut().for_each(Comb::reset);
        self.allpasses.iter_mut().for_each(Allpass::reset);
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        if self.tail_samples.is_finite() {
            settle_frames(self.tail_samples, ctx)
        } else {
            UNBOUNDED_WARMUP
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    /// The reverb's response to an impulse, at 44.1 kHz so the filter lengths are Freeverb's.
    fn impulse_response(params: &str, len: usize) -> Vec<f32> {
        let mut node = node("reverb", params, len, 44_100.0, &[true]);
        let mut input = vec![0.0; len];
        input[0] = 1.0;
        process_one(node.as_mut(), &[input])
    }

    #[test]
    fn the_first_echo_arrives_at_the_shortest_comb() {
        let out = impulse_response(r#"{ "mix": 1 }"#, 4000);
        // Nothing before the shortest comb (1116 samples) plus the all-pass chain has output.
        assert!(out[..1000].iter().all(|&x| x == 0.0));
        assert!(out[1116..].iter().any(|&x| x != 0.0));
    }

    #[test]
    fn the_tail_decays() {
        let out = impulse_response(r#"{ "mix": 1, "size": 0.2 }"#, 44_100);
        let energy = |range: std::ops::Range<usize>| out[range].iter().map(|x| x * x).sum::<f32>();
        assert!(energy(30_000..44_100) < energy(2_000..16_000) * 0.2);
    }

    #[test]
    fn mix_zero_is_the_dry_signal() {
        let out = impulse_response(r#"{ "mix": 0 }"#, 2000);
        assert_eq!(out[0], 1.0);
        assert!(out[1..].iter().all(|&x| x == 0.0));
    }

    #[test]
    fn predelay_holds_the_reverb_back() {
        // 10 ms at 44.1 kHz is 441 samples before the first comb even sees the impulse.
        let out = impulse_response(r#"{ "mix": 1, "predelay": 10 }"#, 4000);
        assert!(out[..1116 + 441].iter().all(|&x| x == 0.0));
    }
}
