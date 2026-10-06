//! Compiling a [`GraphDesc`] into a sequential schedule, and running it one frame at a time.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use crate::desc::{
    Channels, Connection, GeneratorLayout, GraphDesc, Grouping, Interpolation, Modulation, NodeDesc,
};
use crate::dsp::{DelayLine, resample};
use crate::nodes::{AUDIO_INPUT, AUDIO_OUTPUT, OUTPUT, Registry, VIDEO_INPUT};

use crate::{
    Diagnostic, GraphError, InputSpec, Layout, LayoutContext, Node, OutputSpec, ParamSpec,
    ParamValue, PrepareContext, ProcessContext, Range, Severity, Signal, Sources, Tag, Tempo,
};

/// The node type that fills unconnected inputs.
const CONSTANT: &str = "constant";

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
#[derive(Debug, Clone, PartialEq)]
pub struct CompileOptions {
    pub frame_rate: f64,
    /// The project's tempo, for beat and bar units.
    pub tempo: Tempo,
    /// Layouts of the signals the host will supply each frame, by source name.
    pub sources: HashMap<String, Layout>,
    /// The layout the output node must produce (the project's RGB frame).
    pub output: Layout,
    /// The size of a render pixel against a project pixel: 1 at full size, 0.5 for a
    /// half-resolution preview. The `pixel` unit follows it.
    pub pixel_scale: f64,
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

/// What compiling found out about one node: what it costs (how late its output is and how long it
/// needs to settle), what its signals are, and anything worth warning about.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeStats {
    pub node: Arc<str>,
    /// Frames the node itself delays its output by (not counting its inputs' latency).
    pub latency_frames: f64,
    /// Frames the node needs rendered before a seek for its output to be right: its real length,
    /// whatever the host will actually pre-render. [`crate::nodes::support::UNBOUNDED_WARMUP`]
    /// for a node that never settles.
    pub warmup_frames: u32,
    /// The layouts (and tags) of the node's inputs as they arrive, before rate matching.
    pub inputs: Vec<Layout>,
    /// The layouts (and tags) of the node's outputs.
    pub outputs: Vec<Layout>,
    /// Notes and warnings that didn't stop the graph from compiling.
    pub diagnostics: Vec<Diagnostic>,
}

/// A [`Diagnostic`] about one node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeDiagnostic {
    pub node: Arc<str>,
    pub severity: Severity,
    pub message: String,
}

/// A compiled graph, ready to process frames in order.
pub struct Graph {
    node_stats: Vec<NodeStats>,
    steps: Vec<Step>,
    output_step: usize,
    /// The audio output's step, if the graph has one.
    audio_step: Option<usize>,
    /// The source the audio output reads directly, when nothing processes it on the way.
    audio_passthrough: Option<String>,
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
    /// Whether each sample's value is rounded to a whole number before clamping.
    integer: bool,
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
            let value = if self.integer { value.round() } else { value };
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
    /// `params` with the integer parameters rounded: what the node instances are created with.
    /// Modulation starts from the unrounded `params`, so only its result is rounded.
    node_params: BTreeMap<String, ParamValue>,
    specs: &'static [ParamSpec],
    inputs: &'static [InputSpec],
    outputs: &'static [OutputSpec],
    modulation: BTreeMap<String, Modulation>,
    /// For each parameter, whether it is rounded to whole numbers.
    integer: Vec<bool>,
    node: Box<dyn Node>,
    interpolation: Interpolation,
    grouping: Grouping,
    channels: Channels,
    layout: GeneratorLayout,
    /// Whether the node type can run one copy per channel.
    per_channel: bool,
    /// The range the node type is designed for on its main input.
    expects: Range,
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
        spec.number_limits()?;
        let modulation = self
            .modulation
            .get(spec.name)
            .copied()
            .unwrap_or_else(|| spec.default_modulation());
        Some((base, modulation, spec.modulation_bounds(base, modulation)))
    }
}

impl Graph {
    pub fn compile(
        desc: &GraphDesc,
        registry: &Registry,
        options: &CompileOptions,
    ) -> Result<Self, GraphError> {
        let bypassed = apply_bypass(desc, registry);
        let desc = bypassed.as_ref().unwrap_or(desc);
        let active = without_idle_audio_outputs(desc);
        let desc = active.as_ref().unwrap_or(desc);
        let filled = fill_missing_inputs(desc, registry);
        let desc = filled.as_ref().unwrap_or(desc);
        let mut pending = create_nodes(desc, registry)?;
        connect(desc, &mut pending)?;

        let of_kind = |kind: &str| -> Vec<usize> {
            (0..pending.len())
                .filter(|&i| pending[i].kind == kind)
                .collect()
        };
        let outputs = of_kind(OUTPUT);
        let &[output] = outputs.as_slice() else {
            return Err(GraphError::OutputCount(outputs.len()));
        };
        let audio_outputs = of_kind(AUDIO_OUTPUT);
        if audio_outputs.len() > 1 {
            return Err(GraphError::AudioOutputCount(audio_outputs.len()));
        }
        let sinks: Vec<usize> = std::iter::once(output).chain(audio_outputs).collect();
        // Sinks are the ends of the graph: nothing may read them, so they can be compiled last.
        if let Some(reader) = pending
            .iter()
            .find(|p| p.sources().any(|(s, _)| sinks.contains(s)))
        {
            return Err(GraphError::Node {
                node: reader.id.clone(),
                message: "can't read from an output node".into(),
            });
        }
        let mut order = Vec::new();
        for &sink in &sinks {
            for n in schedule(&pending, sink)? {
                if !order.contains(&n) && !sinks.contains(&n) {
                    order.push(n);
                }
            }
        }
        order.extend(&sinks);

        // A track wired straight into the audio output is used as it is, not rendered.
        let audio_passthrough = sinks
            .get(1)
            .and_then(|&audio| pending[audio].wires[0])
            .filter(|&(src, _)| pending[src].kind == AUDIO_INPUT)
            .and_then(|(src, _)| pending[src].node.source().map(str::to_owned));

        let mut compiler = Compiler::new(registry, options, pending, sinks);
        for &n in &order {
            compiler.add_step(n)?;
        }
        let mut graph = compiler.finish();
        graph.audio_passthrough = audio_passthrough;
        Ok(graph)
    }

    /// The layouts, tags and warnings of **every** node, including those that don't feed the
    /// output, without building anything that processes. For editors, which want to show what a
    /// node carries as soon as its input is connected. Latency and warmup are left at zero.
    /// Nodes that can't be worked out (a shape a node rejects, a cycle, or anything downstream
    /// of one) are left out; a graph that can't be read at all gives nothing.
    pub fn inspect(
        desc: &GraphDesc,
        registry: &Registry,
        options: &CompileOptions,
    ) -> Vec<NodeStats> {
        let bypassed = apply_bypass(desc, registry);
        let desc = bypassed.as_ref().unwrap_or(desc);
        let filled = fill_missing_inputs(desc, registry);
        let desc = filled.as_ref().unwrap_or(desc);
        let Ok(mut pending) = create_nodes(desc, registry) else {
            return Vec::new();
        };
        if connect(desc, &mut pending).is_err() {
            return Vec::new();
        }
        // Every node after its inputs; nodes in a cycle are left out.
        let mut order = Vec::new();
        let mut seen = vec![false; pending.len()];
        for root in 0..pending.len() {
            for n in schedule(&pending, root).unwrap_or_default() {
                if !std::mem::replace(&mut seen[n], true) {
                    order.push(n);
                }
            }
        }

        let mut compiler = Compiler::new(registry, options, pending, Vec::new());
        let mut stats = Vec::new();
        for n in order {
            let p = &compiler.pending[n];
            if p.sources()
                .any(|&(src, _)| compiler.layouts[src].is_empty())
            {
                continue;
            }
            let Ok(shape) = compiler.shape(n) else {
                continue;
            };
            stats.push(NodeStats {
                node: p.id.as_str().into(),
                latency_frames: 0.0,
                warmup_frames: 0,
                inputs: shape.input_layouts,
                outputs: shape.output_layouts.clone(),
                diagnostics: shape.diagnostics,
            });
            compiler.layouts[n] = shape.output_layouts;
        }
        stats
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

    /// Each node's own latency, warmup, signals and warnings, in the order they run.
    pub fn node_stats(&self) -> &[NodeStats] {
        &self.node_stats
    }

    /// Every compile note and warning, in the order the nodes run.
    pub fn diagnostics(&self) -> Vec<NodeDiagnostic> {
        self.node_stats
            .iter()
            .flat_map(|s| {
                s.diagnostics.iter().map(|d| NodeDiagnostic {
                    node: s.node.clone(),
                    severity: d.severity,
                    message: d.message.clone(),
                })
            })
            .collect()
    }

    pub fn output_layout(&self) -> Layout {
        self.steps[self.output_step].outputs[0].layout
    }

    /// The layout of the audio output's signal, if the graph has an audio output.
    pub fn audio_layout(&self) -> Option<Layout> {
        self.audio_step.map(|s| self.steps[s].outputs[0].layout)
    }

    /// The audio output's signal for the last processed frame, if the graph has one. It lines up
    /// with the frame [`Self::process`] returned: both have [`Self::latency_frames`].
    pub fn audio_output(&self) -> Option<&Signal> {
        self.audio_step.map(|s| &self.steps[s].outputs[0])
    }

    /// When an audio input is wired straight into the audio output, the source it reads: the
    /// host can then use that audio as it is instead of the rendered blocks.
    pub fn audio_passthrough(&self) -> Option<&str> {
        self.audio_passthrough.as_deref()
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
                Some(s) if s.layout.same_shape(layout) && s.data.len() == layout.len() => {}
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
    /// Warnings about the node's inputs and settings.
    diagnostics: Vec<Diagnostic>,
}

/// Compiles pending nodes into steps one at a time, in schedule order. Each step passes through
/// the same phases: work out its layouts, create and prepare its node instances, line up latency,
/// bind inputs and parameters, and allocate its buffers.
struct Compiler<'a> {
    registry: &'a Registry,
    options: &'a CompileOptions,
    pending: Vec<Pending>,
    /// The sinks, by pending index: the video output, then the audio output if there is one.
    /// They are compiled last, and all line up at the same latency.
    sinks: Vec<usize>,
    /// The latency every sink lines up at, in whole frames, once worked out.
    sink_latency: Option<f64>,
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
    node_stats: Vec<NodeStats>,
}

impl<'a> Compiler<'a> {
    fn new(
        registry: &'a Registry,
        options: &'a CompileOptions,
        pending: Vec<Pending>,
        sinks: Vec<usize>,
    ) -> Self {
        let count = pending.len();
        Self {
            registry,
            options,
            pending,
            sinks,
            sink_latency: None,
            layouts: vec![Vec::new(); count],
            latency: vec![0.0; count],
            step_of: vec![usize::MAX; count],
            steps: Vec::new(),
            sources: Vec::new(),
            warmup_frames: 0,
            latency_frames: 0,
            node_stats: Vec::new(),
        }
    }

    fn finish(self) -> Graph {
        Graph {
            node_stats: self.node_stats,
            output_step: self.step_of[self.sinks[0]],
            audio_step: self.sinks.get(1).map(|&a| self.step_of[a]),
            audio_passthrough: None,
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

    /// Phase 1: the layouts (and tags) the node and its inputs have, and what's worth warning
    /// about. Tags never fail a compile; only shapes a node can't process do.
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
        let connected: Vec<bool> = p.wires.iter().map(Option::is_some).collect();
        let mut diagnostics = Vec::new();
        let outputs_for = |inputs: &[Layout]| {
            p.node
                .output_layouts(&LayoutContext {
                    inputs,
                    connected: &connected,
                    sources: &self.options.sources,
                    output: self.options.output,
                    layout: p.layout,
                    output_count: p.outputs.len(),
                })
                .map_err(|message| self.node_error(n, message))
                .inspect(|layouts| {
                    assert_eq!(
                        layouts.len(),
                        p.outputs.len(),
                        "node `{}` returned the wrong number of layouts",
                        p.id
                    );
                })
        };

        // Separate channels: one node per channel of the main input, each seeing a mono
        // signal. Every input reaches each copy as that one channel.
        let mut separate = None;
        if let (Channels::Separate, Some(main)) = (p.channels, main) {
            if main.samples_per_pixel <= 1 {
                diagnostics.push(Diagnostic::note(
                    "Separate channels has no effect here: the signal has one channel.",
                ));
            } else if !p.per_channel {
                diagnostics.push(Diagnostic::warning(
                    "This node can't run once per channel, so the channels are processed together.",
                ));
            } else {
                let mut channel = main.reshaped(main.width, main.height, 1);
                channel.tag.part = main.tag.part;
                let node_outputs = outputs_for(&vec![channel; p.wires.len()])?;
                if node_outputs.iter().all(|l| l.same_shape(&channel)) {
                    separate = Some((main.samples_per_pixel as usize, channel, node_outputs));
                } else {
                    diagnostics.push(Diagnostic::warning(
                        "This node changes the signal's shape, so the channels are processed together.",
                    ));
                }
            }
        }

        let (channels, channel_layout, node_outputs, raw_outputs) = match separate {
            Some((channels, channel, node_outputs)) => {
                // Interleaved again: the main input's shape and channels, carrying what each
                // copy's output is.
                let main = main.unwrap();
                let outputs = node_outputs
                    .iter()
                    .map(|l| {
                        main.with_tag(Tag {
                            channels: main.tag.channels,
                            ..l.tag
                        })
                    })
                    .collect();
                (channels, Some(channel), node_outputs, outputs)
            }
            None => {
                let outputs = outputs_for(&input_layouts)?;
                (1, main, outputs.clone(), outputs)
            }
        };
        let output_layouts: Vec<Layout> = raw_outputs
            .into_iter()
            .zip(p.outputs)
            .map(|(layout, spec)| {
                layout.with_tag(spec.tag.apply(layout.tag, layout.samples_per_pixel))
            })
            .collect();

        if let Some(main) = main {
            let got = main.tag.range;
            if p.expects != Range::Unknown && got != Range::Unknown && got != p.expects {
                diagnostics.push(Diagnostic::note(range_note(p.expects, got)));
            }
            diagnostics.extend(p.node.diagnostics(&LayoutContext {
                inputs: &input_layouts,
                connected: &connected,
                sources: &self.options.sources,
                output: self.options.output,
                layout: p.layout,
                output_count: p.outputs.len(),
            }));
        }

        // A node without inputs measures its signals against its own output.
        let reference = main.unwrap_or_else(|| output_layouts[0]);
        Ok(NodeShape {
            reference,
            input_layouts,
            channels,
            channel_layout,
            node_outputs,
            output_layouts,
            diagnostics,
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
                p.modulation_of(i).map(|(base, m, _)| {
                    let (lo, hi) = p.specs[i].modulated_range(base, m);
                    if p.integer[i] {
                        (lo.round(), hi.round())
                    } else {
                        (lo, hi)
                    }
                })
            })
            .collect();
        let ctx = PrepareContext {
            frame_rate: self.options.frame_rate,
            tempo: self.options.tempo,
            inputs: &matched,
            outputs: &shape.node_outputs,
            connected: &connected,
            modulated: &modulated,
            pixel_scale: self.options.pixel_scale,
        };

        let mut nodes = vec![std::mem::replace(
            &mut self.pending[n].node,
            Box::new(Placeholder),
        )];
        for _ in 1..shape.channels {
            let copy = self
                .registry
                .create(&self.pending[n].kind, &self.pending[n].node_params)
                .expect("the node type exists")
                .map_err(|message| self.node_error(n, message))?;
            nodes.push(copy);
        }
        for node in &mut nodes {
            node.prepare(&ctx);
        }
        let node = &nodes[0];
        let warmup = node.warmup_frames(&ctx);
        self.warmup_frames = self.warmup_frames.max(warmup);
        let own_latency = node.latency(&ctx) as f64 / ctx.samples_per_frame().max(1) as f64;
        self.node_stats.push(NodeStats {
            node: self.pending[n].id.as_str().into(),
            latency_frames: own_latency,
            warmup_frames: warmup,
            inputs: shape.input_layouts.clone(),
            outputs: shape.output_layouts.clone(),
            diagnostics: shape.diagnostics.clone(),
        });
        if let Some(name) = node.source() {
            self.sources
                .push((name.to_owned(), shape.output_layouts[0]));
        }
        Ok((nodes, own_latency))
    }

    /// The latency all sinks line up at: the latest of their inputs, in whole frames. Sinks are
    /// compiled after everything else, so their inputs' latencies are known.
    fn sink_latency(&mut self) -> f64 {
        if let Some(latency) = self.sink_latency {
            return latency;
        }
        let latest = self
            .sinks
            .iter()
            .flat_map(|&s| self.pending[s].sources())
            .map(|&(src, _)| self.latency[src])
            .fold(0.0, f64::max);
        let latency = (latest - 1e-9).ceil().max(0.0);
        self.sink_latency = Some(latency);
        latency
    }

    /// Phase 3: inputs that arrive with less latency than the latest one are delayed to line up.
    /// Returns the latency the node's inputs line up at, in frames, and records the node's own.
    fn align_latency(&mut self, n: usize, own_latency: f64) -> f64 {
        let mut aligned = self.pending[n]
            .sources()
            .map(|&(src, _)| self.latency[src])
            .fold(0.0, f64::max);
        if self.sinks.contains(&n) {
            // Every sink lines up at the latest one's latency, rounded up to whole frames, so
            // the host skips exactly that many frames and sound and picture stay in sync.
            aligned = self.sink_latency();
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
        grouping: Grouping,
    ) -> InputBinding {
        let compensation = wire.and_then(|(src, _)| {
            let delay = ((aligned - self.latency[src]) * src_layout.len() as f64).round() as usize;
            (delay > 0).then(|| (DelayLine::new(delay), delay, Signal::zeros(src_layout)))
        });
        let needs_resampling =
            secondary && (wire.is_none() || src_layout.len() != shape.reference.len());
        let group = if grouping == Grouping::Pixels
            && src_layout.samples_per_pixel == 1
            && shape.reference.samples_per_pixel > 1
        {
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
        let grouping = self.pending[n].grouping;
        self.pending[n]
            .wires
            .iter()
            .enumerate()
            .map(|(k, &w)| self.binding(shape, aligned, w, shape.input_layouts[k], k > 0, grouping))
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
                        p.grouping,
                    ),
                    base,
                    modulation,
                    limits,
                    integer: p.integer[index],
                    values: Signal::zeros(shape.reference),
                })
            })
            .collect()
    }
}

/// The note for a signal whose range differs from the one a node is tuned for: what to expect,
/// and the conversion to use if the usual behaviour is wanted.
fn range_note(expects: Range, got: Range) -> String {
    let convert = match expects {
        Range::Bipolar => " Video to Audio converts it if you want the usual behaviour.",
        Range::Unipolar => " Audio to Video converts it if you want the usual behaviour.",
        Range::Unknown => "",
    };
    format!(
        "Tuned for values from {}; this signal runs {}, so levels and thresholds act differently.{convert}",
        expects.label().unwrap_or_default(),
        got.label().unwrap_or_default()
    )
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
        let spec = registry
            .get(&d.kind)
            .ok_or_else(|| GraphError::UnknownNodeType {
                id: d.id.clone(),
                kind: d.kind.clone(),
            })?
            .spec;
        // The registry checked the port and parameter counts when the type was registered.
        let specs = spec.params;
        // Rounded parameters: the node is created with whole numbers, the default included.
        let mut node_params = d.params.clone();
        let mut integer = vec![false; specs.len()];
        let always_whole = specs
            .iter()
            .filter(|s| s.integer && s.number_limits().is_some())
            .map(|s| s.name);
        for name in d.integer.iter().map(String::as_str).chain(always_whole) {
            let Some(index) = specs
                .iter()
                .position(|s| s.name == name && s.number_limits().is_some())
            else {
                return Err(node_error(format!("`{name}` isn't a number parameter")));
            };
            integer[index] = true;
            if let Some(value) = specs[index].number_value(&d.params) {
                node_params.insert(name.to_owned(), ParamValue::Number(value.round()));
            }
        }
        let node = registry
            .create(&d.kind, &node_params)
            .expect("the type exists, checked above")
            .map_err(node_error)?;
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
            node_params,
            specs,
            inputs: spec.inputs,
            outputs: spec.outputs,
            modulation: d.modulation.clone(),
            integer,
            wires: vec![None; spec.inputs.len()],
            param_wires: vec![None; specs.len()],
            node,
            interpolation: d.interpolation,
            grouping: d.grouping,
            channels: d.channels,
            layout: d.layout,
            per_channel: spec.per_channel,
            expects: spec.expects,
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

/// The part of `desc` that decides what is rendered: bypassed nodes rewired away, nodes that don't
/// feed the output dropped, and everything that only matters to the editor (labels, positions,
/// exposed pins, modulation settings of unconnected parameters) cleared. Two graphs with equal
/// render forms render identically, so the host re-renders only when it changes.
///
/// With `bypass_all` the graph is skipped: the first video input feeds the output directly.
pub fn render_form(desc: &GraphDesc, registry: &Registry, bypass_all: bool) -> GraphDesc {
    let mut form = if bypass_all {
        passthrough(desc)
    } else {
        let bypassed = apply_bypass(desc, registry);
        let desc = bypassed.as_ref().unwrap_or(desc);
        contributing(desc)
    };
    let connected: HashSet<(String, String)> = form
        .connections
        .iter()
        .filter_map(|c| {
            let (id, port) = split_endpoint(&c.to);
            let param = port?.strip_prefix(PARAM_PREFIX)?;
            Some((id.to_owned(), param.to_owned()))
        })
        .collect();
    for node in &mut form.nodes {
        node.bypass = false;
        node.label = None;
        node.position = None;
        node.exposed = None;
        node.modulation
            .retain(|param, _| connected.contains(&(node.id.clone(), param.clone())));
    }
    form
}

/// Just the first video input wired to the output.
fn passthrough(desc: &GraphDesc) -> GraphDesc {
    let video = desc.nodes.iter().find(|n| n.kind == VIDEO_INPUT);
    let output = desc.nodes.iter().find(|n| n.kind == OUTPUT);
    let nodes: Vec<NodeDesc> = video.into_iter().chain(output).cloned().collect();
    let connections = match (video, output) {
        (Some(v), Some(o)) => vec![Connection {
            from: v.id.clone(),
            to: o.id.clone(),
        }],
        _ => Vec::new(),
    };
    GraphDesc {
        version: desc.version,
        nodes,
        connections,
    }
}

/// Whether anything is connected to node `id`'s inputs or parameters.
fn is_fed(desc: &GraphDesc, id: &str) -> bool {
    desc.connections
        .iter()
        .any(|c| split_endpoint(&c.to).0 == id)
}

/// `desc` without audio outputs that have nothing connected: those leave the source audio as it
/// is, rather than outputting silence. `None` when there are none.
fn without_idle_audio_outputs(desc: &GraphDesc) -> Option<GraphDesc> {
    let idle = |n: &NodeDesc| n.kind == AUDIO_OUTPUT && !is_fed(desc, &n.id);
    desc.nodes.iter().any(idle).then(|| GraphDesc {
        version: desc.version,
        nodes: desc.nodes.iter().filter(|n| !idle(n)).cloned().collect(),
        connections: desc.connections.clone(),
    })
}

/// The nodes that feed an output node, and the connections between them.
fn contributing(desc: &GraphDesc) -> GraphDesc {
    let mut keep: HashSet<&str> = desc
        .nodes
        .iter()
        .filter(|n| n.kind == OUTPUT || (n.kind == AUDIO_OUTPUT && is_fed(desc, &n.id)))
        .map(|n| n.id.as_str())
        .collect();
    let mut queue: Vec<&str> = keep.iter().copied().collect();
    while let Some(id) = queue.pop() {
        for c in &desc.connections {
            if split_endpoint(&c.to).0 == id {
                let from = split_endpoint(&c.from).0;
                if keep.insert(from) {
                    queue.push(from);
                }
            }
        }
    }
    GraphDesc {
        version: desc.version,
        nodes: desc
            .nodes
            .iter()
            .filter(|n| keep.contains(n.id.as_str()))
            .cloned()
            .collect(),
        connections: desc
            .connections
            .iter()
            .filter(|c| {
                keep.contains(split_endpoint(&c.from).0) && keep.contains(split_endpoint(&c.to).0)
            })
            .cloned()
            .collect(),
    }
}

/// Feeds every unconnected input that must have a signal (the main input and required ones) from
/// a hidden constant zero, shaped like the video. A half-built
/// graph then renders (black, or silence) instead of failing. Returns `None` when nothing is
/// missing.
fn fill_missing_inputs(desc: &GraphDesc, registry: &Registry) -> Option<GraphDesc> {
    let mut filled = desc.clone();
    let mut added = 0;
    for node in &desc.nodes {
        let Some(kind) = registry.get(&node.kind) else {
            continue;
        };
        for (k, input) in kind.spec.inputs.iter().enumerate() {
            if !(input.required || k == 0) {
                continue;
            }
            let connected = desc.connections.iter().any(|c| {
                let (to, port) = split_endpoint(&c.to);
                to == node.id && port.map_or(k == 0, |p| p == input.name)
            });
            if connected {
                continue;
            }
            let id = format!("~zero{added}");
            added += 1;
            filled.connections.push(Connection {
                from: id.clone(),
                to: format!("{}.{}", node.id, input.name),
            });
            filled.nodes.push(NodeDesc {
                id,
                kind: CONSTANT.to_owned(),
                params: BTreeMap::new(),
                interpolation: Interpolation::default(),
                grouping: Grouping::default(),
                channels: Channels::default(),
                layout: GeneratorLayout::default(),
                bypass: false,
                label: None,
                position: None,
                modulation: BTreeMap::new(),
                integer: Vec::new(),
                exposed: None,
            });
        }
    }
    (added > 0).then_some(filled)
}

/// Removes bypassed nodes: whatever read a bypassed node's first output reads its main input's
/// source instead, and its other outputs and parameter wires go nowhere. Returns `None` when
/// nothing is bypassed. The output node and nodes without both an input and an output (which
/// have nothing to pass through) are never bypassed.
fn apply_bypass(desc: &GraphDesc, registry: &Registry) -> Option<GraphDesc> {
    let passes_through = |d: &NodeDesc| {
        d.bypass
            && d.kind != OUTPUT
            && registry
                .get(&d.kind)
                .is_some_and(|k| !k.spec.inputs.is_empty() && !k.spec.outputs.is_empty())
    };
    let bypassed: Vec<&NodeDesc> = desc.nodes.iter().filter(|d| passes_through(d)).collect();
    if bypassed.is_empty() {
        return None;
    }
    let kind_of = |id: &str| {
        desc.nodes
            .iter()
            .find(|d| d.id == id)
            .and_then(|d| registry.get(&d.kind))
    };
    let is_bypassed = |id: &str| bypassed.iter().any(|d| d.id == id);
    // The first output's name, as a connection may leave the port out.
    let first_output = |id: &str| kind_of(id).and_then(|k| k.spec.outputs.first().map(|o| o.name));
    let main_input = |id: &str| kind_of(id).and_then(|k| k.spec.inputs.first().map(|i| i.name));

    // What a bypassed node's main input is fed by.
    let feed = |id: &str| {
        desc.connections.iter().find(|c| {
            let (to, port) = split_endpoint(&c.to);
            to == id && port.is_none_or(|p| Some(p) == main_input(id))
        })
    };
    let mut connections = Vec::new();
    for c in &desc.connections {
        let (to_id, _) = split_endpoint(&c.to);
        if is_bypassed(to_id) {
            continue;
        }
        let mut from = c.from.clone();
        // Follow chains of bypassed nodes back to a real source. Each hop moves upstream, so a
        // cycle of bypassed nodes ends by running out of hops.
        let mut hops = 0;
        let mut dropped = false;
        loop {
            let (id, port) = split_endpoint(&from);
            if !is_bypassed(id) {
                break;
            }
            if port.is_some_and(|p| Some(p) != first_output(id)) || hops > desc.nodes.len() {
                dropped = true;
                break;
            }
            match feed(id) {
                Some(upstream) => from = upstream.from.clone(),
                None => {
                    dropped = true;
                    break;
                }
            }
            hops += 1;
        }
        if !dropped {
            connections.push(Connection {
                from,
                to: c.to.clone(),
            });
        }
    }
    Some(GraphDesc {
        version: desc.version,
        nodes: desc
            .nodes
            .iter()
            .filter(|d| !is_bypassed(&d.id))
            .cloned()
            .collect(),
        connections,
    })
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
