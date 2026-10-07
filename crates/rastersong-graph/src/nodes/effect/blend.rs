use crate::dsp::mix;
use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, ProcessContext, Signal};

choice! {
    /// How two signals combine. The formulas assume video's `0..=1` range; on audio they still
    /// work, but `screen` and `overlay` treat 0 as black.
    pub enum Mode {
        /// `a + b`
        Add = "add",
        /// `a - b`
        Subtract = "subtract",
        /// `a × b`
        Multiply = "multiply",
        /// `1 - (1 - a)(1 - b)`: brightens, never past 1.
        Screen = "screen",
        /// `|a - b|`
        Difference = "difference",
        Min = "min",
        Max = "max",
        Average = "average",
        /// Multiplies where `a` is dark and screens where it is bright.
        Overlay = "overlay",
    }
}

impl Mode {
    pub fn apply(self, a: f32, b: f32) -> f32 {
        match self {
            Self::Add => a + b,
            Self::Subtract => a - b,
            Self::Multiply => a * b,
            Self::Screen => 1.0 - (1.0 - a) * (1.0 - b),
            Self::Difference => (a - b).abs(),
            Self::Min => a.min(b),
            Self::Max => a.max(b),
            Self::Average => (a + b) * 0.5,
            Self::Overlay => {
                if a < 0.5 {
                    2.0 * a * b
                } else {
                    1.0 - 2.0 * (1.0 - a) * (1.0 - b)
                }
            }
        }
    }
}

/// Combines two signals sample by sample with a blend mode. Stateless.
#[derive(Debug)]
pub struct Blend {
    mode: Mode,
    amount: f32,
}

params! { Blend {
    MODE: ParamSpec::choice("mode", Mode::OPTIONS, "add"),
    AMOUNT: ParamSpec::number("amount", 1.0,
        0.0,
        1.0)
    .exposed(),
} }

impl NodeKind for Blend {
    const KIND: &'static str = "blend";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .inputs(&[
            InputSpec::required("a"),
            InputSpec::required("b"),
        ])
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "mode": "multiply" }"#,
        r#"{ "mode": "screen", "amount": 0.5 }"#,
        r#"{ "mode": "difference" }"#,
        r#"{ "mode": "overlay", "amount": 0.8 }"#,
        r#"{ "mode": "min" }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "mode": "overlay", "amount": 0.8 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            mode: params.choice_as(Self::MODE)?,
            amount: params.float_at(Self::AMOUNT)?,
        })
    }
}

impl Node for Blend {
    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let amount = ctx.value(Self::AMOUNT, f64::from(self.amount));
        for (i, ((out, &a), &b)) in outputs[0]
            .data
            .iter_mut()
            .zip(&inputs[0].data)
            .zip(&inputs[1].data)
            .enumerate()
        {
            *out = mix(a, self.mode.apply(a, b), amount.at(i));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn blend(params: &str, a: &[f32], b: &[f32]) -> Vec<f32> {
        let mut node = node("blend", params, a.len(), a.len() as f64, &[true, true]);
        process_one(node.as_mut(), &[a.to_vec(), b.to_vec()])
    }

    #[test]
    fn modes_follow_their_formulas() {
        let (a, b) = ([0.2, 0.8], [0.5, 0.5]);
        let mode = |m: &str| blend(&format!(r#"{{ "mode": "{m}" }}"#), &a, &b);
        assert_eq!(mode("add"), [0.7, 1.3]);
        assert_eq!(mode("multiply"), [0.1, 0.4]);
        assert_eq!(mode("screen"), [0.6, 0.9]);
        assert_eq!(mode("min"), [0.2, 0.5]);
        assert_eq!(mode("max"), [0.5, 0.8]);
        assert_eq!(mode("average"), [0.35, 0.65]);
        assert_eq!(mode("overlay"), [0.2, 0.8]);
        let difference = mode("difference");
        assert!((difference[0] - 0.3).abs() < 1e-6 && (difference[1] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn amount_zero_passes_a_through() {
        let a = [0.1, 0.9];
        assert_eq!(blend(r#"{ "amount": 0 }"#, &a, &[1.0, 1.0]), a);
    }

    #[test]
    fn amount_crossfades_toward_the_blend() {
        let out = blend(r#"{ "mode": "add", "amount": 0.5 }"#, &[0.0], &[1.0]);
        assert_eq!(out, [0.5]);
    }
}
