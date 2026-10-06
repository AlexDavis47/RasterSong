use crate::nodes::support::settle_frames;
use crate::nodes::{Category, NodeKind, NodeSpec, Unit};
use crate::{Node, ParamSpec, Params, PrepareContext, ProcessContext, Signal};

/// Slew limiter: the output follows the input but can only rise and fall at a set speed, which
/// rounds off jumps into ramps. Unlike a low pass, edges stay straight and the delay is bounded.
#[derive(Debug)]
pub struct Slew {
    rise: f64,
    fall: f64,
    unit: Unit,
    /// Largest change per sample, up and down, set in `prepare`. Infinite when the time is 0.
    up: f32,
    down: f32,
    /// Samples to travel a full 0 to 1 span at the slowest speed, for warmup.
    slowest: f64,
    level: f32,
}

params! { Slew {
    RISE: ParamSpec::number(
        "rise",
        "Rise",
        0.25,
        0.0,
        4.0,
        "Time to climb a full 0 to 1 when the input jumps up; 0 is instant",
    )
    .fixed()
    .limits(0.0, 1e6),
    FALL: ParamSpec::number(
        "fall",
        "Fall",
        0.25,
        0.0,
        4.0,
        "Time to fall a full 1 to 0 when the input jumps down; 0 is instant",
    )
    .fixed()
    .limits(0.0, 1e6),
    UNIT: Unit::time_param("row", "Unit for rise and fall"),
} }

impl NodeKind for Slew {
    const KIND: &'static str = "slew";
    const SPEC: NodeSpec = NodeSpec::new("Slew", Category::Effect)
        .describe("Limits how fast the signal can rise and fall, turning jumps into ramps")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "rise": 0.5, "fall": 0.1 }"#,
        r#"{ "rise": 0, "fall": 2 }"#,
        r#"{ "rise": 3, "fall": 0, "unit": "frame" }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            rise: params.number_at(Self::RISE)?,
            fall: params.number_at(Self::FALL)?,
            unit: params.choice_as(Self::UNIT)?,
            up: f32::INFINITY,
            down: f32::INFINITY,
            slowest: 0.0,
            level: 0.0,
        })
    }
}

impl Node for Slew {
    fn prepare(&mut self, ctx: &PrepareContext) {
        let unit = self.unit.samples(ctx);
        let (rise, fall) = (self.rise * unit, self.fall * unit);
        let speed = |samples: f64| {
            if samples >= 1.0 {
                (1.0 / samples) as f32
            } else {
                f32::INFINITY
            }
        };
        self.up = speed(rise);
        self.down = speed(fall);
        // Audio spans -1 to 1, twice the video range.
        self.slowest = 2.0 * rise.max(fall);
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let mut y = self.level;
        for (out, &x) in outputs[0].data.iter_mut().zip(&inputs[0].data) {
            let step = x - y;
            // Landing on the input exactly, not by adding a difference, keeps it free of rounding.
            y = if step > self.up {
                y + self.up
            } else if step < -self.down {
                y - self.down
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
