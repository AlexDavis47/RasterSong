//! Helpers for testing nodes. Used by this crate's unit and integration tests; other crates get
//! them with the `testing` feature.

use std::collections::{BTreeMap, HashMap};

use crate::{Layout, Node, ParamValue, PrepareContext, ProcessContext, Registry, Signal, Tempo};

/// Creates `kind` with `params` (JSON) and prepares it for mono blocks of `len` samples at
/// `rate` samples per second, with the given inputs connected.
pub fn node(kind: &str, params: &str, len: usize, rate: f64, connected: &[bool]) -> Box<dyn Node> {
    node_with_tempo(kind, params, len, rate, connected, Tempo::default())
}

/// Like [`node`], at a given project tempo.
pub fn node_with_tempo(
    kind: &str,
    params: &str,
    len: usize,
    rate: f64,
    connected: &[bool],
    tempo: Tempo,
) -> Box<dyn Node> {
    let params: BTreeMap<String, ParamValue> = serde_json::from_str(params).unwrap();
    let registry = Registry::shared();
    let mut node = registry.create(kind, &params).unwrap().unwrap();
    let spec = registry.get(kind).unwrap().spec;
    let layout = Layout::mono(len as u32, 1);
    node.prepare(&PrepareContext {
        frame_rate: rate / len as f64,
        tempo,
        inputs: &vec![layout; spec.inputs.len()],
        outputs: &vec![layout; spec.outputs.len()],
        connected,
        modulated: &[],
        pixel_scale: 1.0,
    });
    node
}

/// Processes one block; `inputs` are the input streams, each `len` long. Returns the first output.
pub fn process(node: &mut dyn Node, outputs: usize, inputs: &[Vec<f32>]) -> Vec<Vec<f32>> {
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
    let mut result = vec![Signal::zeros(layout); outputs];
    let sources: HashMap<String, Signal> = HashMap::new();
    node.process(
        &ProcessContext {
            frame: 0,
            frame_rate: 1.0,
            sources: &sources,
            params: &[],
        },
        &refs,
        &mut result,
    );
    result.into_iter().map(|s| s.data).collect()
}

/// Like [`process`] for a node with one output.
pub fn process_one(node: &mut dyn Node, inputs: &[Vec<f32>]) -> Vec<f32> {
    process(node, 1, inputs).swap_remove(0)
}

/// Test node: delays its input by half a frame and reports that as latency, like a node that
/// needs lookahead would.
#[derive(Debug, Default)]
pub struct Lookahead {
    samples: usize,
    history: Vec<f32>,
}

impl Node for Lookahead {
    fn prepare(&mut self, ctx: &PrepareContext) {
        self.samples = ctx.samples_per_frame() / 2;
    }

    fn process(&mut self, _: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        for (out, &x) in outputs[0].data.iter_mut().zip(&inputs[0].data) {
            self.history.push(x);
            let n = self.history.len();
            *out = if n > self.samples {
                self.history[n - 1 - self.samples]
            } else {
                0.0
            };
        }
    }

    fn reset(&mut self) {
        self.history.clear();
    }

    fn latency(&self, ctx: &PrepareContext) -> usize {
        ctx.samples_per_frame() / 2
    }
}

/// Test node: adds its two inputs, `a` and `b`.
#[derive(Debug)]
pub struct Sum;

impl Node for Sum {
    fn process(&mut self, _: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]) {
        for ((out, &a), &b) in outputs[0]
            .data
            .iter_mut()
            .zip(&inputs[0].data)
            .zip(&inputs[1].data)
        {
            *out = a + b;
        }
    }
}

const SUM_INPUTS: &[crate::InputSpec] = &[
    crate::InputSpec::required("a"),
    crate::InputSpec::required("b"),
];

/// Registers the test nodes `lookahead` and `sum`.
pub fn register_fakes(registry: &mut Registry) {
    use crate::{Category, NodeSpec};
    registry.register_custom("lookahead", NodeSpec::new(Category::Effect), |_| {
        Ok(Lookahead::default())
    });
    registry.register_custom(
        "sum",
        NodeSpec::new(Category::Effect).inputs(SUM_INPUTS),
        |_| Ok(Sum),
    );
}
