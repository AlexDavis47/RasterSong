use crate::dsp::db_to_gain;
use crate::nodes::{Category, Meter, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, ProcessContext, Signal};

/// Scales the signal by a gain in decibels. Stateless.
#[derive(Debug)]
pub struct Gain {
    gain: f32,
    /// The largest output sample of the last frame, for the meter.
    peak: f32,
}

params! { Gain {
    GAIN: ParamSpec::number("gain", 0.0,
        -48.0,
        24.0)
    .unit("dB")
    .exposed()
    .limits(-120.0, 120.0),
} }

impl NodeKind for Gain {
    const KIND: &'static str = "gain";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .per_channel()
        .meters(&[Meter::level("peak")]);
    const TEST_CONFIGS: &'static [&'static str] = &[r#"{ "gain": 6 }"#, r#"{ "gain": -20 }"#];
    const BENCH: Option<&'static str> = Some(r#"{ "gain": 6 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            gain: db_to_gain(params.number_at(Self::GAIN)?) as f32,
            peak: 0.0,
        })
    }
}

impl Node for Gain {
    fn meters(&self, out: &mut [f32]) {
        out[0] = self.peak;
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let gain = ctx.value(Self::GAIN, f64::from(self.gain));
        let mut peak = 0.0f32;
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            // The constant is already a gain; a modulating signal is in dB.
            *out = x * gain.at_with(i, |db| db_to_gain(f64::from(db)) as f32);
            peak = peak.max(out.abs());
        }
        self.peak = peak;
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn gain(db: f64, input: &[f32]) -> Vec<f32> {
        let mut node = node(
            "gain",
            &format!(r#"{{ "gain": {db} }}"#),
            input.len(),
            input.len() as f64,
            &[true],
        );
        process_one(node.as_mut(), &[input.to_vec()])
    }

    #[test]
    fn zero_db_changes_nothing() {
        let input = [0.25, -0.5, 1.0];
        assert_eq!(gain(0.0, &input), input);
    }

    #[test]
    fn six_db_roughly_doubles() {
        let out = gain(6.0, &[0.5]);
        assert!((out[0] - 0.5 * 1.995_262_3).abs() < 1e-5);
    }

    #[test]
    fn minus_twenty_db_is_a_tenth() {
        assert!((gain(-20.0, &[1.0])[0] - 0.1).abs() < 1e-6);
    }
}
