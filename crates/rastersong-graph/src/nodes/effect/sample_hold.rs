use crate::nodes::support::{SampleClock, settle_frames};
use crate::nodes::{Category, NodeKind, NodeSpec, Unit};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// Sample and hold with optional quantizing: every `period` the input is sampled and held until
/// the next one, which turns smooth signals into stairs (streaks in video, a sample-rate crush in
/// audio). The grid of sampling instants is counted from the start of the stream, so it doesn't
/// depend on where a render begins or how it is cut into blocks.
#[derive(Debug)]
pub struct SampleHold {
    period: f64,
    unit: Unit,
    levels: f32,
    /// Samples in one unit, and per hold at the constant period, set in `prepare`.
    unit_samples: f64,
    samples: f64,
    /// The longest a hold can be, for warmup.
    longest: f64,
    clock: SampleClock,
    /// Index of the hold interval last sampled.
    cell: Option<i64>,
    held: f32,
}

params! { SampleHold {
    PERIOD: ParamSpec::number("period", 0.25,
        0.0,
        4.0)
    .limits(0.0, 1e6),
    UNIT: Unit::time_param("row"),
    LEVELS: ParamSpec::number("levels", 0.0,
        0.0,
        32.0).integer()
    .exposed()
    .limits(0.0, 65_536.0),
} }

impl NodeKind for SampleHold {
    const KIND: &'static str = "sample_hold";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "period": 0.5 }"#,
        r#"{ "period": 2.5, "unit": "frame" }"#,
        r#"{ "period": 0, "levels": 4 }"#,
        r#"{ "period": 1.5, "levels": 8, "unit": "row" }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "period": 0.5, "levels": 8 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            period: params.number_at(Self::PERIOD)?,
            unit: params.choice_as(Self::UNIT)?,
            levels: params.float_at(Self::LEVELS)?,
            unit_samples: 1.0,
            samples: 0.0,
            longest: 0.0,
            clock: SampleClock::default(),
            cell: None,
            held: 0.0,
        })
    }
}

impl Node for SampleHold {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.unit_samples = self.unit.samples(ctx);
        self.samples = self.period * self.unit_samples;
        self.longest = ctx.param_max(Self::PERIOD, self.period) * self.unit_samples;
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let levels = ctx.value(Self::LEVELS, f64::from(self.levels));
        // A modulated period reads the grid with each sample's own hold length, so the same
        // position always gives the same held value, whatever the render started from.
        let period = ctx.param(Self::PERIOD);
        let start = self.clock.begin(ctx, inputs[0].data.len());
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let samples = period.map_or(self.samples, |p| {
                f64::from(p[i]).max(0.0) * self.unit_samples
            });
            let held = if samples > 1.0 {
                let cell = ((start + i as u64) as f64 / samples) as i64;
                if self.cell != Some(cell) {
                    self.cell = Some(cell);
                    self.held = x;
                }
                self.held
            } else {
                x
            };
            let n = levels.at(i).round();
            *out = if n >= 2.0 {
                (held * (n - 1.0)).round() / (n - 1.0)
            } else {
                held
            };
        }
    }

    fn reset(&mut self) {
        self.clock.reset();
        self.cell = None;
        self.held = 0.0;
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        // The value held at a seek position was sampled up to one period earlier.
        settle_frames(self.longest, ctx)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn run(params: &str, input: &[f32]) -> Vec<f32> {
        // One row is the whole block, so a period of 0.25 rows is a quarter of the block.
        let mut n = node(
            "sample_hold",
            params,
            input.len(),
            input.len() as f64,
            &[true],
        );
        process_one(n.as_mut(), &[input.to_vec()])
    }

    #[test]
    fn holds_each_sampled_value() {
        let input: Vec<f32> = (0..8).map(|i| i as f32).collect();
        let out = run(r#"{ "period": 0.25 }"#, &input);
        assert_eq!(out, [0.0, 0.0, 2.0, 2.0, 4.0, 4.0, 6.0, 6.0]);
    }

    #[test]
    fn zero_period_only_quantizes() {
        let out = run(r#"{ "period": 0, "levels": 3 }"#, &[0.1, 0.4, 0.6, 0.95]);
        assert_eq!(out, [0.0, 0.5, 0.5, 1.0]);
    }

    #[test]
    fn no_levels_and_no_period_is_a_passthrough() {
        let input = [0.3, 0.7, 0.1];
        assert_eq!(run(r#"{ "period": 0 }"#, &input), input);
    }
}
