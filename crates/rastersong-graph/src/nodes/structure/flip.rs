use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Node, ParamSpec, Params, ProcessContext, Signal};

choice! {
    /// How the picture (or block of audio) is turned around.
    pub enum Mode {
        /// Mirrors each row: left becomes right, and audio plays backwards within the block.
        Horizontal = "horizontal",
        /// Turns the rows upside down.
        Vertical = "vertical",
        /// Both flips: the picture rotated by 180°.
        Reverse = "reverse",
    }
}

/// Mirrors or turns over the picture. Works on one frame at a time, so audio is only
/// reversed within each block.
#[derive(Debug)]
pub struct Flip {
    mode: Mode,
}

params! { Flip {
    MODE: ParamSpec::choice("mode", Mode::OPTIONS, "horizontal"),
} }

impl NodeKind for Flip {
    const KIND: &'static str = "flip";
    const SPEC: NodeSpec = NodeSpec::new(Category::Structure)
        .params(Self::PARAMS);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            mode: params.choice_as(Self::MODE)?,
        })
    }
}

impl Node for Flip {
    fn process(&mut self, _ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        let input = inputs[0];
        let spp = input.layout.samples_per_pixel as usize;
        let (w, h) = (input.layout.width as usize, input.layout.height as usize);
        let out = &mut outputs[0].data;
        for y in 0..h {
            for x in 0..w {
                let (sx, sy) = match self.mode {
                    Mode::Horizontal => (w - 1 - x, y),
                    Mode::Vertical => (x, h - 1 - y),
                    Mode::Reverse => (w - 1 - x, h - 1 - y),
                };
                let src = (sy * w + sx) * spp;
                let dst = (y * w + x) * spp;
                out[dst..dst + spp].copy_from_slice(&input.data[src..src + spp]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{Layout, LayoutContext, ParamValue, ProcessContext, Registry, Signal};

    /// Runs the flip node over `input` (3×2 mono: 0 1 2 / 3 4 5, unless given RGB) and returns
    /// its output layout and data.
    fn flip(mode: &str, input: &Signal) -> Signal {
        let params = [("mode".to_owned(), ParamValue::Text(mode.to_owned()))].into();
        let mut node = Registry::shared().create("flip", &params).unwrap().unwrap();
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

    fn picture() -> Signal {
        Signal::from_data(Layout::mono(3, 2), (0..6).map(|i| i as f32).collect())
    }

    #[test]
    fn horizontal_mirrors_each_row() {
        assert_eq!(
            flip("horizontal", &picture()).data,
            [2.0, 1.0, 0.0, 5.0, 4.0, 3.0]
        );
    }

    #[test]
    fn vertical_swaps_the_rows() {
        assert_eq!(
            flip("vertical", &picture()).data,
            [3.0, 4.0, 5.0, 0.0, 1.0, 2.0]
        );
    }

    #[test]
    fn reverse_is_both() {
        assert_eq!(
            flip("reverse", &picture()).data,
            [5.0, 4.0, 3.0, 2.0, 1.0, 0.0]
        );
    }

    #[test]
    fn every_mode_twice_is_the_identity() {
        let rgb = Signal::from_data(Layout::rgb(3, 2), (0..18).map(|i| i as f32).collect());
        for mode in ["horizontal", "vertical", "reverse"] {
            let once = flip(mode, &rgb);
            assert_eq!(flip(mode, &once), rgb, "{mode}");
        }
    }

    #[test]
    fn pixels_keep_their_channel_order() {
        let rgb = Signal::from_data(Layout::rgb(2, 1), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(
            flip("horizontal", &rgb).data,
            [4.0, 5.0, 6.0, 1.0, 2.0, 3.0]
        );
    }
}
