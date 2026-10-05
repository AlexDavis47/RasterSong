use std::f64::consts::TAU;

use crate::dsp::{HILBERT_TAPS, Hilbert, mix};
use crate::nodes::support::{SampleClock, settle_frames};
use crate::nodes::{Category, FreqUnit, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// Frequency shifter: moves every frequency in the signal up or down by the same amount, in Hz
/// rather than octaves, so harmonics stop lining up. Small shifts give a slow phasing beat, large
/// ones bell-like, inharmonic tones. In video the same idea slides the pattern's spatial
/// frequencies.
///
/// It builds the analytic signal with a Hilbert transformer and rotates it, so the output lags the
/// input by [`Hilbert::LATENCY`] samples (reported as the node's latency).
#[derive(Debug)]
pub struct FrequencyShifter {
    shift: f64,
    unit: FreqUnit,
    mix: f32,
    /// Cycles per sample of one unit of shift, set in `prepare`.
    scale: f64,
    hilbert: Hilbert,
    clock: SampleClock,
    /// The oscillator's phase in cycles, or `None` until the first block after a reset.
    phase: Option<f64>,
}

params! { FrequencyShifter {
    SHIFT: ParamSpec::number(
        "shift",
        "Shift",
        100.0,
        -1000.0,
        1000.0,
        "How far every frequency moves: positive shifts up, negative down",
    )
    .exposed()
    .limits(-1e9, 1e9),
    UNIT: FreqUnit::param("Hertz", "Unit for the shift"),
    MIX: ParamSpec::number(
        "mix",
        "Mix",
        1.0,
        0.0,
        1.0,
        "0 is the dry input, 1 is only the shifted signal; in between beats against the original",
    ),
} }

impl NodeKind for FrequencyShifter {
    const KIND: &'static str = "frequency_shifter";
    const SPEC: NodeSpec = NodeSpec::new("Frequency Shifter", Category::Effect)
        .describe("Moves every frequency up or down by a fixed amount, giving inharmonic tones")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "shift": 5, "unit": "Row" }"#,
        r#"{ "shift": -300, "mix": 0.5 }"#,
        r#"{ "shift": 2, "unit": "Frame", "mix": 0.8 }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            shift: params.number_at(Self::SHIFT)?,
            unit: params.choice_as(Self::UNIT)?,
            mix: params.float_at(Self::MIX)?,
            scale: 0.0,
            hilbert: Hilbert::default(),
            clock: SampleClock::default(),
            phase: None,
        })
    }
}

impl Node for FrequencyShifter {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.scale = self.unit.per_sample(1.0, ctx);
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let shift = ctx.value(Self::SHIFT, self.shift);
        let amount = ctx.value(Self::MIX, f64::from(self.mix));
        let start = self.clock.begin(ctx, inputs[0].data.len());
        // A constant shift gives the same phase wherever a render starts; a modulated one can only
        // accumulate from its first block.
        let mut phase = self
            .phase
            .unwrap_or_else(|| (start as f64 * self.shift * self.scale).rem_euclid(1.0));
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let (real, imag) = self.hilbert.push(x);
            let (sin, cos) = (TAU * phase).sin_cos();
            let shifted = real * cos as f32 - imag * sin as f32;
            *out = mix(real, shifted, amount.at(i));
            phase = (phase + shift.at64(i) * self.scale).rem_euclid(1.0);
        }
        self.phase = Some(phase);
    }

    fn reset(&mut self) {
        self.hilbert.reset();
        self.clock.reset();
        self.phase = None;
    }

    fn latency(&self, _ctx: &PrepareContext) -> usize {
        Hilbert::LATENCY
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        settle_frames(HILBERT_TAPS as f64, ctx)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use crate::testing::{node, process_one};

    /// Power of `signal` at `freq` cycles per sample.
    fn power_at(signal: &[f32], freq: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (n, &x) in signal.iter().enumerate() {
            let w = TAU * freq * n as f64;
            re += f64::from(x) * w.cos();
            im += f64::from(x) * w.sin();
        }
        (re * re + im * im) / (signal.len() as f64).powi(2)
    }

    fn shifted(shift: f64, input_freq: f64) -> Vec<f32> {
        let len = 2048;
        // One block is a second at 2048 samples a second, so Hertz are cycles per 1/2048.
        let mut n = node(
            "frequency_shifter",
            &format!(r#"{{ "shift": {shift} }}"#),
            len,
            len as f64,
            &[true],
        );
        let input: Vec<f32> = (0..len)
            .map(|i| (TAU * input_freq * i as f64).sin() as f32)
            .collect();
        process_one(n.as_mut(), &[input])
    }

    #[test]
    fn a_sine_moves_up_by_the_shift() {
        // 256 Hz shifted by 128 Hz at 2048 samples a second: from 0.125 to 0.1875 cycles/sample.
        let out = shifted(128.0, 0.125);
        let tail = &out[256..];
        assert!(power_at(tail, 0.1875) > 0.2, "{}", power_at(tail, 0.1875));
        assert!(power_at(tail, 0.125) < 0.01);
    }

    #[test]
    fn a_negative_shift_moves_down() {
        let out = shifted(-128.0, 0.25);
        let tail = &out[256..];
        assert!(power_at(tail, 0.1875) > 0.2);
        assert!(power_at(tail, 0.3125) < 0.01);
    }

    #[test]
    fn zero_shift_is_the_delayed_input() {
        let out = shifted(0.0, 0.1);
        let delay = crate::dsp::Hilbert::LATENCY;
        let want: Vec<f32> = (0..out.len())
            .map(|i| {
                if i < delay {
                    0.0
                } else {
                    (TAU * 0.1 * (i - delay) as f64).sin() as f32
                }
            })
            .collect();
        for i in 256..out.len() {
            assert!((out[i] - want[i]).abs() < 0.03, "sample {i}");
        }
    }
}
