//! Compiling a [`GraphDesc`] into a sequential schedule, and running it one frame at a time.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::desc::{Channels, GraphDesc, Interpolation, Modulation};
use crate::dsp::{DelayLine, resample};
use crate::nodes::{OUTPUT, Registry};
use crate::{
    GraphError, InputSpec, Layout, LayoutContext, Node, OutputSpec, ParamSpec, ParamValue,
    PrepareContext, ProcessContext, Signal, Sources, Tempo,
};

/// Most inputs a node can have.
pub const MAX_INPUTS: usize = 8;
/// Most parameters a node can have.
pub const MAX_PARAMS: usize = 16;

/// Marks a connection to a parameter rather than an input: `"node.@param"`.
pub const PARAM_PREFIX: char = '@';

/// Samples measured per output for [`OutputLevel`]; a spread-out subset is plenty for a level.
const LEVEL_SAMPLES: usize = 4096;

static EMPTY_SIGNAL: Signal = Signal::EMPTY;

/// What the host tells the compiler about the render.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub frame_rate: f64,
    /// The project's tempo, for beat and bar units.
    pub tempo: Tempo,
    /// Layouts of the signals the host will supply each frame, by source name.
    pub sources: HashMap<String, Layout>,
    /// The layout the output node must produce (the project's RGB frame).
    pub output: Layout,
}

/// The level of one node output in the last processed frame.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputLevel {
    pub node: Arc<str>,
    pub output: usize,
    /// Root mean square of the output's samples.
    pub rms: f32,
}

/// The value of a modulated parameter in the last processed frame, at its middle sample.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamLevel {
    pub node: Arc<str>,
    /// The parameter's index in the node's specs.
    pub index: usize,
    pub value: f32,
}

/// A compiled graph, ready to process frames in order.
pub struct Graph {
    steps: Vec<Step>,
    output_step: usize,
    frame_rate: f64,
    sources: Vec<(String, Layout)>,
    latency_frames: u32,
    warmup_frames: u32,
}

impl std::fmt::Debug for Graph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Graph")
            .field(
                "steps",
                &self.steps.iter().map(|s| &s.id).collect::<Vec<_>>(),
            )
            .field("latency_frames", &self.latency_frames)
            .field("warmup_frames", &self.warmup_frames)
            .finish_non_exhaustive()
    }
}

struct Step {
    id: Arc<str>,
    /// One node, or one per channel when channels are processed separately.
    nodes: Vec<Box<dyn Node>>,
    /// Per-channel buffers when `nodes` has one instance per channel.
    split: Option<ChannelSplit>,
    interpolation: Interpolation,
    inputs: Vec<InputBinding>,
    /// Signals modulating parameters.
    params: Vec<ParamBinding>,
    outputs: Vec<Signal>,
    levels: Vec<f32>,
}

/// A signal modulating one parameter: its input, and the parameter's per-sample values.
struct ParamBinding {
    /// The parameter's index in the node's specs.
    index: usize,
    spec: &'static ParamSpec,
    input: InputBinding,
    base: f64,
    modulation: Modulation,
    limits: (f64, f64),
    /// The modulated value of each sample, at the main input's length.
    values: Signal,
}

impl ParamBinding {
    fn update(&mut self, done: &[Step], interpolation: Interpolation) {
        self.input.update(done, interpolation);
        let signal = self.input.get(done);
        let (lo, hi) = self.limits;
        for (v, &s) in self.values.data.iter_mut().zip(&signal.data) {
            let value = self
                .spec
                .modulated(self.base, self.modulation, f64::from(s));
            *v = value.clamp(lo, hi) as f32;
        }
    }
}

/// Buffers for running one node instance per channel of an interleaved signal.
struct ChannelSplit {
    /// `inputs[channel][input]`: that input's samples for one channel.
    inputs: Vec<Vec<Signal>>,
    /// `params[channel][k]`: modulated parameter `k`'s values for one channel.
    params: Vec<Vec<Signal>>,
    /// `outputs[channel][output]`.
    outputs: Vec<Vec<Signal>>,
}

/// Copies every `channels`-th sample of `interleaved`, starting at `channel`, into `out`.
fn deinterleave(interleaved: &[f32], channel: usize, channels: usize, out: &mut [f32]) {
    for (o, &x) in out
        .iter_mut()
        .zip(interleaved.iter().skip(channel).step_by(channels))
    {
        *o = x;
    }
}

impl ChannelSplit {
    fn process(
        &mut self,
        nodes: &mut [Box<dyn Node>],
        ctx: &ProcessContext,
        inputs: &[&Signal],
        params: &[ParamBinding],
        outputs: &mut [Signal],
    ) {
        let channels = nodes.len();
        for (k, input) in inputs.iter().enumerate() {
            for (c, channel) in self.inputs.iter_mut().enumerate() {
                deinterleave(&input.data, c, channels, &mut channel[k].data);
            }
        }
        for (k, param) in params.iter().enumerate() {
            for (c, channel) in self.params.iter_mut().enumerate() {
                deinterleave(&param.values.data, c, channels, &mut channel[k].data);
            }
        }
        for (c, node) in nodes.iter_mut().enumerate() {
            let mut refs = [&EMPTY_SIGNAL; MAX_INPUTS];
            for (slot, input) in refs.iter_mut().zip(&self.inputs[c]) {
                *slot = input;
            }
            let mut values = [None; MAX_PARAMS];
            for (param, channel) in params.iter().zip(&self.params[c]) {
                values[param.index] = Some(channel.data.as_slice());
            }
            let ctx = ProcessContext {
                params: &values,
                ..*ctx
            };
            node.process(&ctx, &refs[..inputs.len()], &mut self.outputs[c]);
        }
        for (o, output) in outputs.iter_mut().enumerate() {
            for (c, channel) in self.outputs.iter().enumerate() {
                for (x, &y) in output
                    .data
                    .iter_mut()
                    .skip(c)
                    .step_by(channels)
                    .zip(&channel[o].data)
                {
                    *x = y;
                }
            }
        }
    }
}

/// How one input of a step gets its signal each frame.
struct InputBinding {
    /// Producing step and output port; `None` for an unconnected optional input.
    source: Option<(usize, usize)>,
    /// Latency compensation in the source's samples, and the delayed copy it produces.
    compensation: Option<(DelayLine, usize, Signal)>,
    /// When the source's length differs from the main input's (or the input is unconnected),
    /// the signal at the main input's length.
    resampled: Option<Signal>,
    /// Keep runs of this many output samples together when resampling (a pixel's R, G, B).
    group: usize,
}

impl InputBinding {
    /// Brings this input's scratch buffers up to date for the current frame.
    fn update(&mut self, done: &[Step], interpolation: Interpolation) {
        let Some((step, port)) = self.source else {
            return;
        };
        let mut signal = &done[step].outputs[port];
        if let Some((line, delay, delayed)) = &mut self.compensation {
            for (out, &x) in delayed.data.iter_mut().zip(&signal.data) {
                line.push(x);
                *out = line.read(*delay as f64);
            }
            signal = delayed;
        }
        if let Some(resampled) = &mut self.resampled {
            resample(&signal.data, &mut resampled.data, self.group, interpolation);
        }
    }

    fn get<'a>(&'a self, done: &'a [Step]) -> &'a Signal {
        if let Some(resampled) = &self.resampled {
            resampled
        } else if let Some((_, _, delayed)) = &self.compensation {
            delayed
        } else {
            let (step, port) = self
                .source
                .expect("unconnected inputs are resampled silence");
            &done[step].outputs[port]
        }
    }

    fn reset(&mut self) {
        if let Some((line, _, delayed)) = &mut self.compensation {
            line.reset();
            delayed.data.fill(0.0);
        }
    }
}

/// A node during compilation.
struct Pending {
    id: String,
    kind: String,
    params: BTreeMap<String, ParamValue>,
    specs: &'static [ParamSpec],
    inputs: &'static [InputSpec],
    outputs: &'static [OutputSpec],
    modulation: BTreeMap<String, Modulation>,
    node: Box<dyn Node>,
    interpolation: Interpolation,
    channels: Channels,
    /// For each input port, the connected (node, output port).
    wires: Vec<Option<(usize, usize)>>,
    /// For each parameter, the (node, output port) modulating it.
    param_wires: Vec<Option<(usize, usize)>>,
}

impl Pending {
    /// Every connected (node, output port): inputs, then parameters.
    fn sources(&self) -> impl Iterator<Item = &(usize, usize)> {
        self.wires.iter().chain(&self.param_wires).flatten()
    }

    /// How parameter `index` is modulated, if a signal is connected to it.
    fn modulation_of(&self, index: usize) -> Option<(f64, Modulation, (f64, f64))> {
        self.param_wires[index]?;
        let spec = &self.specs[index];
        let base = spec.number_value(&self.params)?;
        let limits = spec.number_limits()?;
        let modulation = self
            .modulation
            .get(spec.name)
            .copied()
            .unwrap_or_else(|| spec.default_modulation(base));
        Some((base, modulation, limits))
    }
}

impl Graph {
    pub fn compile(
        desc: &GraphDesc,
        registry: &Registry,
        options: &CompileOptions,
    ) -> Result<Self, GraphError> {
        let mut pending = create_nodes(desc, registry)?;
        connect(desc, &mut pending)?;

        let outputs: Vec<usize> = (0..pending.len())
            .filter(|&i| pending[i].kind == OUTPUT)
            .collect();
        let &[output] = outputs.as_slice() else {
            return Err(GraphError::OutputCount(outputs.len()));
        };
        let order = schedule(&pending, output)?;

        let mut compiler = Compiler::new(registry, options, pending, output);
        for &n in &order {
            compiler.add_step(n)?;
        }
        Ok(compiler.finish())
    }

    /// Frames between a source frame going in and its result coming out of [`Self::process`].
    /// The host renders this many extra frames and drops the first ones.
    pub fn latency_frames(&self) -> u32 {
        self.latency_frames
    }

    /// Frames to render and discard after [`Self::reset`] before output is valid, when starting
    /// anywhere other than the first frame.
    pub fn warmup_frames(&self) -> u32 {
        self.warmup_frames
    }

    pub fn output_layout(&self) -> Layout {
        self.steps[self.output_step].outputs[0].layout
    }

    /// Clears all state, as if no frame had been processed.
    pub fn reset(&mut self) {
        for step in &mut self.steps {
            step.nodes.iter_mut().for_each(|n| n.reset());
            step.inputs.iter_mut().for_each(InputBinding::reset);
            step.params.iter_mut().for_each(|p| p.input.reset());
        }
    }

    /// The level of every node output in the last processed frame.
    pub fn levels(&self) -> Vec<OutputLevel> {
        self.steps
            .iter()
            .flat_map(|step| {
                step.levels
                    .iter()
                    .enumerate()
                    .map(|(output, &rms)| OutputLevel {
                        node: step.id.clone(),
                        output,
                        rms,
                    })
            })
            .collect()
    }

    /// The value of every modulated parameter in the last processed frame. A parameter can change
    /// on every sample; this is its value at the middle of the frame.
    pub fn param_levels(&self) -> Vec<ParamLevel> {
        self.steps
            .iter()
            .flat_map(|step| {
                step.params.iter().map(|p| ParamLevel {
                    node: step.id.clone(),
                    index: p.index,
                    value: p
                        .values
                        .data
                        .get(p.values.data.len() / 2)
                        .copied()
                        .unwrap_or(0.0),
                })
            })
            .collect()
    }

    /// Processes one frame and returns the output.
    pub fn process(&mut self, frame: u64, sources: &dyn Sources) -> Result<&Signal, GraphError> {
        for (name, layout) in &self.sources {
            match sources.get(name) {
                Some(s) if s.layout == *layout && s.data.len() == layout.len() => {}
                Some(s) => {
                    return Err(GraphError::Source {
                        name: name.clone(),
                        message: format!("expected {layout}, got {}", s.layout),
                    });
                }
                None => {
                    return Err(GraphError::Source {
                        name: name.clone(),
                        message: "not supplied".into(),
                    });
                }
            }
        }

        for i in 0..self.steps.len() {
            let (done, rest) = self.steps.split_at_mut(i);
            let Step {
                nodes,
                split,
                interpolation,
                inputs,
                params,
                outputs,
                levels,
                ..
            } = &mut rest[0];
            for input in inputs.iter_mut() {
                input.update(done, *interpolation);
            }
            for param in params.iter_mut() {
                param.update(done, *interpolation);
            }
            let mut refs = [&EMPTY_SIGNAL; MAX_INPUTS];
            for (slot, input) in refs.iter_mut().zip(inputs.iter()) {
                *slot = input.get(done);
            }
            let refs = &refs[..inputs.len()];
            let mut values = [None; MAX_PARAMS];
            for param in params.iter() {
                values[param.index] = Some(param.values.data.as_slice());
            }
            let ctx = ProcessContext {
                frame,
                frame_rate: self.frame_rate,
                sources,
                params: &values,
            };
            match split {
                Some(split) => split.process(nodes, &ctx, refs, params, outputs),
                None => nodes[0].process(&ctx, refs, outputs),
            }
            for (level, output) in levels.iter_mut().zip(outputs.iter()) {
                *level = rms(&output.data);
            }
        }
        Ok(&self.steps[self.output_step].outputs[0])
    }
}

/// The layouts one node works with, worked out from what feeds it.
struct NodeShape {
    /// The layout every input and parameter signal is brought to: the main input's, or for nodes
    /// without inputs the node's own first output.
    reference: Layout,
    /// Layouts of the inputs as they arrive from upstream, before rate matching.
    input_layouts: Vec<Layout>,
    /// How many copies of the node run: one per channel when channels are separate.
    channels: usize,
    /// What one node instance sees: the mono channel layout when split, else the main layout.
    channel_layout: Option<Layout>,
    /// Layouts of one instance's outputs.
    node_outputs: Vec<Layout>,
    /// Layouts of the step's outputs (interleaved again when channels are split).
    output_layouts: Vec<Layout>,
}

/// Compiles pending nodes into steps one at a time, in schedule order. Each step passes through
/// the same phases: work out its layouts, create and prepare its node instances, line up latency,
/// bind inputs and parameters, and allocate its buffers.
struct Compiler<'a> {
    registry: &'a Registry,
    options: &'a CompileOptions,
    pending: Vec<Pending>,
    output: usize,
    /// Output layouts of each node compiled so far, by pending index.
    layouts: Vec<Vec<Layout>>,
    /// Latency of each node's outputs relative to the sources, in frames.
    latency: Vec<f64>,
    /// Each pending node's index in `steps`.
    step_of: Vec<usize>,
    steps: Vec<Step>,
    sources: Vec<(String, Layout)>,
    warmup_frames: u32,
    latency_frames: u32,
}

impl<'a> Compiler<'a> {
    fn new(
        registry: &'a Registry,
        options: &'a CompileOptions,
        pending: Vec<Pending>,
        output: usize,
    ) -> Self {
        let count = pending.len();
        Self {
            registry,
            options,
            pending,
            output,
            layouts: vec![Vec::new(); count],
            latency: vec![0.0; count],
            step_of: vec![usize::MAX; count],
            steps: Vec::new(),
            sources: Vec::new(),
            warmup_frames: 0,
            latency_frames: 0,
        }
    }

    fn finish(self) -> Graph {
        Graph {
            output_step: self.step_of[self.output],
            steps: self.steps,
            frame_rate: self.options.frame_rate,
            sources: self.sources,
            latency_frames: self.latency_frames,
            warmup_frames: self.warmup_frames,
        }
    }

    fn node_error(&self, n: usize, message: String) -> GraphError {
        GraphError::Node {
            node: self.pending[n].id.clone(),
            message,
        }
    }

    fn add_step(&mut self, n: usize) -> Result<(), GraphError> {
        let shape = self.shape(n)?;
        let (nodes, own_latency) = self.instantiate(n, &shape)?;
        let aligned = self.align_latency(n, own_latency);
        let inputs = self.bind_inputs(n, &shape, aligned);
        let params = self.bind_params(n, &shape, aligned);

        let p = &self.pending[n];
        let split = (shape.channels > 1).then(|| {
            let channel = Signal::zeros(shape.channel_layout.unwrap());
            ChannelSplit {
                inputs: vec![vec![channel.clone(); p.wires.len()]; shape.channels],
                params: vec![vec![channel; params.len()]; shape.channels],
                outputs: vec![
                    shape
                        .node_outputs
                        .iter()
                        .map(|&l| Signal::zeros(l))
                        .collect();
                    shape.channels
                ],
            }
        });
        self.step_of[n] = self.steps.len();
        self.steps.push(Step {
            id: p.id.as_str().into(),
            nodes,
            split,
            interpolation: p.interpolation,
            inputs,
            params,
            levels: vec![0.0; shape.output_layouts.len()],
            outputs: shape
                .output_layouts
                .iter()
                .map(|&l| Signal::zeros(l))
                .collect(),
        });
        self.layouts[n] = shape.output_layouts;
        Ok(())
    }

    /// Phase 1: the layouts the node and its inputs have.
    fn shape(&self, n: usize) -> Result<NodeShape, GraphError> {
        let p = &self.pending[n];
        let main = p.wires.first().map(|w| {
            let (src, port) = w.expect("main inputs are required");
            self.layouts[src][port]
        });
        let input_layouts: Vec<Layout> = p
            .wires
            .iter()
            .map(|w| w.map_or(main.unwrap(), |(src, port)| self.layouts[src][port]))
            .collect();

        // Separate channels: one node per channel of the main input, each seeing a mono
        // signal. Every input reaches each copy as that one channel.
        let channels = match (p.channels, main) {
            (Channels::Separate, Some(main)) if main.samples_per_pixel > 1 => {
                let per_channel = self
                    .registry
                    .get(&p.kind)
                    .is_some_and(|t| t.spec.per_channel);
                if !per_channel {
                    return Err(self.node_error(n, "can't process channels separately".into()));
                }
                main.samples_per_pixel as usize
            }
            _ => 1,
        };
        let channel_layout = main.map(|m| {
            if channels > 1 {
                Layout::mono(m.width, m.height)
            } else {
                m
            }
        });

        let node_inputs = if channels > 1 {
            vec![channel_layout.unwrap(); p.wires.len()]
        } else {
            input_layouts.clone()
        };
        let node_outputs = p
            .node
            .output_layouts(&LayoutContext {
                inputs: &node_inputs,
                sources: &self.options.sources,
                output: self.options.output,
                output_count: p.outputs.len(),
            })
            .map_err(|message| self.node_error(n, message))?;
        assert_eq!(
            node_outputs.len(),
            p.outputs.len(),
            "node `{}` returned the wrong number of layouts",
            p.id
        );
        let output_layouts = if channels > 1 {
            if node_outputs.iter().any(|&l| Some(l) != channel_layout) {
                return Err(self.node_error(n, "can't process channels separately".into()));
            }
            vec![main.unwrap(); node_outputs.len()]
        } else {
            node_outputs.clone()
        };
        // A node without inputs measures its signals against its own output.
        let reference = main.unwrap_or_else(|| output_layouts[0]);
        Ok(NodeShape {
            reference,
            input_layouts,
            channels,
            channel_layout,
            node_outputs,
            output_layouts,
        })
    }

    /// Phase 2: creates the node instances (one per channel when split) and prepares them.
    /// Returns them with the node's own latency in frames. Also records the node's warmup and, for
    /// source nodes, the signal it reads.
    fn instantiate(
        &mut self,
        n: usize,
        shape: &NodeShape,
    ) -> Result<(Vec<Box<dyn Node>>, f64), GraphError> {
        let p = &self.pending[n];
        // Every input reaches the node at the main input's layout (per channel, if split).
        let matched = vec![shape.channel_layout.unwrap_or_default(); p.wires.len()];
        let connected: Vec<bool> = p.wires.iter().map(Option::is_some).collect();
        let modulated: Vec<Option<(f64, f64)>> = (0..p.specs.len())
            .map(|i| {
                p.modulation_of(i)
                    .map(|(base, m, _)| p.specs[i].modulated_range(base, m))
            })
            .collect();
        let ctx = PrepareContext {
            frame_rate: self.options.frame_rate,
            tempo: self.options.tempo,
            inputs: &matched,
            outputs: &shape.node_outputs,
            connected: &connected,
            modulated: &modulated,
        };

        let mut nodes = vec![std::mem::replace(
            &mut self.pending[n].node,
            Box::new(Placeholder),
        )];
        for _ in 1..shape.channels {
            let copy = self
                .registry
                .create(&self.pending[n].kind, &self.pending[n].params)
                .expect("the node type exists")
                .map_err(|message| self.node_error(n, message))?;
            nodes.push(copy);
        }
        for node in &mut nodes {
            node.prepare(&ctx);
        }
        let node = &nodes[0];
        self.warmup_frames = self.warmup_frames.max(node.warmup_frames(&ctx));
        let own_latency = node.latency(&ctx) as f64 / ctx.samples_per_frame().max(1) as f64;
        if let Some(name) = node.source() {
            self.sources
                .push((name.to_owned(), shape.output_layouts[0]));
        }
        Ok((nodes, own_latency))
    }

    /// Phase 3: inputs that arrive with less latency than the latest one are delayed to line up.
    /// Returns the latency the node's inputs line up at, in frames, and records the node's own.
    fn align_latency(&mut self, n: usize, own_latency: f64) -> f64 {
        let mut aligned = self.pending[n]
            .sources()
            .map(|&(src, _)| self.latency[src])
            .fold(0.0, f64::max);
        if n == self.output {
            // The output's total latency is rounded up to whole frames so the host can skip
            // exactly that many frames.
            aligned = (aligned - 1e-9).ceil().max(0.0);
            self.latency_frames = aligned as u32;
        }
        self.latency[n] = aligned + own_latency;
        aligned
    }

    /// How one input (or parameter) gets its signal: latency compensation, then resampling to the
    /// reference length.
    fn binding(
        &self,
        shape: &NodeShape,
        aligned: f64,
        wire: Option<(usize, usize)>,
        src_layout: Layout,
        secondary: bool,
    ) -> InputBinding {
        let compensation = wire.and_then(|(src, _)| {
            let delay = ((aligned - self.latency[src]) * src_layout.len() as f64).round() as usize;
            (delay > 0).then(|| (DelayLine::new(delay), delay, Signal::zeros(src_layout)))
        });
        let needs_resampling =
            secondary && (wire.is_none() || src_layout.len() != shape.reference.len());
        let group = if src_layout.samples_per_pixel == 1 && shape.reference.samples_per_pixel > 1 {
            shape.reference.samples_per_pixel as usize
        } else {
            1
        };
        InputBinding {
            source: wire.map(|(src, port)| (self.step_of[src], port)),
            compensation,
            resampled: needs_resampling.then(|| Signal::zeros(shape.reference)),
            group,
        }
    }

    /// Phase 4a: the node's inputs. Every one but the main input is resampled to its length.
    fn bind_inputs(&self, n: usize, shape: &NodeShape, aligned: f64) -> Vec<InputBinding> {
        self.pending[n]
            .wires
            .iter()
            .enumerate()
            .map(|(k, &w)| self.binding(shape, aligned, w, shape.input_layouts[k], k > 0))
            .collect()
    }

    /// Phase 4b: the signals modulating the node's parameters, each with a buffer of its
    /// per-sample values.
    fn bind_params(&self, n: usize, shape: &NodeShape, aligned: f64) -> Vec<ParamBinding> {
        let p = &self.pending[n];
        (0..p.specs.len())
            .filter_map(|index| {
                let (base, modulation, limits) = p.modulation_of(index)?;
                let (src, port) = p.param_wires[index]?;
                Some(ParamBinding {
                    index,
                    spec: &p.specs[index],
                    input: self.binding(
                        shape,
                        aligned,
                        Some((src, port)),
                        self.layouts[src][port],
                        true,
                    ),
                    base,
                    modulation,
                    limits,
                    values: Signal::zeros(shape.reference),
                })
            })
            .collect()
    }
}

/// RMS over an evenly spread subset of at most [`LEVEL_SAMPLES`] samples.
fn rms(data: &[f32]) -> f32 {
    if data.is_empty() {
        return 0.0;
    }
    let stride = data.len().div_ceil(LEVEL_SAMPLES);
    let (sum, count) = data
        .iter()
        .step_by(stride)
        .fold((0.0f64, 0usize), |(s, c), &x| {
            (s + f64::from(x) * f64::from(x), c + 1)
        });
    (sum / count as f64).sqrt() as f32
}

/// Stands in for a node while it is being prepared.
struct Placeholder;

impl Node for Placeholder {
    fn process(&mut self, _: &ProcessContext, _: &[&Signal], _: &mut [Signal]) {}
}

fn create_nodes(desc: &GraphDesc, registry: &Registry) -> Result<Vec<Pending>, GraphError> {
    let mut pending: Vec<Pending> = Vec::with_capacity(desc.nodes.len());
    for d in &desc.nodes {
        let node_error = |message: String| GraphError::Node {
            node: d.id.clone(),
            message,
        };
        if d.id.is_empty() || d.id.contains('.') {
            return Err(node_error(
                "node ids must be non-empty and contain no `.`".into(),
            ));
        }
        if pending.iter().any(|p| p.id == d.id) {
            return Err(GraphError::DuplicateId(d.id.clone()));
        }
        let node = registry
            .create(&d.kind, &d.params)
            .ok_or_else(|| GraphError::UnknownNodeType {
                id: d.id.clone(),
                kind: d.kind.clone(),
            })?
            .map_err(node_error)?;
        // The registry checked the port and parameter counts when the type was registered.
        let spec = registry
            .get(&d.kind)
            .expect("created above, so the type exists")
            .spec;
        let specs = spec.params;
        for name in d.modulation.keys() {
            if !specs.iter().any(|s| s.name == name && s.modulatable) {
                return Err(node_error(format!(
                    "`{name}` isn't a parameter that can be modulated"
                )));
            }
        }
        pending.push(Pending {
            id: d.id.clone(),
            kind: d.kind.clone(),
            params: d.params.clone(),
            specs,
            inputs: spec.inputs,
            outputs: spec.outputs,
            modulation: d.modulation.clone(),
            wires: vec![None; spec.inputs.len()],
            param_wires: vec![None; specs.len()],
            node,
            interpolation: d.interpolation,
            channels: d.channels,
        });
    }
    Ok(pending)
}

fn connect(desc: &GraphDesc, pending: &mut [Pending]) -> Result<(), GraphError> {
    for c in &desc.connections {
        let connection_error = |message: String| GraphError::Connection {
            connection: format!("{} -> {}", c.from, c.to),
            message,
        };
        let find = |id: &str| {
            pending
                .iter()
                .position(|p| p.id == id)
                .ok_or_else(|| connection_error(format!("no node named `{id}`")))
        };
        let (from_id, from_port) = split_endpoint(&c.from);
        let (to_id, to_port) = split_endpoint(&c.to);
        let (from, to) = (find(from_id)?, find(to_id)?);

        let outputs = pending[from].outputs;
        let out = match from_port {
            Some(name) => outputs.iter().position(|o| o.name == name),
            None => (!outputs.is_empty()).then_some(0),
        }
        .ok_or_else(|| {
            connection_error(format!(
                "`{}` has no output {from_port:?}; outputs are {:?}",
                pending[from].id,
                outputs.iter().map(|o| o.name).collect::<Vec<_>>()
            ))
        })?;

        // A parameter: `node.@name`.
        if let Some(param) = to_port.and_then(|p| p.strip_prefix(PARAM_PREFIX)) {
            let index = pending[to]
                .specs
                .iter()
                .position(|s| s.name == param && s.modulatable)
                .ok_or_else(|| {
                    connection_error(format!(
                        "`{}` has no parameter `{param}` that can be modulated",
                        pending[to].id
                    ))
                })?;
            if pending[to].param_wires[index].is_some() {
                return Err(connection_error(format!(
                    "parameter `{param}` is already connected"
                )));
            }
            pending[to].param_wires[index] = Some((from, out));
            continue;
        }

        let inputs = pending[to].inputs;
        let input = match to_port {
            Some(name) => inputs.iter().position(|i| i.name == name),
            None => (!inputs.is_empty()).then_some(0),
        }
        .ok_or_else(|| {
            let names: Vec<_> = inputs.iter().map(|i| i.name).collect();
            connection_error(format!(
                "`{}` has no input {to_port:?}; inputs are {names:?}",
                pending[to].id
            ))
        })?;

        if pending[to].wires[input].is_some() {
            return Err(connection_error(format!(
                "input `{}` is already connected",
                inputs[input].name
            )));
        }
        pending[to].wires[input] = Some((from, out));
    }

    for p in pending.iter() {
        // The main input defines the node's length and layout, so it is always required.
        for (k, (spec, wire)) in p.inputs.iter().zip(&p.wires).enumerate() {
            if (spec.required || k == 0) && wire.is_none() {
                return Err(GraphError::MissingInput {
                    node: p.id.clone(),
                    input: spec.name.to_owned(),
                });
            }
        }
    }
    Ok(())
}

/// Splits `"node.port"` into its parts; the port is optional.
fn split_endpoint(endpoint: &str) -> (&str, Option<&str>) {
    match endpoint.split_once('.') {
        Some((id, port)) => (id, Some(port)),
        None => (endpoint, None),
    }
}

/// Orders the nodes that feed `output` so every node comes after its inputs. Nodes that don't
/// contribute to the output are dropped.
fn schedule(pending: &[Pending], output: usize) -> Result<Vec<usize>, GraphError> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        Visiting,
        Done,
    }
    fn visit(
        n: usize,
        pending: &[Pending],
        marks: &mut [Mark],
        order: &mut Vec<usize>,
        path: &mut Vec<usize>,
    ) -> Result<(), GraphError> {
        match marks[n] {
            Mark::Done => return Ok(()),
            Mark::Visiting => {
                let start = path.iter().position(|&p| p == n).unwrap();
                return Err(GraphError::Cycle(
                    path[start..]
                        .iter()
                        .map(|&p| pending[p].id.clone())
                        .collect(),
                ));
            }
            Mark::New => {}
        }
        marks[n] = Mark::Visiting;
        path.push(n);
        for &(src, _) in pending[n].sources() {
            visit(src, pending, marks, order, path)?;
        }
        path.pop();
        marks[n] = Mark::Done;
        order.push(n);
        Ok(())
    }

    let mut marks = vec![Mark::New; pending.len()];
    let mut order = Vec::new();
    visit(output, pending, &mut marks, &mut order, &mut Vec::new())?;
    Ok(order)
}
