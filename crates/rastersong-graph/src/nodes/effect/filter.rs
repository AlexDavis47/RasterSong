use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};

use crate::dsp::{Biquad, BiquadKind, DelayLine};
use crate::nodes::support::UNBOUNDED_WARMUP;
use crate::nodes::{Category, NodeKind, NodeSpec, Unit};
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

choice! {
    /// How steeply a low or high pass cuts beyond the cutoff, in dB per octave.
    pub enum Slope {
        /// A single pole: a gentle, smooth roll-off with no resonance.
        Six = "6",
        /// Two poles: the classic resonant filter.
        Twelve = "12",
        /// Four poles: two cascaded stages.
        TwentyFour = "24",
        /// Eight poles: four cascaded stages, a very sharp cut.
        FortyEight = "48",
    }
}

impl Slope {
    /// The number of biquad stages (the one-pole slope has none).
    fn stages(self) -> usize {
        match self {
            Self::Six => 0,
            Self::Twelve => 1,
            Self::TwentyFour => 2,
            Self::FortyEight => 4,
        }
    }

    /// The quality factor of stage `index` (0-based) of a Butterworth filter of this slope. The
    /// last stage has the highest, and carries the resonance.
    fn butterworth_q(self, index: usize) -> f64 {
        let order = (self.stages() * 2) as f64;
        1.0 / (2.0 * ((2 * index + 1) as f64 * PI / (2.0 * order)).cos())
    }
}

/// A resonant filter with a choice of responses, running across rows and frames like an audio
/// filter. The cutoff is in cycles per row by default, so the look is the same at any resolution.
#[derive(Debug)]
pub struct Filter {
    kind: Kind,
    slope: Slope,
    cutoff: f64,
    unit: Unit,
    q: f64,
    gain: f64,
    /// Set in `prepare`.
    per_sample: f64,
    modulated: bool,
    /// The largest and smallest cutoff in cycles per sample that modulation can reach.
    fastest: f64,
    slowest: f64,
    sections: [Biquad; MAX_STAGES],
    /// The 6 dB slope's state, and its smoothing coefficient while the cutoff isn't modulated.
    pole: f32,
    coefficient: f32,
    /// Comb filter state: the output history.
    line: DelayLine,
}

/// The most biquad stages any response uses (the 48 dB/oct slope).
const MAX_STAGES: usize = 4;

params! { Filter {
    RESPONSE: ParamSpec::choice(
        "response",
        "Type",
        Kind::OPTIONS,
        "lowpass",
        "lowpass, highpass, bandpass, allpass, tilt (gain dB of low-versus-high balance) or comb (echo every cutoff cycle)",
    ),
    SLOPE: ParamSpec::choice(
        "slope",
        "Slope (dB/oct)",
        Slope::OPTIONS,
        "12",
        "How sharply the cut falls off past the cutoff: 6 is a gentle one-pole roll-off, 48 a brick wall",
    )
    .shown_when("response", &["lowpass", "highpass"]),
    CUTOFF: ParamSpec::number("cutoff", "Cutoff", 40.0, 0.01, 200.0, "Frequency of the filter's corner or centre")
        .exposed()
        .limits(1e-06, 1e9),
    UNIT: Unit::freq_param("row", "Unit for the cutoff"),
    Q: ParamSpec::number(
        "q",
        "Resonance",
        0.707,
        0.1,
        20.0,
        "Sharpness: 0.707 is flat (no resonance), higher rings or narrows. For a comb, higher repeats more. The 6 dB slope has none",
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
        r#"{ "response": "lowpass", "slope": "6", "cutoff": 0.7 }"#,
        r#"{ "response": "highpass", "slope": "6", "cutoff": 0.7 }"#,
        r#"{ "response": "lowpass", "slope": "24", "cutoff": 0.7, "q": 2 }"#,
        r#"{ "response": "highpass", "slope": "48", "cutoff": 0.9 }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "response": "lowpass", "cutoff": 40, "q": 2 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            kind: params.choice_as(Self::RESPONSE)?,
            slope: params.choice_as(Self::SLOPE)?,
            cutoff: params.number_at(Self::CUTOFF)?,
            unit: params.choice_as(Self::UNIT)?,
            q: params.number_at(Self::Q)?,
            gain: params.number_at(Self::GAIN)?,
            per_sample: 0.0,
            modulated: false,
            fastest: 0.0,
            slowest: 0.0,
            sections: [Biquad::default(); MAX_STAGES],
            pole: 0.0,
            coefficient: 1.0,
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
    fn design(&self, cutoff: f64, q: f64, gain: f64) -> [Biquad; MAX_STAGES] {
        let one = |kind| Biquad::design(kind, cutoff, q);
        let mut sections = [Biquad::default(); MAX_STAGES];
        match self.kind {
            Kind::LowPass | Kind::HighPass => {
                let kind = if self.kind == Kind::LowPass {
                    BiquadKind::LowPass
                } else {
                    BiquadKind::HighPass
                };
                let stages = self.slope.stages();
                for (i, section) in sections.iter_mut().take(stages).enumerate() {
                    // Butterworth stages are maximally flat; the resonance scales the last,
                    // sharpest one (so the default 0.707 is flat at every slope).
                    let resonance = if i + 1 == stages {
                        q / FRAC_1_SQRT_2
                    } else {
                        1.0
                    };
                    *section =
                        Biquad::design(kind, cutoff, self.slope.butterworth_q(i) * resonance);
                }
            }
            Kind::BandPass => sections[0] = one(BiquadKind::BandPass),
            Kind::AllPass => sections[0] = one(BiquadKind::AllPass),
            Kind::Tilt => {
                sections[0] = one(BiquadKind::LowShelf {
                    gain_db: gain / 2.0,
                });
                sections[1] = one(BiquadKind::HighShelf {
                    gain_db: -gain / 2.0,
                });
            }
            Kind::Comb => {}
        }
        sections
    }

    /// Whether this is the gentle one-pole low or high pass, which has its own simple loop.
    fn one_pole(&self) -> bool {
        matches!(self.kind, Kind::LowPass | Kind::HighPass) && self.slope == Slope::Six
    }

    /// Smoothing coefficient of the one-pole filter for a cutoff in cycles per sample.
    fn pole_coefficient(cycles_per_sample: f64) -> f32 {
        (1.0 - (-TAU * cycles_per_sample.min(0.5)).exp()) as f32
    }

    fn cutoff_per_sample(&self, value: f64) -> f64 {
        value * self.per_sample
    }
}

/// A `Biquad` with the identity response: what an unused second section is. `process` of a
/// default (all-zero) biquad would output silence, so unused sections are skipped instead.
fn sections_used(kind: Kind, slope: Slope) -> usize {
    match kind {
        Kind::LowPass | Kind::HighPass => slope.stages(),
        Kind::Tilt => 2,
        Kind::Comb => 0,
        Kind::BandPass | Kind::AllPass => 1,
    }
}

impl Node for Filter {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.per_sample = self.unit.per_sample(1.0, ctx);
        self.modulated = ctx.modulation(Self::CUTOFF).is_some();
        self.fastest = self.cutoff_per_sample(ctx.param_max(Self::CUTOFF, self.cutoff));
        self.slowest = self.cutoff_per_sample(ctx.param_min(Self::CUTOFF, self.cutoff));
        self.sections = self.design(self.cutoff_per_sample(self.cutoff), self.q, self.gain);
        self.coefficient = Self::pole_coefficient(self.cutoff_per_sample(self.cutoff));
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
        if self.one_pole() {
            let high = self.kind == Kind::HighPass;
            let mut y = self.pole;
            for (i, (out, &x)) in outputs[0].data.iter_mut().zip(input).enumerate() {
                let a = if modulated {
                    Self::pole_coefficient(self.cutoff_per_sample(cutoff.at64(i)))
                } else {
                    self.coefficient
                };
                y += a * (x - y);
                *out = if high { x - y } else { y };
            }
            self.pole = y;
            return;
        }
        let used = sections_used(self.kind, self.slope);
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
        self.pole = 0.0;
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
            _ if self.one_pole() => 7.0 / (TAU * self.slowest.max(1e-9)),
            // Cascaded stages ring for longer, roughly in proportion to their number.
            _ => {
                let stages = sections_used(self.kind, self.slope).max(1) as f64;
                7.0 * self.q.max(1.0) * stages / (PI * self.slowest.max(1e-9))
            }
        };
        if samples.is_finite() {
            ((samples / frame).ceil() as u32).max(1)
        } else {
            UNBOUNDED_WARMUP
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
    fn steeper_slopes_cut_more_and_stay_flat_in_the_pass_band() {
        // Two octaves above the cutoff the response has fallen by about 12 dB per 6 of slope.
        let at = |slope: &str| {
            gain(
                &format!(r#"{{ "response": "lowpass", "slope": "{slope}", "cutoff": 8 }}"#),
                32.0,
            )
        };
        let (g6, g12, g24, g48) = (at("6"), at("12"), at("24"), at("48"));
        assert!(g6 > g12 && g12 > g24 && g24 > g48, "{g6} {g12} {g24} {g48}");
        assert!(
            (g24 - 1.0 / 16.0 / 16.0).abs() < 0.01,
            "24 dB/oct is -48 dB two octaves up: {g24}"
        );
        for slope in ["6", "12", "24", "48"] {
            let p = format!(r#"{{ "response": "lowpass", "slope": "{slope}", "cutoff": 32 }}"#);
            assert!(gain(&p, 1.0) > 0.97, "{slope} passes the low end");
            let p = format!(r#"{{ "response": "highpass", "slope": "{slope}", "cutoff": 8 }}"#);
            assert!(gain(&p, 120.0) > 0.9, "{slope} passes the high end");
        }
    }

    #[test]
    fn resonance_peaks_the_cutoff_at_every_slope() {
        for slope in ["12", "24", "48"] {
            let flat = gain(&format!(r#"{{ "slope": "{slope}", "cutoff": 16 }}"#), 16.0);
            let peaked = gain(
                &format!(r#"{{ "slope": "{slope}", "cutoff": 16, "q": 6 }}"#),
                16.0,
            );
            assert!(peaked > 2.0 * flat, "{slope}: {peaked} vs {flat}");
        }
    }

    #[test]
    fn the_six_db_slope_is_the_one_pole_low_pass() {
        // One cycle per 32-sample row: coefficient 1 - exp(-2π/32).
        let len = 32;
        let mut n = node(
            "filter",
            r#"{ "slope": "6", "cutoff": 1 }"#,
            len,
            len as f64,
            &[true],
        );
        let out = process_one(n.as_mut(), &[vec![1.0; len]]);
        let a = 1.0 - (-std::f32::consts::TAU / 32.0).exp();
        assert!((out[0] - a).abs() < 1e-6);
        assert!((out[1] - (a + a * (1.0 - a))).abs() < 1e-6);
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
