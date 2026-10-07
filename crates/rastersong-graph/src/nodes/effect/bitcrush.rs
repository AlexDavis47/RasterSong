use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, ProcessContext, Signal};

/// Reduces bit depth: quantizes to `2^bits` levels over `0..=1`. Fractional and modulated
/// bit depths are allowed.
#[derive(Debug)]
pub struct Bitcrush {
    bits: f32,
}

params! { Bitcrush {
    BITS: ParamSpec::number("bits", 4.0,
        1.0,
        24.0)
    .exposed()
    .unit("bits"),
} }

impl NodeKind for Bitcrush {
    const KIND: &'static str = "bitcrush";
    const SPEC: NodeSpec = NodeSpec::new(Category::Effect)
        .params(Self::PARAMS)
        .per_channel();
    const TEST_CONFIGS: &'static [&'static str] = &[r#"{ "bits": 3 }"#, r#"{ "bits": 1.5 }"#];
    const BENCH: Option<&'static str> = Some(r#"{ "bits": 3 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            bits: params.float_at(Self::BITS)?,
        })
    }
}

impl Node for Bitcrush {
    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let bits = ctx.value(Self::BITS, f64::from(self.bits));
        for (i, (out, &x)) in outputs[0].data.iter_mut().zip(&inputs[0].data).enumerate() {
            let steps = bits.at(i).clamp(1.0, 24.0).exp2() - 1.0;
            *out = (x * steps).round() / steps;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn crush(bits: f64, input: &[f32]) -> Vec<f32> {
        let mut node = node(
            "bitcrush",
            &format!(r#"{{ "bits": {bits} }}"#),
            input.len(),
            input.len() as f64,
            &[true],
        );
        process_one(node.as_mut(), &[input.to_vec()])
    }

    #[test]
    fn one_bit_is_black_or_white() {
        let out = crush(1.0, &[0.0, 0.2, 0.49, 0.51, 0.9, 1.0]);
        assert_eq!(out, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn two_bits_snap_to_thirds() {
        let out = crush(2.0, &[0.0, 0.2, 0.4, 0.5, 0.7, 1.0]);
        let third = 1.0 / 3.0;
        let expected = [0.0, third, third, 2.0 * third, 2.0 * third, 1.0];
        for (a, b) in out.iter().zip(expected) {
            assert!((a - b).abs() < 1e-6, "{out:?}");
        }
    }

    #[test]
    fn the_output_only_has_the_available_levels() {
        let input: Vec<f32> = (0..=100).map(|i| i as f32 / 100.0).collect();
        let out = crush(3.0, &input);
        let mut levels: Vec<i32> = out.iter().map(|&x| (x * 7.0).round() as i32).collect();
        levels.dedup();
        assert_eq!(levels.len(), 8, "2^3 levels");
        assert!(
            out.iter()
                .all(|&x| ((x * 7.0).round() - x * 7.0).abs() < 1e-5)
        );
    }
}
