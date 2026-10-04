//! Nodes that take signals apart and rebuild them. They are the only nodes that care about layout.

use crate::{InputSpec, Layout, LayoutContext, Node, ProcessContext, Signal};

fn expect_rgb(layout: Layout) -> Result<(), String> {
    if layout.samples_per_pixel == 3 {
        Ok(())
    } else {
        Err(format!("expects an RGB signal, got {layout}"))
    }
}

/// RGB → separate R, G and B signals.
#[derive(Debug)]
pub struct Split;

impl Node for Split {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn outputs(&self) -> &'static [&'static str] {
        &["r", "g", "b"]
    }

    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        expect_rgb(input)?;
        Ok(vec![Layout::mono(input.width, input.height); 3])
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let [r, g, b] = outputs else { unreachable!() };
        for (i, pixel) in inputs[0].data.as_chunks::<3>().0.iter().enumerate() {
            r.data[i] = pixel[0];
            g.data[i] = pixel[1];
            b.data[i] = pixel[2];
        }
    }
}

/// Separate R, G and B signals → RGB. The inverse of [`Split`].
#[derive(Debug)]
pub struct Combine;

impl Node for Combine {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[
            InputSpec::required("r"),
            InputSpec::required("g"),
            InputSpec::required("b"),
        ];
        INPUTS
    }

    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let r = ctx.inputs[0];
        // Rate matching is 1-D; it must never be used to reconcile images of different sizes.
        if r.samples_per_pixel != 1 || ctx.inputs.iter().any(|&l| l != r) {
            return Err(format!(
                "expects three mono signals of the same size, got {}, {}, {}",
                ctx.inputs[0], ctx.inputs[1], ctx.inputs[2]
            ));
        }
        Ok(vec![Layout::rgb(r.width, r.height)])
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let [r, g, b] = inputs else { unreachable!() };
        for (i, pixel) in outputs[0]
            .data
            .as_chunks_mut::<3>()
            .0
            .iter_mut()
            .enumerate()
        {
            pixel[0] = r.data[i];
            pixel[1] = g.data[i];
            pixel[2] = b.data[i];
        }
    }
}

/// RGB → one mono carrier with the channels packed in sequence (R, G, B, R, G, B, …), three times
/// as wide. The samples don't change, but downstream nodes now treat each channel value as its own
/// sample: a mono modulator varies across a pixel's R, G and B instead of moving them together.
#[derive(Debug)]
pub struct Interleave;

impl Node for Interleave {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        expect_rgb(input)?;
        Ok(vec![Layout::mono(input.width * 3, input.height)])
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        outputs[0].data.copy_from_slice(&inputs[0].data);
    }
}

/// A packed mono carrier → RGB. The inverse of [`Interleave`].
#[derive(Debug)]
pub struct Pack;

impl Node for Pack {
    fn inputs(&self) -> &'static [InputSpec] {
        const INPUTS: &[InputSpec] = &[InputSpec::required("in")];
        INPUTS
    }

    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        if input.samples_per_pixel != 1 || !input.width.is_multiple_of(3) {
            return Err(format!(
                "expects a mono signal whose width is a multiple of 3, got {input}"
            ));
        }
        Ok(vec![Layout::rgb(input.width / 3, input.height)])
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        outputs[0].data.copy_from_slice(&inputs[0].data);
    }
}
