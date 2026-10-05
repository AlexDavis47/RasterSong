use crate::dsp::mix;
use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, ProcessContext, Signal};

choice! {
    /// What the signal is flipped around.
    pub enum Mode {
        /// `1 - x`: black becomes white. For video.
        Video = "video",
        /// `-x`: flips the wave upside down. For audio.
        Audio = "audio",
    }
}

/// Inverts the signal. Stateless.
#[derive(Debug)]
pub struct Invert {
    mode: Mode,
    amount: f32,
}

params! { Invert {
    MODE: ParamSpec::choice(
        "mode",
        "Mode",
        Mode::OPTIONS,
        "video",
        "video flips around the middle of 0 to 1 (1 - x), audio flips the sign (-x)",
    ),
    AMOUNT: ParamSpec::number(
        "amount",
        "Amount",
        1.0,
        0.0,
        1.0,
        "0 leaves the signal alone, 1 is fully inverted, in between fades toward it",
    )
    .exposed(),
} }

impl NodeKind for Invert {
    const KIND: &'static str = "invert";
    const SPEC: NodeSpec = NodeSpec::new("Invert", Category::Effect)
        .describe("Flips the signal: negative for video, upside down for audio")
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "mode": "audio" }"#,
        r#"{ "mode": "video", "amount": 0.4 }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            mode: params.choice_as(Self::MODE)?,
            amount: params.float_at(Self::AMOUNT)?,
        })
    }
}

impl Node for Invert {
    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let amount = ctx.value(Self::AMOUNT, f64::from(self.amount));
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let flipped = match self.mode {
                Mode::Video => 1.0 - x,
                Mode::Audio => -x,
            };
            *out = mix(x, flipped, amount.at(i));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn invert(params: &str, input: &[f32]) -> Vec<f32> {
        let mut n = node("invert", params, input.len(), input.len() as f64, &[true]);
        process_one(n.as_mut(), &[input.to_vec()])
    }

    #[test]
    fn video_mode_is_one_minus_x() {
        assert_eq!(invert("{}", &[0.0, 0.25, 1.0]), [1.0, 0.75, 0.0]);
    }

    #[test]
    fn audio_mode_flips_the_sign() {
        assert_eq!(
            invert(r#"{ "mode": "audio" }"#, &[0.5, -0.25]),
            [-0.5, 0.25]
        );
    }

    #[test]
    fn amount_zero_is_a_passthrough() {
        let input = [0.1, 0.9];
        assert_eq!(invert(r#"{ "amount": 0 }"#, &input), input);
    }

    #[test]
    fn inverting_twice_restores_the_signal() {
        let input = [0.2, 0.7, 1.0];
        let once = invert("{}", &input);
        let twice = invert("{}", &once);
        for (a, b) in input.iter().zip(&twice) {
            assert!((a - b).abs() < 1e-6);
        }
    }
}
