use super::{ms_to_samples, settle_frames};
use crate::dsp::{db_to_gain, smoothing_coefficient};
use crate::nodes::{Category, NodeSpec};
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
    sidechain: bool,
    /// Current gain, and samples left before the gate starts closing.
    gain: f64,
    hold_left: u64,
}

impl Gate {
    pub const PARAMS: &[ParamSpec] = &[
        ParamSpec::number(
            "threshold",
            "Threshold",
            -40.0,
            -80.0,
            0.0,
            "Level the signal must reach to open the gate",
        )
        .unit("dB")
        .limits(-200.0, 60.0),
        ParamSpec::number(
            "attack",
            "Attack",
            1.0,
            0.01,
            1000.0,
            "How quickly the gate opens",
        )
        .unit("ms")
        .limits(0.0, 1e6),
        ParamSpec::number(
            "hold",
            "Hold",
            50.0,
            0.0,
            5000.0,
            "How long the gate stays open after the signal drops below the threshold",
        )
        .unit("ms")
        .limits(0.0, 1e6),
        ParamSpec::number(
            "release",
            "Release",
            100.0,
            0.1,
            5000.0,
            "How quickly the gate closes",
        )
        .unit("ms")
        .limits(0.0, 1e6),
        ParamSpec::number(
            "range",
            "Range",
            -80.0,
            SILENT_RANGE,
            0.0,
            "How far a closed gate turns the signal down; -80 dB is silence",
        )
        .unit("dB"),
    ];

    pub const SPEC: NodeSpec = NodeSpec::new("Gate", Category::Effect)
        .describe("Silences the signal while it, or a sidechain, is quiet")
        .params(Self::PARAMS)
        .per_channel();

    pub fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            threshold: params.number("threshold")?,
            attack_ms: params.number("attack")?,
            hold_ms: params.number("hold")?,
            release_ms: params.number("release")?,
            range: params.number("range")?,
            threshold_gain: 0.0,
            closed_gain: 0.0,
            attack: 0.0,
            release: 0.0,
            hold_samples: 0,
            sidechain: false,
            gain: 0.0,
            hold_left: 0,
        })
    }
}

impl Node for Gate {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in"), InputSpec::optional("sidechain")];
        INPUTS
    }

    fn prepare(&mut self, ctx: &PrepareContext) {
        self.threshold_gain = db_to_gain(self.threshold) as f32;
        self.closed_gain = if self.range <= SILENT_RANGE {
            0.0
        } else {
            db_to_gain(self.range)
        };
        self.attack = smoothing_coefficient(ms_to_samples(self.attack_ms, ctx));
        self.release = smoothing_coefficient(ms_to_samples(self.release_ms, ctx));
        self.hold_samples = ms_to_samples(self.hold_ms, ctx).round() as u64;
        self.sidechain = ctx.connected[1];
        self.reset();
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = &inputs[0].data;
        let detector = if self.sidechain {
            &inputs[1].data
        } else {
            input
        };
        for ((out, &x), &d) in outputs[0].data.iter_mut().zip(input).zip(detector) {
            let open = if d.abs() >= self.threshold_gain {
                self.hold_left = self.hold_samples;
                true
            } else if self.hold_left > 0 {
                self.hold_left -= 1;
                true
            } else {
                false
            };
            let target = if open { 1.0 } else { self.closed_gain };
            let c = if target > self.gain {
                self.attack
            } else {
                self.release
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
        let slowest = self.attack_ms.max(self.release_ms);
        settle_frames(ms_to_samples(self.hold_ms + 7.0 * slowest, ctx), ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{node, process};

    #[test]
    fn opens_for_loud_signals_and_closes_after_the_hold() {
        // 1 kHz sample rate: hold 10 ms = 10 samples, near-instant attack and release.
        let params = r#"{ "threshold": -20, "attack": 0.01, "hold": 10, "release": 0.1 }"#;
        let mut gate = node("gate", params, 100, 1000.0, &[true, false]);
        let mut input = vec![0.5; 20];
        input.extend(vec![0.01; 80]);
        let out = process(gate.as_mut(), &[input.clone(), vec![0.0; 100]]);
        assert!((out[10] - 0.5).abs() < 1e-3, "open while loud");
        assert!((out[25] - 0.01).abs() < 1e-3, "held open after it drops");
        assert!(out[60].abs() < 1e-6, "closed after the hold");
    }

    #[test]
    fn a_closed_gate_keeps_the_range() {
        let params = r#"{ "threshold": -10, "range": -20, "release": 0.1 }"#;
        let mut gate = node("gate", params, 100, 1000.0, &[true, false]);
        let out = process(gate.as_mut(), &[vec![0.1; 100], vec![0.0; 100]]);
        assert!((out[99] - 0.01).abs() < 1e-4, "{}", out[99]);
    }
}
