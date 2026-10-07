use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Layout, LayoutContext, Node, Params, ProcessContext, Signal};

/// Swaps rows and columns, so the picture's width and height swap too. Works on one frame at a
/// time, so audio is only transposed within each block.
#[derive(Debug)]
pub struct Transpose;

params! { Transpose {} }

impl NodeKind for Transpose {
    const KIND: &'static str = "transpose";
    const SPEC: NodeSpec = NodeSpec::new(Category::Structure)
        .params(Self::PARAMS);

    fn new(_params: &Params) -> Result<Self, String> {
        Ok(Self)
    }
}

impl Node for Transpose {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let input = ctx.inputs[0];
        let layout = input.reshaped(input.height, input.width, input.samples_per_pixel);
        Ok(vec![layout; ctx.output_count])
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = inputs[0];
        let spp = input.layout.samples_per_pixel as usize;
        let (w, h) = (input.layout.width as usize, input.layout.height as usize);
        let out = &mut outputs[0].data;
        // The output is `h` wide and `w` tall: its pixel (y, x) is the input's (x, y).
        for y in 0..h {
            for x in 0..w {
                let src = (y * w + x) * spp;
                let dst = (x * h + y) * spp;
                out[dst..dst + spp].copy_from_slice(&input.data[src..src + spp]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{Layout, LayoutContext, ProcessContext, Registry, Signal};

    fn transpose(input: &Signal) -> Signal {
        let mut node = Registry::shared()
            .create("transpose", &Default::default())
            .unwrap()
            .unwrap();
        let sources = HashMap::new();
        let inputs = [input.layout];
        let layouts = node
            .output_layouts(&LayoutContext {
                inputs: &inputs,
                connected: &[true],
                sources: &sources,
                output: input.layout,
                layout: Default::default(),
                output_count: 1,
            })
            .unwrap();
        let mut out = vec![Signal::zeros(layouts[0])];
        let sources = HashMap::<String, Signal>::new();
        node.process(
            &ProcessContext {
                frame: 0,
                frame_rate: 1.0,
                sources: &sources,
                params: &[],
            },
            &[input],
            &mut out,
        );
        out.remove(0)
    }

    #[test]
    fn swaps_width_and_height() {
        let picture = Signal::from_data(Layout::mono(3, 2), (0..6).map(|i| i as f32).collect());
        let out = transpose(&picture);
        assert_eq!(out.layout, Layout::mono(2, 3));
        assert_eq!(out.data, [0.0, 3.0, 1.0, 4.0, 2.0, 5.0]);
    }

    #[test]
    fn twice_is_the_identity_and_pixels_keep_their_channels() {
        let rgb = Signal::from_data(Layout::rgb(3, 2), (0..18).map(|i| i as f32).collect());
        assert_eq!(transpose(&transpose(&rgb)), rgb);
    }
}
