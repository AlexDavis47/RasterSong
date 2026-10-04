use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{InputSpec, Node, ParamSpec, Params, ProcessContext, Signal};

/// Amplitude modulation: `out = carrier × (1 + depth × modulator)`.
#[derive(Debug)]
pub struct Am {
    depth: f32,
}

params! { Am {
    DEPTH: ParamSpec::number(
        "depth",
        "Depth",
        1.0,
        -10.0,
        10.0,
        "How strongly the modulator scales the carrier: carrier × (1 + depth × modulator)",
    )
    .exposed()
    .unbounded(),
} }

impl NodeKind for Am {
    const KIND: &'static str = "am";
    const SPEC: NodeSpec = NodeSpec::new("Amplitude Modulation", Category::Effect)
        .describe("Scales the carrier by the modulator")
        .params(Self::PARAMS)
        .inputs(&[
            InputSpec::required("carrier", "The signal that gets scaled"),
            InputSpec::required("modulator", "The signal that scales the carrier"),
        ])
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[r#"{ "depth": 0.8 }"#, r#"{ "depth": -2 }"#];
    const BENCH: Option<&'static str> = Some(r#"{ "depth": 0.8 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            depth: params.float_at(Self::DEPTH)?,
        })
    }
}

impl Node for Am {
    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let (carrier, modulator) = (&inputs[0].data, &inputs[1].data);
        let depth = ctx.value(Self::DEPTH, f64::from(self.depth));
        for (i, ((out, &c), &m)) in outputs[0]
            .data
            .iter_mut()
            .zip(carrier)
            .zip(modulator)
            .enumerate()
        {
            *out = c * (1.0 + depth.at(i) * m);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn am(depth: f64, carrier: &[f32], modulator: &[f32]) -> Vec<f32> {
        let mut node = node(
            "am",
            &format!(r#"{{ "depth": {depth} }}"#),
            carrier.len(),
            carrier.len() as f64,
            &[true, true],
        );
        process_one(node.as_mut(), &[carrier.to_vec(), modulator.to_vec()])
    }

    #[test]
    fn scales_the_carrier_by_one_plus_depth_times_the_modulator() {
        let out = am(0.5, &[1.0, 2.0, -1.0], &[1.0, -1.0, 0.0]);
        assert_eq!(out, [1.5, 1.0, -1.0]);
    }

    #[test]
    fn zero_depth_passes_the_carrier_through() {
        let carrier = [0.3, -0.7, 0.9];
        assert_eq!(am(0.0, &carrier, &[5.0, 5.0, 5.0]), carrier);
    }

    #[test]
    fn a_depth_of_minus_one_with_a_full_modulator_silences_it() {
        assert_eq!(am(-1.0, &[0.8, 0.8], &[1.0, 1.0]), [0.0, 0.0]);
    }
}
