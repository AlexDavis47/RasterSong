use std::f64::consts::PI;

use crate::dsp::{Biquad, BiquadKind, DelayLine};
use crate::nodes::support::MAX_WARMUP_FRAMES;
use crate::nodes::{Category, FreqUnit, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

choice! {
    /// The response of the filter.
    pub enum Kind {
        /// Passes what is slower than the cutoff.
        LowPass = "lowpass",
        /// Passes what is faster than the cutoff.
        HighPass = "highpass",
        /// Passes a band around the cutoff.
        BandPass = "bandpass",
        /// Keeps every level and shifts phase around the cutoff: smears edges without blurring.
        AllPass = "allpass",
        /// Tilts the balance between low and high by `gain` dB around the cutoff.
        Tilt = "tilt",
        /// Repeats the signal every cycle of the cutoff, giving a ringing, striped echo.
        Comb = "comb",
    }
}

/// A resonant filter with a choice of responses, running across rows and frames like an audio
/// filter. The cutoff is in cycles per row by default, so the look is the same at any resolution.
#[derive(Debug)]
pub struct Filter {
    kind: Kind,
    cutoff: f64,
    unit: FreqUnit,
    q: f64,
    gain: f64,
    /// Set in `prepare`.
    per_sample: f64,
    modulated: bool,
    /// The largest and smallest cutoff in cycles per sample that modulation can reach.
    fastest: f64,
    slowest: f64,
    sections: [Biquad; 2],
    /// Comb filter state: the output history.
    line: DelayLine,
}

params! { Filter {
    RESPONSE: ParamSpec::choice(
        "response",
        "Type",
        Kind::OPTIONS,
        "lowpass",
        "lowpass, highpass, bandpass, allpass, tilt (gain dB of low-versus-high balance) or comb (echo every cutoff cycle)",
    ),
    CUTOFF: ParamSpec::number("cutoff", "Cutoff", 40.0, 0.01, 1000.0, "Frequency of the filter's corner or centre")
        .exposed()
        .limits(1e-06, 1e9)
        .octaves(),
    UNIT: ParamSpec::choice(
        "unit",
        "Unit",
        FreqUnit::OPTIONS,
        "cycles/row",
        "Unit for the cutoff",
    ),
    Q: ParamSpec::number(
        "q",
        "Resonance",
        0.707,
        0.1,
        20.0,
        "Sharpness: 0.707 is flat, higher rings or narrows. For a comb, higher repeats more",
    )
    .limits(0.05, 100.0),
    GAIN: ParamSpec::number("gain", "Gain", 0.0, -24.0, 24.0, "For tilt: dB boost of lows and cut of highs (negative reverses)")
        .unit("dB")
        .limits(-48.0, 48.0),
} }

impl NodeKind for Filter {
    const KIND: &'static str = "filter";
    const SPEC: NodeSpec = NodeSpec::new("Filter", Category::Effect)
        .describe("A resonant low, high, band or all pass, tilt or comb filter")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "response": "lowpass", "cutoff": 0.7, "q": 4 }"#,
        r#"{ "response": "highpass", "cutoff": 1.2 }"#,
        r#"{ "response": "bandpass", "cutoff": 0.9, "q": 8 }"#,
        r#"{ "response": "allpass", "cutoff": 0.5 }"#,
        r#"{ "response": "tilt", "cutoff": 1, "gain": 9 }"#,
        r#"{ "response": "comb", "cutoff": 0.5, "q": 3 }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "response": "lowpass", "cutoff": 40, "q": 2 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            kind: params.choice_as(Self::RESPONSE)?,
            cutoff: params.number_at(Self::CUTOFF)?,
            unit: params.choice_as(Self::UNIT)?,
            q: params.number_at(Self::Q)?,
            gain: params.number_at(Self::GAIN)?,
            per_sample: 0.0,
            modulated: false,
            fastest: 0.0,
            slowest: 0.0,
            sections: [Biquad::default(); 2],
            line: DelayLine::default(),
        })
    }
}

impl Filter {
    /// The feedback of a comb with quality `q`: 0.41 at the flat 0.707, approaching 1.
    fn feedback(q: f64) -> f32 {
        (1.0 - 1.0 / (1.0 + q)).min(0.995) as f32
    }

    /// The biquad sections for a cutoff in cycles per sample.
    fn design(&self, cutoff: f64, q: f64, gain: f64) -> [Biquad; 2] {
        let one = |kind| Biquad::design(kind, cutoff, q);
        match self.kind {
            Kind::LowPass => [one(BiquadKind::LowPass), Biquad::default()],
            Kind::HighPass => [one(BiquadKind::HighPass), Biquad::default()],
            Kind::BandPass => [one(BiquadKind::BandPass), Biquad::default()],
            Kind::AllPass => [one(BiquadKind::AllPass), Biquad::default()],
            Kind::Tilt => [
                one(BiquadKind::LowShelf { gain_db: gain / 2.0 }),
                one(BiquadKind::HighShelf { gain_db: -gain / 2.0 }),
            ],
            Kind::Comb => [Biquad::default(); 2],
        }
    }

    fn cutoff_per_sample(&self, value: f64) -> f64 {
        value * self.per_sample
    }
}

/// A `Biquad` with the identity response: what an unused second section is. `process` of a
/// default (all-zero) biquad would output silence, so unused sections are skipped instead.
fn sections_used(kind: Kind) -> usize {
    match kind {
        Kind::Tilt => 2,
        Kind::Comb => 0,
        _ => 1,
    }
}

impl Node for Filter {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.per_sample = self.unit.per_sample(1.0, ctx);
        self.modulated = ctx.modulation(Self::CUTOFF).is_some();
        self.fastest = self.cutoff_per_sample(ctx.param_max(Self::CUTOFF, self.cutoff));
        self.slowest = self.cutoff_per_sample(ctx.param_min(Self::CUTOFF, self.cutoff));
        self.sections = self.design(self.cutoff_per_sample(self.cutoff), self.q, self.gain);
        // A comb's period is at most this long; the line also holds the one-sample feedback delay.
        let longest = (1.0 / self.slowest.max(1e-9)).min(4.0 * ctx.samples_per_frame() as f64);
        self.line = DelayLine::new(longest.ceil() as usize + 2);
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = &inputs[0].data;
        let cutoff = ctx.value(Self::CUTOFF, self.cutoff);
        let modulated = ctx.param(Self::CUTOFF).is_some() && self.modulated;
        if self.kind == Kind::Comb {
            let feedback = Self::feedback(self.q);
            for (i, (out, &x)) in outputs[0].data.iter_mut().zip(input).enumerate() {
                let period = 1.0 / self.cutoff_per_sample(cutoff.at64(i)).max(1e-9);
                // The newest output isn't in the line yet, so the period is at least one sample.
                let echo = self.line.read((period - 1.0).max(0.0));
                let y = x + feedback * echo;
                self.line.push(y);
                // Compensate for the resonant gain so levels stay near the input's.
                *out = y * (1.0 - feedback);
            }
            return;
        }
        let used = sections_used(self.kind);
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(input).enumerate() {
            if modulated {
                let designed =
                    self.design(self.cutoff_per_sample(cutoff.at64(i)), self.q, self.gain);
                for (section, new) in self.sections.iter_mut().zip(designed).take(used) {
                    section.retune(new);
                }
            }
            let mut y = f64::from(x);
            for section in &mut self.sections[..used] {
                y = section.process(y);
            }
            *out = y as f32;
        }
    }

    fn reset(&mut self) {
        for s in &mut self.sections {
            s.reset();
        }
        self.line.reset();
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        let frame = ctx.samples_per_frame() as f64;
        let samples = match self.kind {
            // Periods until the repeats have decayed by 60 dB.
            Kind::Comb => {
                let repeats = 0.001f64.ln() / f64::from(Self::feedback(self.q)).ln();
                repeats / self.slowest.max(1e-9)
            }
            // A resonant section rings for about 7 time constants of Q / (π f).
            _ => 7.0 * self.q.max(1.0) / (PI * self.slowest.max(1e-9)),
        };
        if samples.is_finite() {
            ((samples / frame).ceil() as u32).clamp(1, MAX_WARMUP_FRAMES)
        } else {
            MAX_WARMUP_FRAMES
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use crate::testing::{node, process_one};

    /// Peak output of a sine of `cycles` cycles per `len` samples through the filter (a row is
    /// the whole block, so the cutoff is in cycles per block), after a settling block.
    fn gain(params: &str, cycles: f64) -> f32 {
        let len = 256;
        let mut node = node("filter", params, len, len as f64, &[true]);
        let sine: Vec<f32> = (0..len)
            .map(|i| (TAU * cycles * i as f64 / len as f64).sin() as f32)
            .collect();
        process_one(node.as_mut(), std::slice::from_ref(&sine));
        let out = process_one(node.as_mut(), &[sine]);
        out.iter().fold(0.0f32, |peak, &x| peak.max(x.abs()))
    }

    #[test]
    fn low_pass_passes_slow_and_blocks_fast() {
        let p = r#"{ "response": "lowpass", "cutoff": 8 }"#;
        assert!(gain(p, 1.0) > 0.95);
        assert!(gain(p, 100.0) < 0.05);
    }

    #[test]
    fn high_pass_passes_fast_and_blocks_slow() {
        let p = r#"{ "response": "highpass", "cutoff": 8 }"#;
        assert!(gain(p, 1.0) < 0.05);
        assert!(gain(p, 100.0) > 0.95);
    }

    #[test]
    fn band_pass_peaks_at_the_cutoff() {
        let p = r#"{ "response": "bandpass", "cutoff": 16, "q": 4 }"#;
        assert!((gain(p, 16.0) - 1.0).abs() < 0.05);
        assert!(gain(p, 2.0) < 0.3 && gain(p, 100.0) < 0.3);
    }

    #[test]
    fn all_pass_keeps_every_level() {
        let p = r#"{ "response": "allpass", "cutoff": 16 }"#;
        for cycles in [2.0, 16.0, 90.0] {
            assert!((gain(p, cycles) - 1.0).abs() < 0.02, "{cycles}");
        }
    }

    #[test]
    fn tilt_boosts_lows_and_cuts_highs() {
        let p = r#"{ "response": "tilt", "cutoff": 16, "gain": 12 }"#;
        assert!(gain(p, 1.0) > 1.8, "+6 dB is about x2");
        assert!(gain(p, 110.0) < 0.55);
    }

    #[test]
    fn comb_notches_half_a_cycle_and_resonates_at_the_cutoff() {
        // A comb with a period of 16 samples (cutoff 16 cycles per block of 256) resonates at
        // multiples of 16 cycles and is weakest in between.
        let p = r#"{ "response": "comb", "cutoff": 16, "q": 3 }"#;
        assert!(gain(p, 16.0) > 2.0 * gain(p, 8.0));
    }
}
