use std::collections::HashMap;

use crate::{Layout, Signal};

/// An input port. The first input of a node is its **main input**: it defines the node's output
/// length and layout, and every other input is resampled to its length before `process`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputSpec {
    pub name: &'static str,
    /// One sentence for tooltips and the generated reference.
    pub help: &'static str,
    /// Optional inputs that aren't connected receive silence (all zeros) at the main input's length.
    pub required: bool,
}

impl InputSpec {
    pub const fn required(name: &'static str, help: &'static str) -> Self {
        Self {
            name,
            help,
            required: true,
        }
    }

    pub const fn optional(name: &'static str, help: &'static str) -> Self {
        Self {
            name,
            help,
            required: false,
        }
    }
}

/// An output port: its name in graph files, what it carries, and what it's for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSpec {
    pub name: &'static str,
    /// One sentence for tooltips and the generated reference.
    pub help: &'static str,
    /// What the output carries, so editors can colour its wire.
    pub hint: PortHint,
}

impl OutputSpec {
    /// An output that carries whatever the node's main input carries.
    pub const fn new(name: &'static str, help: &'static str) -> Self {
        Self {
            name,
            help,
            hint: PortHint::Inherit,
        }
    }

    pub const fn hint(mut self, hint: PortHint) -> Self {
        self.hint = hint;
        self
    }
}

/// What an output carries, so editors can colour its wires. Rendering never looks at it.
///
/// A wire has a **kind** (video or audio) and may have a **part** of a signal: a colour channel
/// or a frequency band. Hints set one or both; whatever a hint leaves open comes from the node's
/// main input, so a delay on the red channel is still red video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortHint {
    /// Everything from the main input (most effects).
    Inherit,
    /// Whole RGB video.
    Rgb,
    /// One channel of video.
    Red,
    Green,
    Blue,
    /// Whole audio.
    Audio,
    /// A frequency band; the kind comes from the main input.
    Low,
    Mid,
    High,
    /// Converted to audio or video; the part comes from the main input.
    AsAudio,
    AsVideo,
}

/// Named per-frame inputs the host supplies to the graph (decoded video, audio blocks).
pub trait Sources {
    fn get(&self, name: &str) -> Option<&Signal>;
}

impl Sources for HashMap<String, Signal> {
    fn get(&self, name: &str) -> Option<&Signal> {
        HashMap::get(self, name)
    }
}

/// Context for working out a node's output layouts at compile time.
#[derive(Debug)]
pub struct LayoutContext<'a> {
    /// Layouts of the connected inputs as produced upstream (before rate matching). Unconnected
    /// optional inputs have the main input's layout.
    pub inputs: &'a [Layout],
    /// Layouts of the named sources the host will supply.
    pub sources: &'a HashMap<String, Layout>,
    /// The layout the graph's output must have.
    pub output: Layout,
    /// How many outputs the node has.
    pub output_count: usize,
}

/// Context for [`Node::prepare`], [`Node::latency`] and [`Node::warmup_frames`].
#[derive(Debug)]
pub struct PrepareContext<'a> {
    pub frame_rate: f64,
    /// Input layouts as the node will receive them: every input has the main input's layout.
    pub inputs: &'a [Layout],
    pub outputs: &'a [Layout],
    /// Which inputs are connected. Unconnected optional inputs are all zeros.
    pub connected: &'a [bool],
    /// For each parameter (in the order of the node's specs), the range its value can move over
    /// when a signal modulates it, or `None` when it's constant. Empty means none are modulated.
    pub modulated: &'a [Option<(f64, f64)>],
}

impl PrepareContext<'_> {
    /// The layout that defines this node's units: the main input, or for source nodes the first output.
    pub fn main(&self) -> Layout {
        self.inputs
            .first()
            .or(self.outputs.first())
            .copied()
            .unwrap_or_default()
    }

    pub fn samples_per_row(&self) -> usize {
        self.main().samples_per_row()
    }

    pub fn samples_per_frame(&self) -> usize {
        self.main().len()
    }

    /// Samples per second of the main signal.
    pub fn sample_rate(&self) -> f64 {
        self.samples_per_frame() as f64 * self.frame_rate
    }

    /// The range parameter `index` moves over when modulated, or `None` when it's constant.
    pub fn modulation(&self, index: usize) -> Option<(f64, f64)> {
        self.modulated.get(index).copied().flatten()
    }

    /// The highest value parameter `index` can take: `base`, or the top of its modulation range.
    pub fn param_max(&self, index: usize, base: f64) -> f64 {
        self.modulation(index).map_or(base, |(_, hi)| hi.max(base))
    }

    /// The lowest value parameter `index` can take.
    pub fn param_min(&self, index: usize, base: f64) -> f64 {
        self.modulation(index).map_or(base, |(lo, _)| lo.min(base))
    }
}

/// Context for [`Node::process`].
pub struct ProcessContext<'a> {
    /// Index of the frame being processed, counting from the start of the render.
    pub frame: u64,
    pub frame_rate: f64,
    pub sources: &'a dyn Sources,
    /// Per-sample values of modulated parameters, by parameter index (the order of the node's
    /// specs), at the main input's length; `None` (or out of range) for constant parameters,
    /// which the node reads from its own fields.
    pub params: &'a [Option<&'a [f32]>],
}

impl ProcessContext<'_> {
    /// The per-sample values of parameter `index`, if a signal modulates it.
    pub fn param(&self, index: usize) -> Option<&[f32]> {
        self.params.get(index).copied().flatten()
    }

    /// Parameter `index` as a value read sample by sample: `constant` (the node's own copy) when
    /// nothing modulates it, the modulating signal's values when something does.
    pub fn value(&self, index: usize, constant: f64) -> Value<'_> {
        match self.param(index) {
            Some(stream) => Value::Stream(stream),
            None => Value::Const(constant),
        }
    }
}

/// A parameter's value for each sample of a block. See [`ProcessContext::value`].
#[derive(Debug, Clone, Copy)]
pub enum Value<'a> {
    Const(f64),
    Stream(&'a [f32]),
}

impl Value<'_> {
    /// The value at sample `i`.
    #[inline]
    pub fn at(&self, i: usize) -> f32 {
        match self {
            Self::Const(v) => *v as f32,
            Self::Stream(s) => s[i],
        }
    }

    /// The value at sample `i`, for nodes that compute in `f64`.
    #[inline]
    pub fn at64(&self, i: usize) -> f64 {
        match self {
            Self::Const(v) => *v,
            Self::Stream(s) => f64::from(s[i]),
        }
    }

    /// Like [`Self::at`], but `convert` turns a modulating signal's value into what the node
    /// computes with (the constant is already in that form), e.g. decibels to gain.
    #[inline]
    pub fn at_with(&self, i: usize, convert: impl Fn(f32) -> f32) -> f32 {
        match self {
            Self::Const(v) => *v as f32,
            Self::Stream(s) => convert(s[i]),
        }
    }
}

impl std::fmt::Debug for ProcessContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessContext")
            .field("frame", &self.frame)
            .field("frame_rate", &self.frame_rate)
            .finish_non_exhaustive()
    }
}

/// A processing node.
///
/// Rules every node must satisfy (enforced by the property tests):
/// - **Deterministic:** the same inputs after [`Node::reset`] always give the same outputs.
/// - **Block-size independent:** a stateful node keeps its own history and never processes the
///   same sample twice, so splitting a stream into blocks differently doesn't change the output.
/// - **No allocation in `process`.** Allocate in [`Node::prepare`].
pub trait Node: Send {
    /// For source nodes, the name of the host-supplied signal they read.
    fn source(&self) -> Option<&str> {
        None
    }

    /// Output layouts for the given inputs, or an error message if the inputs don't fit this node.
    /// The default passes the main input's layout through.
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        Ok(vec![ctx.inputs[0]; ctx.output_count])
    }

    /// Called once the graph is compiled and all layouts are known. Allocate buffers here.
    fn prepare(&mut self, ctx: &PrepareContext) {
        let _ = ctx;
    }

    /// Processes exactly one frame. `inputs` are already rate-matched to the main input, and each
    /// output is pre-sized to its layout.
    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]);

    /// Clears all internal state, as if no frame had ever been processed.
    fn reset(&mut self) {}

    /// Samples of delay this node adds to its output (non-zero for nodes that look ahead).
    fn latency(&self, ctx: &PrepareContext) -> usize {
        let _ = ctx;
        0
    }

    /// Frames of history this node needs before its output is valid after a reset.
    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        let _ = ctx;
        0
    }
}
