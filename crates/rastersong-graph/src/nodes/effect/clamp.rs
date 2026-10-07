use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Layout, LayoutContext, Node, ParamSpec, Params, ProcessContext, Range, Signal};

/// Limits every sample to a range. Stateless.
#[derive(Debug)]
pub struct Clamp {
    min: f32,
    max: f32,
}

params! { Clamp {
    MIN: ParamSpec::number("min", 0.0, -1.0, 1.0)
        .exposed()
        .limits(-100.0, 100.0),
    MAX: ParamSpec::number("max", 1.0, -1.0, 1.0)
        .exposed()
        .limits(-100.0, 100.0),
} }

impl NodeKind for Clamp {
    const KIND: &'static str = "clamp";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
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
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        // What survives is the clamp range, narrowed by the input's nominal range.
        let mut layout = ctx.inputs[0];
        let (mut lo, mut hi) = (f64::from(self.min), f64::from(self.max));
        if let Some((in_lo, in_hi)) = layout.tag.range.bounds() {
            lo = lo.max(in_lo);
            hi = hi.min(in_hi);
        }
        layout.tag.range = Range::from_bounds(lo, hi.max(lo));
        Ok(vec![layout])
    }

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
