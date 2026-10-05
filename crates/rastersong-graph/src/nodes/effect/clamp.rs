use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, ProcessContext, Signal};

/// Limits every sample to a range. Stateless.
#[derive(Debug)]
pub struct Clamp {
    min: f32,
    max: f32,
}

params! { Clamp {
    MIN: ParamSpec::number("min", "Min", 0.0, -1.0, 1.0, "Samples below this are raised to it")
        .exposed()
        .limits(-100.0, 100.0),
    MAX: ParamSpec::number("max", "Max", 1.0, -1.0, 1.0, "Samples above this are lowered to it")
        .exposed()
        .limits(-100.0, 100.0),
} }

impl NodeKind for Clamp {
    const KIND: &'static str = "clamp";
    const SPEC: NodeSpec = NodeSpec::new("Clamp", Category::Effect)
        .describe("Limits every sample to a range")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "min": 0.2, "max": 0.8 }"#,
        r#"{ "min": -1, "max": 1 }"#,
        r#"{ "min": 0.9, "max": 0.1 }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "min": 0.2, "max": 0.8 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            min: params.float_at(Self::MIN)?,
            max: params.float_at(Self::MAX)?,
        })
    }
}

impl Node for Clamp {
    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let min = ctx.value(Self::MIN, f64::from(self.min));
        let max = ctx.value(Self::MAX, f64::from(self.max));
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            // If the bounds cross, the range collapses to the lower bound.
            let (lo, hi) = (min.at(i), max.at(i));
            *out = x.max(lo).min(hi.max(lo));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn clamp(params: &str, input: &[f32]) -> Vec<f32> {
        let mut n = node("clamp", params, input.len(), input.len() as f64, &[true]);
        process_one(n.as_mut(), &[input.to_vec()])
    }

    #[test]
    fn limits_to_the_range() {
        let out = clamp(r#"{ "min": 0.2, "max": 0.8 }"#, &[0.0, 0.5, 1.0]);
        assert_eq!(out, [0.2, 0.5, 0.8]);
    }

    #[test]
    fn crossed_bounds_collapse_to_the_minimum() {
        let out = clamp(r#"{ "min": 0.6, "max": 0.3 }"#, &[0.0, 0.5, 1.0]);
        assert_eq!(out, [0.6, 0.6, 0.6]);
    }
}
