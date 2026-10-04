use std::f64::consts::PI;

use crate::dsp::{Biquad, BiquadKind};
use crate::nodes::support::MAX_WARMUP_FRAMES;
use crate::nodes::{Category, FreqUnit, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// A three-band parametric equaliser: a low shelf, a peaking mid band and a high shelf, each
/// boosting or cutting by its gain in dB. Not to be confused with Three Band, which splits the
/// signal into three outputs.
#[derive(Debug)]
pub struct Equalizer {
    unit: FreqUnit,
    low_freq: f64,
    low_gain: f64,
    mid_freq: f64,
    mid_gain: f64,
    mid_q: f64,
    high_freq: f64,
    high_gain: f64,
    /// Set in `prepare`: cycles per sample for one unit, and whether any parameter is modulated.
    per_sample: f64,
    modulated: bool,
    /// Slowest cutoff in cycles per sample, for warmup.
    slowest: f64,
    bands: [Biquad; 3],
}

params! { Equalizer {
    UNIT: FreqUnit::param("Row", "Unit for the three frequencies"),
    LOW_FREQ: ParamSpec::number("low_freq", "Low freq", 5.0, 0.01, 1000.0, "Corner of the low shelf")
        .limits(1e-06, 1e9)
        .octaves(),
    LOW_GAIN: ParamSpec::number("low_gain", "Low gain", 0.0, -24.0, 24.0, "Boost or cut of everything below the low corner")
        .unit("dB")
        .exposed()
        .limits(-48.0, 48.0),
    MID_FREQ: ParamSpec::number("mid_freq", "Mid freq", 30.0, 0.01, 1000.0, "Centre of the mid band")
        .limits(1e-06, 1e9)
        .octaves(),
    MID_GAIN: ParamSpec::number("mid_gain", "Mid gain", 0.0, -24.0, 24.0, "Boost or cut around the mid frequency")
        .unit("dB")
        .exposed()
        .limits(-48.0, 48.0),
    MID_Q: ParamSpec::number("mid_q", "Mid Q", 1.0, 0.1, 20.0, "Width of the mid band: higher is narrower")
        .limits(0.05, 100.0),
    HIGH_FREQ: ParamSpec::number("high_freq", "High freq", 150.0, 0.01, 1000.0, "Corner of the high shelf")
        .limits(1e-06, 1e9)
        .octaves(),
    HIGH_GAIN: ParamSpec::number("high_gain", "High gain", 0.0, -24.0, 24.0, "Boost or cut of everything above the high corner")
        .unit("dB")
        .exposed()
        .limits(-48.0, 48.0),
} }

impl NodeKind for Equalizer {
    const KIND: &'static str = "equalizer";
    const SPEC: NodeSpec = NodeSpec::new("Equalizer", Category::Effect)
        .describe("Boosts or cuts low, mid and high ranges with a shelf, a peak and a shelf")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "low_gain": 6, "mid_gain": -9, "high_gain": 4 }"#,
        r#"{ "low_freq": 0.3, "mid_freq": 0.9, "high_freq": 1.5, "mid_gain": 12, "mid_q": 6 }"#,
    ];
    const BENCH: Option<&'static str> =
        Some(r#"{ "low_gain": 6, "mid_gain": -6, "high_gain": 3 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            unit: params.choice_as(Self::UNIT)?,
            low_freq: params.number_at(Self::LOW_FREQ)?,
            low_gain: params.number_at(Self::LOW_GAIN)?,
            mid_freq: params.number_at(Self::MID_FREQ)?,
            mid_gain: params.number_at(Self::MID_GAIN)?,
            mid_q: params.number_at(Self::MID_Q)?,
            high_freq: params.number_at(Self::HIGH_FREQ)?,
            high_gain: params.number_at(Self::HIGH_GAIN)?,
            per_sample: 0.0,
            modulated: false,
            slowest: 0.0,
            bands: [Biquad::default(); 3],
        })
    }
}

impl Equalizer {
    /// The three bands for the given frequencies (in this node's unit) and gains (dB).
    fn design(&self, freq: [f64; 3], gain: [f64; 3]) -> [Biquad; 3] {
        let f = |i: usize| freq[i] * self.per_sample;
        let shelf_q = std::f64::consts::FRAC_1_SQRT_2;
        [
            Biquad::design(BiquadKind::LowShelf { gain_db: gain[0] }, f(0), shelf_q),
            Biquad::design(BiquadKind::Peak { gain_db: gain[1] }, f(1), self.mid_q),
            Biquad::design(BiquadKind::HighShelf { gain_db: gain[2] }, f(2), shelf_q),
        ]
    }

    fn base(&self) -> ([f64; 3], [f64; 3]) {
        (
            [self.low_freq, self.mid_freq, self.high_freq],
            [self.low_gain, self.mid_gain, self.high_gain],
        )
    }
}

impl Node for Equalizer {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.per_sample = self.unit.per_sample(1.0, ctx);
        let params = [
            Self::LOW_FREQ,
            Self::LOW_GAIN,
            Self::MID_FREQ,
            Self::MID_GAIN,
            Self::HIGH_FREQ,
            Self::HIGH_GAIN,
        ];
        self.modulated = params.iter().any(|&p| ctx.modulation(p).is_some());
        let slowest = ctx
            .param_min(Self::LOW_FREQ, self.low_freq)
            .min(ctx.param_min(Self::MID_FREQ, self.mid_freq))
            .min(ctx.param_min(Self::HIGH_FREQ, self.high_freq));
        self.slowest = slowest * self.per_sample;
        let (freq, gain) = self.base();
        self.bands = self.design(freq, gain);
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let freq = [
            ctx.value(Self::LOW_FREQ, self.low_freq),
            ctx.value(Self::MID_FREQ, self.mid_freq),
            ctx.value(Self::HIGH_FREQ, self.high_freq),
        ];
        let gain = [
            ctx.value(Self::LOW_GAIN, self.low_gain),
            ctx.value(Self::MID_GAIN, self.mid_gain),
            ctx.value(Self::HIGH_GAIN, self.high_gain),
        ];
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            if self.modulated {
                let designed = self.design(
                    [freq[0].at64(i), freq[1].at64(i), freq[2].at64(i)],
                    [gain[0].at64(i), gain[1].at64(i), gain[2].at64(i)],
                );
                for (band, new) in self.bands.iter_mut().zip(designed) {
                    band.retune(new);
                }
            }
            let mut y = f64::from(x);
            for band in &mut self.bands {
                y = band.process(y);
            }
            *out = y as f32;
        }
    }

    fn reset(&mut self) {
        for band in &mut self.bands {
            band.reset();
        }
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        let samples = 7.0 * self.mid_q.max(1.0) / (PI * self.slowest.max(1e-9));
        if samples.is_finite() {
            ((samples / ctx.samples_per_frame() as f64).ceil() as u32).clamp(1, MAX_WARMUP_FRAMES)
        } else {
            MAX_WARMUP_FRAMES
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
        process_one(node.as_mut(), std::slice::from_ref(&sine));
        let out = process_one(node.as_mut(), &[sine]);
        out.iter().fold(0.0f32, |peak, &x| peak.max(x.abs()))
    }

    #[test]
    fn flat_settings_pass_everything() {
        for cycles in [1.0, 20.0, 100.0] {
            assert!((gain("{}", cycles) - 1.0).abs() < 0.01, "{cycles}");
        }
    }

    #[test]
    fn each_band_moves_its_own_range() {
        let p = r#"{ "low_freq": 4, "mid_freq": 30, "high_freq": 100, "low_gain": 12, "mid_gain": -12, "high_gain": 12 }"#;
        assert!(gain(p, 0.5) > 3.5, "low shelf boosts, +12 dB is x4");
        assert!(gain(p, 30.0) < 0.3, "mid band cuts");
        assert!(gain(p, 120.0) > 3.0, "high shelf boosts");
    }
}
