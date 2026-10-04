//! Property tests for the node contract, run against every effect node in several configurations:
//! - processing a stream in blocks of any size gives exactly the output of one large block,
//! - `reset()` followed by the same input gives exactly the same output,
//! - valid input never produces NaN or infinity.

use std::collections::BTreeMap;

use proptest::prelude::*;
use rastersong_graph::{
    Layout, Node, ParamValue, PrepareContext, ProcessContext, Registry, Signal,
};

/// Samples per row. Blocks are whole rows, so "rows" units mean the same thing in every block.
const WIDTH: u32 = 6;
const MAX_ROWS: usize = 24;

const CONFIGS: &[(&str, &str)] = &[
    ("am", r#"{ "depth": 0.8 }"#),
    ("delay", r#"{ "time": 1.5 }"#),
    ("delay", r#"{ "time": 0.5, "depth": 0.4 }"#),
    ("delay", r#"{ "time": 2.25, "feedback": 0.6, "mix": 0.7 }"#),
    ("delay", r#"{ "time": 0.3, "depth": 0.2, "feedback": 0.5 }"#),
    ("bitcrush", r#"{ "bits": 3, "depth": 2 }"#),
    ("lowpass", r#"{ "cutoff": 0.7 }"#),
    ("lowpass", r#"{ "cutoff": 1.5, "depth": 2 }"#),
    ("three_band", r#"{ "low_hz": 300, "high_hz": 3000 }"#),
    ("to_audio", r#"{ "mapping": "bugged" }"#),
    ("to_video", r#"{ "mapping": "bugged" }"#),
];

struct Harness {
    node: Box<dyn Node>,
    inputs: usize,
    outputs: usize,
}

impl Harness {
    fn new(kind: &str, params: &str, total_rows: usize) -> Self {
        let params: BTreeMap<String, ParamValue> = serde_json::from_str(params).unwrap();
        let mut node = Registry::default().create(kind, &params).unwrap().unwrap();
        let (inputs, outputs) = (node.inputs().len(), node.outputs().len());
        let layout = Layout::mono(WIDTH, total_rows as u32);
        node.prepare(&PrepareContext {
            frame_rate: 30.0,
            inputs: &vec![layout; inputs],
            outputs: &vec![layout; outputs],
            connected: &vec![true; inputs],
        });
        Self {
            node,
            inputs,
            outputs,
        }
    }

    /// Resets the node, then processes `signal` (and `modulation` as the second input) in blocks
    /// of the given row counts, returning each output stream concatenated.
    fn run(&mut self, blocks: &[usize], signal: &[f32], modulation: &[f32]) -> Vec<Vec<f32>> {
        struct NoSources;
        impl rastersong_graph::Sources for NoSources {
            fn get(&self, _: &str) -> Option<&Signal> {
                None
            }
        }

        self.node.reset();
        let mut result = vec![Vec::new(); self.outputs];
        let mut start = 0;
        for (frame, &rows) in blocks.iter().enumerate() {
            let layout = Layout::mono(WIDTH, rows as u32);
            let range = start..start + layout.len();
            let streams = [signal, modulation];
            let inputs: Vec<Signal> = streams[..self.inputs]
                .iter()
                .map(|s| Signal::from_data(layout, s[range.clone()].to_vec()))
                .collect();
            let refs: Vec<&Signal> = inputs.iter().collect();
            let mut outputs = vec![Signal::zeros(layout); self.outputs];
            let ctx = ProcessContext {
                frame: frame as u64,
                frame_rate: 30.0,
                sources: &NoSources,
            };
            self.node.process(&ctx, &refs, &mut outputs);
            for (stream, out) in result.iter_mut().zip(outputs) {
                stream.extend(out.data);
            }
            start = range.end;
        }
        result
    }
}

/// A stream of `rows` rows, a modulation stream, and a way of cutting the rows into blocks.
fn case() -> impl Strategy<Value = (Vec<usize>, Vec<f32>, Vec<f32>)> {
    (1..=MAX_ROWS).prop_flat_map(|rows| {
        let len = rows * WIDTH as usize;
        (
            proptest::collection::btree_set(1..rows.max(2), 0..6),
            proptest::collection::vec(-1.0f32..1.5, len),
            proptest::collection::vec(-1.0f32..1.0, len),
        )
            .prop_map(move |(cuts, signal, modulation)| {
                let mut blocks = Vec::new();
                let mut last = 0;
                for cut in cuts.into_iter().filter(|&c| c < rows).chain([rows]) {
                    blocks.push(cut - last);
                    last = cut;
                }
                (blocks, signal, modulation)
            })
    })
}

proptest! {
    #[test]
    fn block_size_independent((blocks, signal, modulation) in case()) {
        let rows: usize = blocks.iter().sum();
        for (kind, params) in CONFIGS {
            let mut harness = Harness::new(kind, params, rows);
            let whole = harness.run(&[rows], &signal, &modulation);
            let split = harness.run(&blocks, &signal, &modulation);
            prop_assert_eq!(&split, &whole, "{} {} with blocks {:?}", kind, params, blocks);
        }
    }

    #[test]
    fn reset_is_deterministic_and_output_is_finite((blocks, signal, modulation) in case()) {
        let rows: usize = blocks.iter().sum();
        for (kind, params) in CONFIGS {
            let mut harness = Harness::new(kind, params, rows);
            let first = harness.run(&blocks, &signal, &modulation);
            let second = harness.run(&blocks, &signal, &modulation);
            prop_assert_eq!(&first, &second, "{} {}", kind, params);
            prop_assert!(first.iter().flatten().all(|x| x.is_finite()), "{} {} produced NaN or infinity", kind, params);
        }
    }
}

#[test]
fn three_band_outputs_add_back_up_to_the_input() {
    let rows = 8;
    let mut harness = Harness::new("three_band", r#"{ "low_hz": 300, "high_hz": 3000 }"#, rows);
    let signal: Vec<f32> = (0..rows * WIDTH as usize)
        .map(|i| ((i * 37) % 17) as f32 / 8.0 - 1.0)
        .collect();
    let bands = harness.run(&[rows], &signal, &[]);
    for (i, &x) in signal.iter().enumerate() {
        let sum = bands[0][i] + bands[1][i] + bands[2][i];
        assert!((sum - x).abs() < 1e-5, "sample {i}: {sum} != {x}");
    }
}

#[test]
fn delay_by_one_row_shifts_by_one_row() {
    let rows = 4;
    let mut harness = Harness::new("delay", r#"{ "time": 1 }"#, rows);
    let signal: Vec<f32> = (0..rows * WIDTH as usize).map(|i| i as f32).collect();
    let modulation = vec![0.0; signal.len()];
    let out = harness.run(&[rows], &signal, &modulation);
    let width = WIDTH as usize;
    assert!(out[0][..width].iter().all(|&x| x == 0.0));
    assert_eq!(out[0][width..], signal[..signal.len() - width]);
}

#[test]
fn bitcrush_quantizes_to_levels() {
    let mut harness = Harness::new("bitcrush", r#"{ "bits": 1 }"#, 1);
    let signal = [0.0, 0.2, 0.49, 0.51, 0.9, 1.0];
    let out = harness.run(&[1], &signal, &[0.0; 6]);
    assert_eq!(out[0], [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
}
