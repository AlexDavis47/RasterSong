use std::f64::consts::PI;

use super::filter::Slope;
use crate::dsp::{Biquad, BiquadKind, MAX_STAGES, butterworth_cascade};
use crate::nodes::support::UNBOUNDED_WARMUP;
use crate::nodes::{Category, NodeKind, NodeSpec, Unit};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

choice! {
    /// What the band does to the signal.
    pub enum Shape {
        /// Boosts or cuts a range around the frequency by `gain` dB.
        Peak = "peak",
        /// Boosts or cuts everything below the frequency by `gain` dB.
        LowShelf = "low_shelf",
        /// Boosts or cuts everything above the frequency by `gain` dB.
        HighShelf = "high_shelf",
        /// Removes what is slower than the frequency.
        LowCut = "low_cut",
        /// Removes what is faster than the frequency.
        HighCut = "high_cut",
        /// Removes a narrow range around the frequency.
        Notch = "notch",
        /// Keeps only a range around the frequency.
        BandPass = "band_pass",
    }
}

/// A single equaliser band. Any number of bands in series make a full equaliser, so one node is
/// one band: add more nodes for more bands.
#[derive(Debug)]
pub struct Equalizer {
    band: Shape,
    slope: Slope,
    unit: Unit,
    frequency: f64,
    q: f64,
    gain: f64,
    /// Set in `prepare`: cycles per sample for one unit, and whether any parameter is modulated.
    per_sample: f64,
    modulated: bool,
    /// Slowest frequency in cycles per sample and highest Q, for warmup.
    slowest: f64,
    sharpest: f64,
    sections: [Biquad; MAX_STAGES],
}

params! { Equalizer {
    BAND: ParamSpec::choice("band", Shape::OPTIONS, "peak"),
    FREQUENCY: ParamSpec::number("frequency", 30.0, 0.01, 500.0)
        .exposed()
        .limits(1e-06, 1e9),
    UNIT: Unit::freq_param("row"),
    Q: ParamSpec::number("q", 0.707, 0.1, 20.0)
        .limits(0.05, 100.0),
    GAIN: ParamSpec::number("gain", 0.0, -24.0, 24.0)
        .unit("dB")
        .exposed()
        .limits(-48.0, 48.0)
        .shown_when("band", &["peak", "low_shelf", "high_shelf"]),
    SLOPE: ParamSpec::choice("slope", &["12", "24", "48"], "12")
        .shown_when("band", &["low_cut", "high_cut"]),
} }

impl NodeKind for Equalizer {
    const KIND: &'static str = "equalizer";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "band": "peak", "frequency": 0.9, "gain": 12, "q": 6 }"#,
        r#"{ "band": "low_shelf", "frequency": 0.3, "gain": 6 }"#,
        r#"{ "band": "high_shelf", "frequency": 1.5, "gain": -9 }"#,
        r#"{ "band": "low_cut", "frequency": 0.5 }"#,
        r#"{ "band": "high_cut", "frequency": 0.9, "slope": "24", "q": 2 }"#,
        r#"{ "band": "high_cut", "frequency": 0.9, "slope": "48" }"#,
        r#"{ "band": "notch", "frequency": 0.7, "q": 4 }"#,
        r#"{ "band": "band_pass", "frequency": 0.9, "q": 8 }"#,
    ];
    const BENCH: Option<&'static str> =
        Some(r#"{ "band": "peak", "frequency": 30, "gain": -6, "q": 1 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            band: params.choice_as(Self::BAND)?,
            slope: params.choice_as(Self::SLOPE)?,
            unit: params.choice_as(Self::UNIT)?,
            frequency: params.number_at(Self::FREQUENCY)?,
            q: params.number_at(Self::Q)?,
            gain: params.number_at(Self::GAIN)?,
            per_sample: 0.0,
            modulated: false,
            slowest: 0.0,
            sharpest: 1.0,
            sections: [Biquad::default(); MAX_STAGES],
        })
    }
}

impl Equalizer {
    /// How many biquad sections the band uses.
    fn used(&self) -> usize {
        match self.band {
            Shape::LowCut | Shape::HighCut => self.slope.stages(),
            _ => 1,
        }
    }

    /// The sections for a frequency (in this node's unit), quality factor and gain (dB).
    fn design(&self, frequency: f64, q: f64, gain: f64) -> [Biquad; MAX_STAGES] {
        let f = frequency * self.per_sample;
        let one = |kind| {
            let mut sections = [Biquad::default(); MAX_STAGES];
            sections[0] = Biquad::design(kind, f, q);
            sections
        };
        match self.band {
            Shape::Peak => one(BiquadKind::Peak { gain_db: gain }),
            Shape::LowShelf => one(BiquadKind::LowShelf { gain_db: gain }),
            Shape::HighShelf => one(BiquadKind::HighShelf { gain_db: gain }),
            Shape::Notch => one(BiquadKind::Notch),
            Shape::BandPass => one(BiquadKind::BandPass),
            Shape::LowCut => butterworth_cascade(true, self.slope.stages(), f, q),
            Shape::HighCut => butterworth_cascade(false, self.slope.stages(), f, q),
        }
    }
}

impl Node for Equalizer {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.per_sample = self.unit.per_sample(1.0, ctx);
        self.modulated = [Self::FREQUENCY, Self::Q, Self::GAIN]
            .iter()
            .any(|&p| ctx.modulation(p).is_some());
        self.slowest = ctx.param_min(Self::FREQUENCY, self.frequency) * self.per_sample;
        self.sharpest = ctx.param_max(Self::Q, self.q);
        self.sections = self.design(self.frequency, self.q, self.gain);
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let frequency = ctx.value(Self::FREQUENCY, self.frequency);
        let q = ctx.value(Self::Q, self.q);
        let gain = ctx.value(Self::GAIN, self.gain);
        let used = self.used();
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            if self.modulated {
                let designed = self.design(frequency.at64(i), q.at64(i), gain.at64(i));
                for (section, new) in self.sections.iter_mut().zip(designed).take(used) {
                    section.retune(new);
                }
            }
            let mut y = f64::from(x);
            for section in self.sections.iter_mut().take(used) {
                y = section.process(y);
            }
            *out = y as f32;
        }
    }

    fn reset(&mut self) {
        for section in &mut self.sections {
            section.reset();
        }
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        // A resonant section rings for about 7 time constants of Q / (π f); cascaded stages ring
        // for longer, roughly in proportion to their number.
        let stages = self.used() as f64;
        let samples = 7.0 * self.sharpest.max(1.0) * stages / (PI * self.slowest.max(1e-9));
        if samples.is_finite() {
            ((samples / ctx.samples_per_frame() as f64).ceil() as u32).max(1)
        } else {
            UNBOUNDED_WARMUP
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use crate::testing::{node, process_one};

    /// Gain at `cycles` per 256-sample block (one row) after settling.
    fn gain(params: &str, cycles: f64) -> f32 {
        let len = 256;
        let mut node = node("equalizer", params, len, len as f64, &[true]);
        let sine: Vec<f32> = (0..len)
            .map(|i| (TAU * cycles * i as f64 / len as f64).sin() as f32)
            .collect();
        for _ in 0..4 {
            process_one(node.as_mut(), std::slice::from_ref(&sine));
        }
        let out = process_one(node.as_mut(), &[sine]);
        out.iter().fold(0.0f32, |peak, &x| peak.max(x.abs()))
    }

    #[test]
    fn a_flat_band_passes_everything() {
        for cycles in [1.0, 20.0, 100.0] {
            assert!((gain("{}", cycles) - 1.0).abs() < 0.01, "{cycles}");
        }
    }

    #[test]
    fn a_peak_moves_only_its_own_range() {
        let p = r#"{ "band": "peak", "frequency": 30, "gain": -12, "q": 4 }"#;
        assert!(gain(p, 30.0) < 0.3, "centre is cut");
        assert!((gain(p, 3.0) - 1.0).abs() < 0.05, "far below is untouched");
    }

    #[test]
    fn shelves_move_everything_beyond_their_corner() {
        let low = r#"{ "band": "low_shelf", "frequency": 8, "gain": 12 }"#;
        assert!(gain(low, 0.5) > 3.5, "+12 dB is x4");
        assert!((gain(low, 100.0) - 1.0).abs() < 0.1);
        let high = r#"{ "band": "high_shelf", "frequency": 60, "gain": 12 }"#;
        assert!(gain(high, 120.0) > 3.5);
        assert!((gain(high, 1.0) - 1.0).abs() < 0.1);
    }

    #[test]
    fn cuts_remove_one_side_and_steeper_slopes_remove_more() {
        let low = r#"{ "band": "low_cut", "frequency": 40 }"#;
        assert!(gain(low, 2.0) < 0.1);
        assert!(gain(low, 120.0) > 0.9);
        let high12 = r#"{ "band": "high_cut", "frequency": 20, "slope": "12" }"#;
        let high48 = r#"{ "band": "high_cut", "frequency": 20, "slope": "48" }"#;
        assert!(gain(high12, 120.0) < 0.1);
        assert!(gain(high48, 60.0) < gain(high12, 60.0) / 5.0);
    }

    #[test]
    fn a_notch_removes_its_centre_and_a_band_pass_keeps_only_it() {
        let notch = r#"{ "band": "notch", "frequency": 30, "q": 4 }"#;
        assert!(gain(notch, 30.0) < 0.05);
        assert!(gain(notch, 3.0) > 0.95);
        let pass = r#"{ "band": "band_pass", "frequency": 30, "q": 4 }"#;
        assert!((gain(pass, 30.0) - 1.0).abs() < 0.05);
        assert!(gain(pass, 3.0) < 0.2);
    }
}
