use crate::dsp::{AttackRelease, db_to_gain};
use crate::nodes::support::settle_frames;
use crate::nodes::{Category, NodeKind, NodeSpec, Unit};
use crate::{InputSpec, Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// Range at or below which a closed gate is fully silent.
const SILENT_RANGE: f64 = -80.0;

/// A noise gate: lets the signal through while it (or the sidechain, when connected) is above
/// `threshold`, holds open for `hold` after it drops, then closes down to `range`. Times are in
/// milliseconds of the signal's own time unless `unit` says otherwise.
#[derive(Debug)]
pub struct Gate {
    threshold: f64,
    /// Attack, hold and release, in `unit`.
    attack_time: f64,
    hold_time: f64,
    release_time: f64,
    unit: Unit,
    range: f64,
    /// Set in `prepare`.
    threshold_gain: f32,
    closed_gain: f64,
    times: AttackRelease,
    hold_samples: u64,
    /// Samples in one `unit`.
    unit_samples: f64,
    /// Hold plus the slowest attack or release modulation can reach, in samples, for warmup.
    longest_samples: f64,
    sidechain: bool,
    /// Current gain, and samples left before the gate starts closing.
    gain: f64,
    hold_left: u64,
}

params! { Gate {
    THRESHOLD: ParamSpec::number("threshold", -40.0,
        -80.0,
        0.0)
    .unit("dB")
    .exposed()
    .limits(-200.0, 60.0),
    ATTACK: ParamSpec::number("attack", 1.0,
        0.01,
        1000.0)
    .limits(0.0, 1e6),
    HOLD: ParamSpec::number("hold", 50.0,
        0.0,
        5000.0)
    .limits(0.0, 1e6),
    RELEASE: ParamSpec::number("release", 100.0,
        0.1,
        5000.0)
    .limits(0.0, 1e6),
    UNIT: Unit::time_param("ms"),
    RANGE: ParamSpec::number("range", -80.0,
        SILENT_RANGE,
        0.0)
    .unit("dB"),
} }

impl NodeKind for Gate {
    const KIND: &'static str = "gate";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .inputs(&[
            InputSpec::required("in"),
            InputSpec::optional("sidechain"),
        ])
        .per_channel()
        .expects(crate::Range::Bipolar);
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "threshold": -12, "hold": 5, "release": 10 }"#,
        r#"{ "threshold": -6, "range": -20, "attack": 3 }"#,
        r#"{ "threshold": -12, "unit": "beat", "attack": 0.001, "hold": 0.01, "release": 0.05 }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            threshold: params.number_at(Self::THRESHOLD)?,
            attack_time: params.number_at(Self::ATTACK)?,
            hold_time: params.number_at(Self::HOLD)?,
            release_time: params.number_at(Self::RELEASE)?,
            unit: params.choice_as(Self::UNIT)?,
            range: params.number_at(Self::RANGE)?,
            threshold_gain: 0.0,
            closed_gain: 0.0,
            times: AttackRelease::default(),
            hold_samples: 0,
            unit_samples: 1.0,
            longest_samples: 0.0,
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
        self.unit_samples = self.unit.samples(ctx);
        self.times = AttackRelease::new(self.attack_time, self.release_time, self.unit_samples);
        self.hold_samples = (self.hold_time * self.unit_samples).round() as u64;
        let slowest = ctx
            .param_max(Self::ATTACK, self.attack_time)
            .max(ctx.param_max(Self::RELEASE, self.release_time));
        self.longest_samples =
            (ctx.param_max(Self::HOLD, self.hold_time) + 7.0 * slowest) * self.unit_samples;
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
        let samples = |time: f32| f64::from(time) * self.unit_samples;
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
                self.times.attack(attack, i)
            } else {
                self.times.release(release, i)
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
        settle_frames(self.longest_samples, ctx)
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
