use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Layout, LayoutContext, Node, ParamSpec, Params, ProcessContext, Range, Signal};

/// Adds a constant to the signal. Stateless.
#[derive(Debug)]
pub struct Offset {
    amount: f32,
}

params! { Offset {
    AMOUNT: ParamSpec::number(
        "amount",
        "Amount",
        0.0,
        -1.0,
        1.0,
        "Added to every sample: brightens video, shifts audio up",
    )
    .exposed()
    .limits(-100.0, 100.0),
} }

impl NodeKind for Offset {
    const KIND: &'static str = "offset";
    const SPEC: NodeSpec = NodeSpec::new("Offset", Category::Effect)
        .describe("Adds a constant to every sample")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] =
        &[r#"{ "amount": 0.25 }"#, r#"{ "amount": -1 }"#];
    const BENCH: Option<&'static str> = Some(r#"{ "amount": 0.25 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            amount: params.float_at(Self::AMOUNT)?,
        })
    }
}

impl Node for Offset {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        // The input's nominal range moves with the offset.
        let mut layout = ctx.inputs[0];
        if let Some((lo, hi)) = layout.tag.range.bounds() {
            let amount = f64::from(self.amount);
            layout.tag.range = Range::from_bounds(lo + amount, hi + amount);
        }
        Ok(vec![layout])
    }

    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let amount = ctx.value(Self::AMOUNT, f64::from(self.amount));
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            *out = x + amount.at(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    #[test]
    fn adds_the_amount() {
        let mut n = node("offset", r#"{ "amount": 0.5 }"#, 3, 3.0, &[true]);
        assert_eq!(
            process_one(n.as_mut(), &[vec![0.0, 0.25, -1.0]]),
            [0.5, 0.75, -0.5]
        );
    }
}
