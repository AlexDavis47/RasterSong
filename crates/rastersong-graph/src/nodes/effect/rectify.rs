use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, ProcessContext, Signal};

choice! {
    /// Which half of the wave survives.
    pub enum Kind {
        /// `|x|`: negative parts flip up.
        Full = "full",
        /// Negative parts become 0.
        Half = "half",
        /// Positive parts become 0; the negative half is kept.
        Negative = "negative",
    }
}

/// Rectifies the signal around a centre. Stateless.
#[derive(Debug)]
pub struct Rectify {
    kind: Kind,
    center: f32,
}

params! { Rectify {
    KIND: ParamSpec::choice(
        "kind",
        "Kind",
        Kind::OPTIONS,
        "full",
        "full flips the negative half up, half drops it, negative keeps only the negative half",
    ),
    CENTER: ParamSpec::number(
        "center",
        "Center",
        0.0,
        -1.0,
        1.0,
        "The value the wave is rectified around: 0 for audio, 0.5 to fold video around mid-gray",
    )
    .exposed()
    .limits(-100.0, 100.0),
} }

impl NodeKind for Rectify {
    const KIND: &'static str = "rectify";
    const SPEC: NodeSpec = NodeSpec::new("Rectify", Category::Effect)
        .describe("Folds or drops one half of the wave around a centre value")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "kind": "half" }"#,
        r#"{ "kind": "negative", "center": 0.2 }"#,
        r#"{ "kind": "full", "center": 0.5 }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            kind: params.choice_as(Self::KIND)?,
            center: params.float_at(Self::CENTER)?,
        })
    }
}

impl Node for Rectify {
    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let center = ctx.value(Self::CENTER, f64::from(self.center));
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let c = center.at(i);
            let d = x - c;
            *out = c + match self.kind {
                Kind::Full => d.abs(),
                Kind::Half => d.max(0.0),
                Kind::Negative => d.min(0.0),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn rectify(params: &str, input: &[f32]) -> Vec<f32> {
        let mut n = node("rectify", params, input.len(), input.len() as f64, &[true]);
        process_one(n.as_mut(), &[input.to_vec()])
    }

    #[test]
    fn full_flips_the_negative_half() {
        assert_eq!(rectify("{}", &[-0.5, 0.25]), [0.5, 0.25]);
    }

    #[test]
    fn half_and_negative_keep_one_side() {
        assert_eq!(rectify(r#"{ "kind": "half" }"#, &[-0.5, 0.25]), [0.0, 0.25]);
        assert_eq!(
            rectify(r#"{ "kind": "negative" }"#, &[-0.5, 0.25]),
            [-0.5, 0.0]
        );
    }

    #[test]
    fn the_centre_moves_the_fold() {
        let out = rectify(r#"{ "center": 0.5 }"#, &[0.25, 0.75, 1.0]);
        assert_eq!(out, [0.75, 0.75, 1.0]);
    }
}
