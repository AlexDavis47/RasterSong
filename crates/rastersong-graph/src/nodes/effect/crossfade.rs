use crate::dsp::mix;
use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, ProcessContext, Signal};

choice! {
    /// How `position` moves between the two signals.
    pub enum Curve {
        /// A smooth blend: 0 is all `a`, 1 is all `b`.
        Fade = "fade",
        /// A hard cut: `a` below 0.5, `b` from 0.5 up.
        Switch = "switch",
    }
}

/// Crossfades or switches between two signals. Stateless.
#[derive(Debug)]
pub struct Crossfade {
    curve: Curve,
    position: f32,
}

params! { Crossfade {
    CURVE: ParamSpec::choice("curve", Curve::OPTIONS, "fade"),
    POSITION: ParamSpec::number("position", 0.5,
        0.0,
        1.0)
    .exposed(),
} }

impl NodeKind for Crossfade {
    const KIND: &'static str = "crossfade";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .inputs(&[InputSpec::required("a"), InputSpec::required("b")])
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "position": 0.25 }"#,
        r#"{ "curve": "switch", "position": 0.7 }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "position": 0.25 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            curve: params.choice_as(Self::CURVE)?,
            position: params.float_at(Self::POSITION)?,
        })
    }
}

impl Node for Crossfade {
    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let position = ctx.value(Self::POSITION, f64::from(self.position));
        for (i, ((out, &a), &b)) in outputs[0]
            .data
            .iter_mut()
            .zip(&inputs[0].data)
            .zip(&inputs[1].data)
            .enumerate()
        {
            let p = position.at(i).clamp(0.0, 1.0);
            *out = match self.curve {
                Curve::Fade => mix(a, b, p),
                Curve::Switch => {
                    if p < 0.5 {
                        a
                    } else {
                        b
                    }
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn crossfade(params: &str, a: &[f32], b: &[f32]) -> Vec<f32> {
        let mut n = node("crossfade", params, a.len(), a.len() as f64, &[true, true]);
        process_one(n.as_mut(), &[a.to_vec(), b.to_vec()])
    }

    #[test]
    fn fade_blends() {
        let out = crossfade(r#"{ "position": 0.25 }"#, &[0.0, 1.0], &[1.0, 0.0]);
        assert_eq!(out, [0.25, 0.75]);
    }

    #[test]
    fn the_ends_are_one_signal() {
        let (a, b) = ([0.1, 0.2], [0.8, 0.9]);
        assert_eq!(crossfade(r#"{ "position": 0 }"#, &a, &b), a);
        assert_eq!(crossfade(r#"{ "position": 1 }"#, &a, &b), b);
    }

    #[test]
    fn switch_cuts_at_the_middle() {
        let (a, b) = ([0.1], [0.9]);
        let cut = |p: &str| {
            crossfade(
                &format!(r#"{{ "curve": "switch", "position": {p} }}"#),
                &a,
                &b,
            )
        };
        assert_eq!(cut("0.49"), a);
        assert_eq!(cut("0.5"), b);
    }
}
