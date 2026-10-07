use crate::dsp::mix;
use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, ProcessContext, Signal};

/// Ring modulation: `out = carrier × modulator`, blended with the dry carrier. Unlike
/// amplitude modulation the carrier itself disappears, leaving only the sum and difference
/// frequencies. Stateless.
#[derive(Debug)]
pub struct RingMod {
    mix: f32,
}

params! { RingMod {
    MIX: ParamSpec::mix().exposed(),
} }

impl NodeKind for RingMod {
    const KIND: &'static str = "ring_mod";
    const SPEC: NodeSpec = NodeSpec::new("Ring Modulation", Category::Effect)
        .describe("Multiplies the carrier by the modulator, leaving sum and difference tones")
        .params(Self::PARAMS)
        .inputs(&[
            InputSpec::required("carrier", "The signal that gets multiplied"),
            InputSpec::required("modulator", "The signal it is multiplied by"),
        ])
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[r#"{ "mix": 0.5 }"#, r#"{ "mix": 1 }"#];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            mix: params.float_at(Self::MIX)?,
        })
    }
}

impl Node for RingMod {
    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let amount = ctx.value(Self::MIX, f64::from(self.mix));
        for (i, ((out, &c), &m)) in outputs[0]
            .data
            .iter_mut()
            .zip(&inputs[0].data)
            .zip(&inputs[1].data)
            .enumerate()
        {
            *out = mix(c, c * m, amount.at(i));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn ring(params: &str, carrier: &[f32], modulator: &[f32]) -> Vec<f32> {
        let mut n = node(
            "ring_mod",
            params,
            carrier.len(),
            carrier.len() as f64,
            &[true, true],
        );
        process_one(n.as_mut(), &[carrier.to_vec(), modulator.to_vec()])
    }

    #[test]
    fn multiplies_the_signals() {
        assert_eq!(ring("{}", &[0.5, -1.0], &[0.5, 0.5]), [0.25, -0.5]);
    }

    #[test]
    fn mix_zero_is_the_carrier() {
        let carrier = [0.3, -0.7];
        assert_eq!(ring(r#"{ "mix": 0 }"#, &carrier, &[0.9, 0.9]), carrier);
    }
}
