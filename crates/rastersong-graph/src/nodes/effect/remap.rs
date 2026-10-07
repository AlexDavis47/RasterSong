use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Layout, LayoutContext, Node, ParamSpec, Params, ProcessContext, Range, Signal};

choice! {
    /// What happens to samples outside the input range.
    pub enum Outside {
        /// They stop at the ends of the output range.
        Clamp = "clamp",
        /// They continue the line past the output range.
        Extend = "extend",
    }
}

/// Maps the range `in_low..in_high` onto `out_low..out_high`. Stateless.
#[derive(Debug)]
pub struct Remap {
    in_low: f32,
    in_high: f32,
    out_low: f32,
    out_high: f32,
    outside: Outside,
}

params! { Remap {
    IN_LOW: ParamSpec::number("in_low", 0.0, -1.0, 1.0)
        .exposed()
        .limits(-100.0, 100.0),
    IN_HIGH: ParamSpec::number("in_high", 1.0, -1.0, 1.0)
        .exposed()
        .limits(-100.0, 100.0),
    OUT_LOW: ParamSpec::number("out_low", 0.0, -1.0, 1.0)
        .limits(-100.0, 100.0),
    OUT_HIGH: ParamSpec::number("out_high", 1.0, -1.0, 1.0)
        .limits(-100.0, 100.0),
    OUTSIDE: ParamSpec::choice("outside", Outside::OPTIONS, "clamp"),
} }

impl NodeKind for Remap {
    const KIND: &'static str = "remap";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "in_low": 0.2, "in_high": 0.8 }"#,
        r#"{ "out_low": 1, "out_high": 0 }"#,
        r#"{ "in_low": -1, "in_high": 1, "out_low": 0, "out_high": 1, "outside": "extend" }"#,
        r#"{ "in_low": 0.5, "in_high": 0.5 }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "in_low": 0.2, "in_high": 0.8 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            in_low: params.float_at(Self::IN_LOW)?,
            in_high: params.float_at(Self::IN_HIGH)?,
            out_low: params.float_at(Self::OUT_LOW)?,
            out_high: params.float_at(Self::OUT_HIGH)?,
            outside: params.choice_as(Self::OUTSIDE)?,
        })
    }
}

impl Remap {
    #[inline]
    fn map(&self, x: f32, in_low: f32, in_high: f32, out_low: f32, out_high: f32) -> f32 {
        let span = in_high - in_low;
        // A zero-width input range is a step at that value.
        let t = if span.abs() < 1e-12 {
            if x >= in_low { 1.0 } else { 0.0 }
        } else {
            (x - in_low) / span
        };
        let t = match self.outside {
            Outside::Clamp => t.clamp(0.0, 1.0),
            Outside::Extend => t,
        };
        out_low + (out_high - out_low) * t
    }
}

impl Node for Remap {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let mut layout = ctx.inputs[0];
        let out = (f64::from(self.out_low), f64::from(self.out_high));
        layout.tag.range = match (self.outside, layout.tag.range.bounds()) {
            (Outside::Clamp, _) => Range::from_bounds(out.0, out.1),
            // Extended, the input's nominal range maps through the same line.
            (Outside::Extend, Some((lo, hi))) => {
                let at = |x: f64| {
                    f64::from(self.map(
                        x as f32,
                        self.in_low,
                        self.in_high,
                        self.out_low,
                        self.out_high,
                    ))
                };
                Range::from_bounds(at(lo), at(hi))
            }
            (Outside::Extend, None) => Range::Unknown,
        };
        Ok(vec![layout])
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let in_low = ctx.value(Self::IN_LOW, f64::from(self.in_low));
        let in_high = ctx.value(Self::IN_HIGH, f64::from(self.in_high));
        let out_low = ctx.value(Self::OUT_LOW, f64::from(self.out_low));
        let out_high = ctx.value(Self::OUT_HIGH, f64::from(self.out_high));
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            *out = self.map(
                x,
                in_low.at(i),
                in_high.at(i),
                out_low.at(i),
                out_high.at(i),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn remap(params: &str, input: &[f32]) -> Vec<f32> {
        let mut n = node("remap", params, input.len(), input.len() as f64, &[true]);
        process_one(n.as_mut(), &[input.to_vec()])
    }

    #[test]
    fn defaults_are_the_identity_inside_the_range() {
        let input = [0.0, 0.3, 1.0];
        assert_eq!(remap("{}", &input), input);
    }

    #[test]
    fn stretches_a_range_and_clamps_outside() {
        let out = remap(r#"{ "in_low": 0.25, "in_high": 0.75 }"#, &[0.0, 0.5, 1.0]);
        assert_eq!(out, [0.0, 0.5, 1.0]);
        let out = remap(r#"{ "in_low": 0.25, "in_high": 0.75 }"#, &[0.25, 0.375]);
        assert_eq!(out, [0.0, 0.25]);
    }

    #[test]
    fn swapped_output_range_inverts() {
        let out = remap(r#"{ "out_low": 1, "out_high": 0 }"#, &[0.0, 0.25, 1.0]);
        assert_eq!(out, [1.0, 0.75, 0.0]);
    }

    #[test]
    fn extend_continues_past_the_ends() {
        let out = remap(r#"{ "outside": "extend", "out_high": 2 }"#, &[0.5, 1.5]);
        assert_eq!(out, [1.0, 3.0]);
    }

    #[test]
    fn a_zero_width_input_range_is_a_step() {
        let out = remap(r#"{ "in_low": 0.5, "in_high": 0.5 }"#, &[0.4, 0.5, 0.6]);
        assert_eq!(out, [0.0, 1.0, 1.0]);
    }
}
