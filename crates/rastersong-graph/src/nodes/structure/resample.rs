use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Layout, LayoutContext, Node, ParamSpec, Params, ProcessContext, Signal};

choice! {
    /// How the new pixels are read from the old ones.
    pub enum Method {
        /// The nearest source pixel: blocky when enlarging, crisp when shrinking.
        Nearest = "nearest",
        /// A blend of the four nearest source pixels: smooth.
        Linear = "linear",
    }
}

/// Resizes the picture to a new width and height. Effects that follow work on the new, different
/// number of samples per row, so rows, frames and cycles per row mean something different: this is
/// how to process at a lower (or higher) resolution. The output node needs the project's size, so
/// resample back before it.
#[derive(Debug)]
pub struct Resample {
    width: u32,
    height: u32,
    method: Method,
}

params! { Resample {
    WIDTH: ParamSpec::number(
        "width",
        "Width",
        0.0,
        0.0,
        4096.0,
        "New width in pixels; 0 keeps the input's width",
    )
    .fixed()
    .limits(0.0, 16_384.0),
    HEIGHT: ParamSpec::number(
        "height",
        "Height",
        0.0,
        0.0,
        4096.0,
        "New height in pixels; 0 keeps the input's height",
    )
    .fixed()
    .limits(0.0, 16_384.0),
    METHOD: ParamSpec::choice(
        "method",
        "Method",
        Method::OPTIONS,
        "linear",
        "nearest picks the closest pixel, linear blends the four closest",
    ),
} }

impl NodeKind for Resample {
    const KIND: &'static str = "resample";
    const SPEC: NodeSpec = NodeSpec::new("Resample", Category::Structure)
        .describe("Resizes the picture; effects after it work at the new resolution")
        .params(Self::PARAMS);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            width: params.number_at(Self::WIDTH)?.round().max(0.0) as u32,
            height: params.number_at(Self::HEIGHT)?.round().max(0.0) as u32,
            method: params.choice_as(Self::METHOD)?,
        })
    }
}

impl Resample {
    fn target(&self, input: Layout) -> Layout {
        let pick = |wanted: u32, current: u32| if wanted == 0 { current } else { wanted };
        Layout::new(
            pick(self.width, input.width),
            pick(self.height, input.height),
            input.samples_per_pixel,
        )
    }
}

impl Node for Resample {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        let target = self.target(ctx.inputs[0]);
        // Rows are samples of one block; keep the block small enough to be a picture.
        if target.len() > 1 << 28 {
            return Err(format!("{target} is too large"));
        }
        Ok(vec![target; ctx.output_count])
    }

    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = inputs[0];
        let out = &mut outputs[0];
        if input.layout == out.layout {
            out.data.copy_from_slice(&input.data);
            return;
        }
        let spp = input.layout.samples_per_pixel as usize;
        let (sw, sh) = (input.layout.width as usize, input.layout.height as usize);
        let (dw, dh) = (out.layout.width as usize, out.layout.height as usize);
        let (rx, ry) = (sw as f64 / dw as f64, sh as f64 / dh as f64);
        for y in 0..dh {
            // Centre-aligned, so the picture covers the same area at any size.
            let fy = (y as f64 + 0.5) * ry - 0.5;
            for x in 0..dw {
                let fx = (x as f64 + 0.5) * rx - 0.5;
                let dst = (y * dw + x) * spp;
                match self.method {
                    Method::Nearest => {
                        let sx = (((x as f64 + 0.5) * rx) as usize).min(sw - 1);
                        let sy = (((y as f64 + 0.5) * ry) as usize).min(sh - 1);
                        let src = (sy * sw + sx) * spp;
                        out.data[dst..dst + spp].copy_from_slice(&input.data[src..src + spp]);
                    }
                    Method::Linear => {
                        let fx = fx.clamp(0.0, (sw - 1) as f64);
                        let fy = fy.clamp(0.0, (sh - 1) as f64);
                        let (x0, y0) = (fx as usize, fy as usize);
                        let (x1, y1) = ((x0 + 1).min(sw - 1), (y0 + 1).min(sh - 1));
                        let (tx, ty) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
                        for c in 0..spp {
                            let at = |px: usize, py: usize| input.data[(py * sw + px) * spp + c];
                            let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
                            let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
                            out.data[dst + c] = top + (bottom - top) * ty;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{Layout, LayoutContext, ParamValue, ProcessContext, Registry, Signal};

    fn resample(params: &str, input: &Signal) -> Signal {
        let params: std::collections::BTreeMap<String, ParamValue> =
            serde_json::from_str(params).unwrap();
        let mut node = Registry::shared()
            .create("resample", &params)
            .unwrap()
            .unwrap();
        let sources = HashMap::new();
        let inputs = [input.layout];
        let layouts = node
            .output_layouts(&LayoutContext {
                inputs: &inputs,
                sources: &sources,
                output: input.layout,
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
    fn defaults_keep_the_picture() {
        let input = Signal::from_data(Layout::mono(3, 2), (0..6).map(|i| i as f32).collect());
        assert_eq!(resample("{}", &input), input);
    }

    #[test]
    fn zero_keeps_one_dimension() {
        let input = Signal::zeros(Layout::rgb(8, 6));
        let out = resample(r#"{ "width": 4 }"#, &input);
        assert_eq!(out.layout, Layout::rgb(4, 6));
    }

    #[test]
    fn nearest_doubling_repeats_pixels() {
        let input = Signal::from_data(Layout::mono(2, 1), vec![1.0, 2.0]);
        let out = resample(r#"{ "width": 4, "method": "nearest" }"#, &input);
        assert_eq!(out.data, [1.0, 1.0, 2.0, 2.0]);
    }

    #[test]
    fn linear_halving_averages_pairs() {
        let input = Signal::from_data(Layout::mono(4, 1), vec![0.0, 2.0, 4.0, 6.0]);
        let out = resample(r#"{ "width": 2 }"#, &input);
        assert_eq!(out.data, [1.0, 5.0]);
    }

    #[test]
    fn a_flat_picture_stays_flat_at_any_size() {
        let input = Signal::from_data(Layout::rgb(5, 3), vec![0.4; 45]);
        let out = resample(r#"{ "width": 7, "height": 2 }"#, &input);
        assert_eq!(out.layout, Layout::rgb(7, 2));
        assert!(out.data.iter().all(|&v| (v - 0.4).abs() < 1e-6));
    }

    #[test]
    fn channels_stay_separate() {
        let input = Signal::from_data(Layout::rgb(2, 1), vec![1.0, 0.0, 0.5, 1.0, 0.0, 0.5]);
        let out = resample(r#"{ "width": 4, "method": "nearest" }"#, &input);
        assert_eq!(
            out.data,
            [1.0, 0.0, 0.5, 1.0, 0.0, 0.5, 1.0, 0.0, 0.5, 1.0, 0.0, 0.5]
        );
    }
}
