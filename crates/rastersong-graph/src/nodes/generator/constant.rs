use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Layout, LayoutContext, Node, OutputSpec, ParamSpec, Params};
use crate::{ProcessContext, Range, Signal};

/// The same value in every sample. As video it is a flat colour (0 black, 1 white); as audio it is
/// silence at 0 or a DC offset. Unconnected inputs read one with the value 0.
#[derive(Debug)]
pub struct Constant {
    value: f32,
}

params! { Constant {
    VALUE: ParamSpec::number("value", "Value", 0.0, -1.0, 1.0, "The value of every sample")
        .limits(-10.0, 10.0),
} }

impl NodeKind for Constant {
    const KIND: &'static str = "constant";
    const SPEC: NodeSpec = NodeSpec::new("Constant", Category::Generator)
        .describe("The same value in every sample: a flat colour, or silence")
        .params(Self::PARAMS)
        .takes_layout()
        .inputs(&[])
        .outputs(&[OutputSpec::new("out", "The constant signal")]);
    const TEST_CONFIGS: &'static [&'static str] = &[r#"{ "value": 0.25 }"#, r#"{ "value": -1 }"#];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            value: params.float_at(Self::VALUE)?,
        })
    }
}

impl Node for Constant {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let nominal = ctx.layout.nominal();
        let value = f64::from(self.value);
        let fits = nominal
            .bounds()
            .is_some_and(|(lo, hi)| (lo..=hi).contains(&value));
        let range = if fits {
            nominal
        } else {
            Range::from_bounds(value, value)
        };
        ctx.layout.output_layouts(ctx, range)
    }

    fn process(&mut self, ctx: &ProcessContext, _inputs: &[&Signal], outputs: &mut [Signal]) {
        let value = ctx.value(Self::VALUE, f64::from(self.value));
        for (i, out) in outputs[0].data.iter_mut().enumerate() {
            *out = value.at(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    #[test]
    fn fills_the_signal_with_its_value() {
        let mut constant = node("constant", r#"{ "value": 0.25 }"#, 6, 6.0, &[]);
        assert_eq!(process_one(constant.as_mut(), &[vec![0.0; 6]]), [0.25; 6]);
        let mut zero = node("constant", "{}", 3, 3.0, &[]);
        assert_eq!(process_one(zero.as_mut(), &[vec![1.0; 3]]), [0.0; 3]);
    }
}
