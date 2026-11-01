use crate::nodes::support::settle_frames;
use crate::nodes::{Category, Measure, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// Slew limiter: the output follows the input but can only rise and fall at a set speed, which
/// rounds off jumps into ramps. Unlike a low pass, edges stay straight and the delay is bounded.
#[derive(Debug)]
pub struct Slew {
    rise: f64,
    fall: f64,
    rise_unit: Measure,
    fall_unit: Measure,
    /// The largest change per sample, up and down, of the constant
    /// times: both set in `prepare`. Infinite when the time is 0.
    up: f32,
    down: f32,
    /// Samples to travel a full 0 to 1 span at the slowest speed, for warmup.
    slowest: f64,
    level: f32,
}

params! { Slew {
    RISE: ParamSpec::number("rise", 0.25,
        0.0,
        4.0)
    .length("row")
    .limits(0.0, 1e6),
    FALL: ParamSpec::number("fall", 0.25,
        0.0,
        4.0)
    .length("row")
    .limits(0.0, 1e6),
} }

impl NodeKind for Slew {
    const KIND: &'static str = "slew";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "rise": 0.5, "fall": 0.1 }"#,
        r#"{ "rise": 0, "fall": 2 }"#,
        r#"{ "rise": 3, "fall": 0, "rise.unit": "frame", "fall.unit": "frame" }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            rise: params.number_at(Self::RISE)?,
            fall: params.number_at(Self::FALL)?,
            rise_unit: params.measure_at(Self::RISE)?,
            fall_unit: params.measure_at(Self::FALL)?,
            up: f32::INFINITY,
            down: f32::INFINITY,
            slowest: 0.0,
            level: 0.0,
        })
    }
}

/// The largest change per sample for a time of `samples` to travel a full span: unlimited when
/// it is under a sample.
fn speed(samples: f64) -> f32 {
    if samples >= 1.0 {
        (1.0 / samples) as f32
    } else {
        f32::INFINITY
    }
}

impl Node for Slew {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.rise_unit.prepare(ctx);
        self.fall_unit.prepare(ctx);
        self.up = speed(self.rise_unit.samples(self.rise));
        self.down = speed(self.fall_unit.samples(self.fall));
        // Audio spans -1 to 1, twice the video range; the times may be as long as a signal makes them.
        self.slowest = 2.0
            * self
                .rise_unit
                .samples(ctx.param_max(Self::RISE, self.rise))
                .max(self.fall_unit.samples(ctx.param_max(Self::FALL, self.fall)));
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let mut y = self.level;
        let (rise, fall) = (ctx.param(Self::RISE), ctx.param(Self::FALL));
        let limit = |stream: Option<&[f32]>, unit: &Measure, i: usize, constant: f32| {
            stream.map_or(constant, |s| speed(unit.samples(f64::from(s[i]).max(0.0))))
        };
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let (up, down) = (
                limit(rise, &self.rise_unit, i, self.up),
                limit(fall, &self.fall_unit, i, self.down),
            );
            let step = x - y;
            // Landing on the input exactly, not by adding a difference, keeps it free of rounding.
            y = if step > up {
                y + up
            } else if step < -down {
                y - down
            } else {
                x
            };
            *out = y;
        }
        self.level = y;
    }

    fn reset(&mut self) {
        self.level = 0.0;
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        settle_frames(self.slowest, ctx)
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn slew(params: &str, input: &[f32]) -> Vec<f32> {
        // One row is the whole block.
        let mut n = node("slew", params, input.len(), input.len() as f64, &[true]);
        process_one(n.as_mut(), &[input.to_vec()])
    }

    #[test]
    fn a_step_becomes_a_ramp_of_the_set_length() {
        // Rising a full unit takes 4 samples (0.5 rows of an 8 sample block).
        let out = slew(r#"{ "rise": 0.5, "fall": 0 }"#, &[1.0; 8]);
        assert_eq!(out, [0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn fall_and_rise_are_independent() {
        let out = slew(
            r#"{ "rise": 0, "fall": 0.5 }"#,
            &[1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        assert_eq!(out, [1.0, 1.0, 0.75, 0.5, 0.25, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn zero_times_pass_the_signal_through() {
        let input = [0.9, 0.1, 0.5];
        assert_eq!(slew(r#"{ "rise": 0, "fall": 0 }"#, &input), input);
    }
}
