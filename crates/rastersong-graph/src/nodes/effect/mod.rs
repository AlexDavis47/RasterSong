//! Effect nodes. They see flat sample streams and never look at layout, except to convert
//! user-facing units (rows, frames, cycles per row, milliseconds) to samples.

mod am;
mod bitcrush;
mod compressor;
mod delay;
mod distortion;
mod gate;
mod lowpass;
mod three_band;

pub use am::Am;
pub use bitcrush::Bitcrush;
pub use compressor::Compressor;
pub use delay::Delay;
pub use distortion::{Distortion, Shape};
pub use gate::Gate;
pub use lowpass::Lowpass;
pub use three_band::ThreeBand;

use crate::{Params, PrepareContext};

/// Maximum warmup a node with infinite memory reports, in frames.
const MAX_WARMUP_FRAMES: u32 = 120;

/// Frames for `samples` of settling time, at least one and at most [`MAX_WARMUP_FRAMES`].
fn settle_frames(samples: f64, ctx: &PrepareContext) -> u32 {
    if !samples.is_finite() {
        return MAX_WARMUP_FRAMES;
    }
    ((samples / ctx.samples_per_frame().max(1) as f64).ceil() as u32).clamp(1, MAX_WARMUP_FRAMES)
}

/// Samples of the main signal in `ms` milliseconds of its own time.
fn ms_to_samples(ms: f64, ctx: &PrepareContext) -> f64 {
    ms / 1000.0 * ctx.sample_rate()
}

/// A length unit for user-facing parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    Rows,
    Frames,
}

impl LengthUnit {
    fn read(params: &Params) -> Result<Self, String> {
        Ok(match params.choice("unit")? {
            "rows" => Self::Rows,
            _ => Self::Frames,
        })
    }

    fn samples(self, ctx: &PrepareContext) -> f64 {
        match self {
            Self::Rows => ctx.samples_per_row() as f64,
            Self::Frames => ctx.samples_per_frame() as f64,
        }
    }
}

/// Test helpers shared by the effect nodes' unit tests.
#[cfg(test)]
mod test_util {
    use std::collections::{BTreeMap, HashMap};

    use crate::{Layout, Node, ParamValue, PrepareContext, ProcessContext, Registry, Signal};

    /// Creates `kind` with `params` (JSON) and prepares it for mono blocks of `len` samples at
    /// `rate` samples per second, with the given inputs connected.
    pub fn node(
        kind: &str,
        params: &str,
        len: usize,
        rate: f64,
        connected: &[bool],
    ) -> Box<dyn Node> {
        let params: BTreeMap<String, ParamValue> = serde_json::from_str(params).unwrap();
        let mut node = Registry::default().create(kind, &params).unwrap().unwrap();
        let layout = Layout::mono(len as u32, 1);
        node.prepare(&PrepareContext {
            frame_rate: rate / len as f64,
            inputs: &vec![layout; node.inputs().len()],
            outputs: &vec![layout; node.outputs().len()],
            connected,
            modulated: &[],
        });
        node
    }

    /// Processes one block; `inputs` are the input streams, each `len` long.
    pub fn process(node: &mut dyn Node, inputs: &[Vec<f32>]) -> Vec<f32> {
        let len = inputs[0].len();
        let layout = Layout::mono(len as u32, 1);
        let signals: Vec<Signal> = inputs
            .iter()
            .map(|data| Signal {
                data: data.clone(),
                layout,
            })
            .collect();
        let refs: Vec<&Signal> = signals.iter().collect();
        let mut outputs = vec![Signal::zeros(layout); node.outputs().len()];
        let sources: HashMap<String, Signal> = HashMap::new();
        node.process(
            &ProcessContext {
                frame: 0,
                frame_rate: 1.0,
                sources: &sources,
                params: &[],
            },
            &refs,
            &mut outputs,
        );
        outputs.swap_remove(0).data
    }
}
