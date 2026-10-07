use crate::dsp::{DelayLine, mix};
use crate::nodes::{Category, NodeKind, NodeSpec, Unit};
use crate::{InputSpec, Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// Frequency (phase) modulation: the carrier is read back through a delay whose length follows
/// the modulator, `delay = index × (1 + modulator)`. Where the modulator is high, the carrier is
/// read from further in the past, which bends and warps it like FM synthesis does.
#[derive(Debug)]
pub struct Fm {
    index: f64,
    unit: Unit,
    mix: f32,
    /// Set in `prepare`.
    unit_samples: f64,
    max_delay: f64,
    line: DelayLine,
}

params! { Fm {
    INDEX: ParamSpec::number("index", 0.5,
        0.0,
        10.0)
    .exposed()
    .limits(0.0, 1000.0),
    UNIT: Unit::time_param("row"),
    MIX: ParamSpec::mix(),
} }

impl NodeKind for Fm {
    const KIND: &'static str = "fm";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .inputs(&[
            InputSpec::required("carrier"),
            InputSpec::required("modulator"),
        ])
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "index": 0.7 }"#,
        r#"{ "index": 0.25, "unit": "frame", "mix": 0.6 }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "index": 0.5 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            index: params.number_at(Self::INDEX)?,
            unit: params.choice_as(Self::UNIT)?,
            mix: params.float_at(Self::MIX)?,
            unit_samples: 0.0,
            max_delay: 0.0,
            line: DelayLine::default(),
        })
    }
}

impl Node for Fm {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.unit_samples = self.unit.samples(ctx);
        // The modulator is nominally within ±1, but a louder one is clamped to a reachable delay.
        self.max_delay = 2.0 * ctx.param_max(Self::INDEX, self.index) * self.unit_samples;
        self.line = DelayLine::new(self.max_delay.ceil() as usize + 1);
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let index = ctx.value(Self::INDEX, self.index);
        let amount = ctx.value(Self::MIX, f64::from(self.mix));
        for (i, ((out, &c), &m)) in outputs[0]
            .data
            .iter_mut()
            .zip(&inputs[0].data)
            .zip(&inputs[1].data)
            .enumerate()
        {
            self.line.push(c);
            let delay = (index.at64(i) * self.unit_samples * (1.0 + f64::from(m))).max(0.0);
            *out = mix(c, self.line.read(delay), amount.at(i));
        }
    }

    fn reset(&mut self) {
        self.line.reset();
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        (self.max_delay / ctx.samples_per_frame() as f64).ceil() as u32
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn fm(params: &str, carrier: &[f32], modulator: &[f32]) -> Vec<f32> {
        let len = carrier.len();
        let mut node = node("fm", params, len, len as f64, &[true, true]);
        process_one(node.as_mut(), &[carrier.to_vec(), modulator.to_vec()])
    }

    #[test]
    fn a_silent_modulator_delays_by_the_index() {
        // The block is one row; an index of 0.125 rows is 1 sample, and a modulator of 0 gives 1×.
        let carrier: Vec<f32> = (1..=8).map(|i| i as f32).collect();
        let out = fm(r#"{ "index": 0.125 }"#, &carrier, &[0.0; 8]);
        assert_eq!(out, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
    }

    #[test]
    fn a_modulator_of_minus_one_reads_the_present() {
        let carrier = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(fm(r#"{ "index": 1 }"#, &carrier, &[-1.0; 4]), carrier);
    }

    #[test]
    fn index_zero_is_the_dry_carrier() {
        let carrier = [0.3, -0.4, 0.5];
        assert_eq!(
            fm(r#"{ "index": 0 }"#, &carrier, &[1.0, 0.5, -0.2]),
            carrier
        );
    }
}
