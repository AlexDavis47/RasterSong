use crate::dsp::{db_to_gain, smoothing_coefficient};
use crate::nodes::support::{ms_to_samples, settle_frames};
use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// Range at or below which a closed gate is fully silent.
const SILENT_RANGE: f64 = -80.0;

/// A noise gate: lets the signal through while it (or the sidechain, when connected) is above
/// `threshold`, holds open for `hold` after it drops, then closes down to `range`. Times are
/// milliseconds of the signal's own time.
#[derive(Debug)]
pub struct Gate {
    threshold: f64,
    attack_ms: f64,
    hold_ms: f64,
    release_ms: f64,
    range: f64,
    /// Set in `prepare`.
    threshold_gain: f32,
    closed_gain: f64,
    attack: f64,
    release: f64,
    hold_samples: u64,
    sample_rate: f64,
    /// Hold plus the slowest attack or release modulation can reach, in ms, for warmup.
    longest_ms: f64,
    sidechain: bool,
    /// Current gain, and samples left before the gate starts closing.
    gain: f64,
    hold_left: u64,
}

params! { Gate {
    THRESHOLD: ParamSpec::number(
        "threshold",
        "Threshold",
        -40.0,
        -80.0,
        0.0,
        "Level the signal must reach to open the gate",
    )
    .unit("dB")
    .exposed()
    .limits(-200.0, 60.0),
    ATTACK: ParamSpec::number(
        "attack",
        "Attack",
        1.0,
        0.01,
        1000.0,
        "How quickly the gate opens",
    )
    .unit("ms")
    .limits(0.0, 1e6),
    HOLD: ParamSpec::number(
        "hold",
        "Hold",
        50.0,
        0.0,
        5000.0,
        "How long the gate stays open after the signal drops below the threshold",
    )
    .unit("ms")
    .limits(0.0, 1e6),
    RELEASE: ParamSpec::number(
        "release",
        "Release",
        100.0,
        0.1,
        5000.0,
        "How quickly the gate closes",
    )
    .unit("ms")
    .limits(0.0, 1e6),
    RANGE: ParamSpec::number(
        "range",
        "Range",
        -80.0,
        SILENT_RANGE,
        0.0,
        "How far a closed gate turns the signal down; -80 dB is silence",
    )
    .unit("dB"),
} }

impl NodeKind for Gate {
    const KIND: &'static str = "gate";
    const SPEC: NodeSpec = NodeSpec::new("Gate", Category::Effect)
        .describe("Silences the signal while it, or a sidechain, is quiet")
        .params(Self::PARAMS)
        .inputs(&[
            InputSpec::required("in", "The signal to gate"),
            InputSpec::optional(
                "sidechain",
                "A signal whose level opens the gate instead of the input's own",
            ),
        ])
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "threshold": -12, "hold": 5, "release": 10 }"#,
        r#"{ "threshold": -6, "range": -20, "attack": 3 }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            threshold: params.number_at(Self::THRESHOLD)?,
            attack_ms: params.number_at(Self::ATTACK)?,
            hold_ms: params.number_at(Self::HOLD)?,
            release_ms: params.number_at(Self::RELEASE)?,
            range: params.number_at(Self::RANGE)?,
            threshold_gain: 0.0,
            closed_gain: 0.0,
            attack: 0.0,
            release: 0.0,
            hold_samples: 0,
            sample_rate: 1.0,
            longest_ms: 0.0,
            sidechain: false,
            gain: 0.0,
            hold_left: 0,
        })
    }
}

impl Gate {
    /// The gain of a closed gate at `range` dB.
    fn closed_gain(range: f64) -> f64 {
        if range <= SILENT_RANGE {
            0.0
        } else {
            db_to_gain(range)
        }
    }
}

impl Node for Gate {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.threshold_gain = db_to_gain(self.threshold) as f32;
        self.closed_gain = Self::closed_gain(self.range);
        self.sample_rate = ctx.sample_rate();
        self.attack = smoothing_coefficient(ms_to_samples(self.attack_ms, ctx));
        self.release = smoothing_coefficient(ms_to_samples(self.release_ms, ctx));
        self.hold_samples = ms_to_samples(self.hold_ms, ctx).round() as u64;
        let slowest = ctx
            .param_max(Self::ATTACK, self.attack_ms)
            .max(ctx.param_max(Self::RELEASE, self.release_ms));
        self.longest_ms = ctx.param_max(Self::HOLD, self.hold_ms) + 7.0 * slowest;
        self.sidechain = ctx.connected[1];
        self.reset();
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = &inputs[0].data;
        let detector = if self.sidechain {
            &inputs[1].data
        } else {
            input
        };
        let samples = |ms: f32| f64::from(ms) / 1000.0 * self.sample_rate;
        let threshold = ctx.value(Self::THRESHOLD, f64::from(self.threshold_gain));
        let range = ctx.value(Self::RANGE, self.closed_gain);
        let (attack, hold) = (ctx.param(Self::ATTACK), ctx.param(Self::HOLD));
        let release = ctx.param(Self::RELEASE);
        for (i, ((out, &x), &d)) in outputs[0]
            .data
            .iter_mut()
            .zip(input)
            .zip(detector)
            .enumerate()
        {
            let threshold = threshold.at_with(i, |t| db_to_gain(f64::from(t)) as f32);
            let open = if d.abs() >= threshold {
                self.hold_left = hold.map_or(self.hold_samples, |h| samples(h[i]).round() as u64);
                true
            } else if self.hold_left > 0 {
                self.hold_left -= 1;
                true
            } else {
                false
            };
            let closed = match range {
                crate::Value::Const(gain) => gain,
                crate::Value::Stream(r) => Self::closed_gain(f64::from(r[i])),
            };
            let target = if open { 1.0 } else { closed };
            let c = if target > self.gain {
                attack.map_or(self.attack, |a| smoothing_coefficient(samples(a[i])))
            } else {
                release.map_or(self.release, |r| smoothing_coefficient(samples(r[i])))
            };
            self.gain = target + c * (self.gain - target);
            *out = (f64::from(x) * self.gain) as f32;
        }
    }

    fn reset(&mut self) {
        self.gain = self.closed_gain;
        self.hold_left = 0;
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        settle_frames(ms_to_samples(self.longest_ms, ctx), ctx)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    #[test]
    fn opens_for_loud_signals_and_closes_after_the_hold() {
        // 1 kHz sample rate: hold 10 ms = 10 samples, near-instant attack and release.
        let params = r#"{ "threshold": -20, "attack": 0.01, "hold": 10, "release": 0.1 }"#;
        let mut gate = node("gate", params, 100, 1000.0, &[true, false]);
        let mut input = vec![0.5; 20];
        input.extend(vec![0.01; 80]);
        let out = process_one(gate.as_mut(), &[input.clone(), vec![0.0; 100]]);
        assert!((out[10] - 0.5).abs() < 1e-3, "open while loud");
        assert!((out[25] - 0.01).abs() < 1e-3, "held open after it drops");
        assert!(out[60].abs() < 1e-6, "closed after the hold");
    }

    #[test]
    fn a_closed_gate_keeps_the_range() {
        let params = r#"{ "threshold": -10, "range": -20, "release": 0.1 }"#;
        let mut gate = node("gate", params, 100, 1000.0, &[true, false]);
        let out = process_one(gate.as_mut(), &[vec![0.1; 100], vec![0.0; 100]]);
        assert!((out[99] - 0.01).abs() < 1e-4, "{}", out[99]);
    }

    #[test]
    fn the_sidechain_opens_the_gate() {
        let params = r#"{ "threshold": -20, "attack": 0.01, "hold": 0, "release": 0.1 }"#;
        let mut gate = node("gate", params, 100, 1000.0, &[true, true]);
        // A quiet input stays shut until a loud sidechain arrives at sample 50.
        let mut sidechain = vec![0.0; 100];
        sidechain[50..].fill(1.0);
        let out = process_one(gate.as_mut(), &[vec![0.05; 100], sidechain]);
        assert!(out[10].abs() < 1e-6);
        assert!((out[90] - 0.05).abs() < 1e-3);
    }
}
