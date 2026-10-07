use std::collections::HashMap;

use crate::{Layout, Signal, TagRule};

/// An input port. The first input of a node is its **main input**: it defines the node's output
/// length and layout, and every other input is resampled to its length before `process`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputSpec {
    pub name: &'static str,
    /// Optional inputs that aren't connected receive silence (all zeros) at the main input's length.
    pub required: bool,
}

impl InputSpec {
    pub const fn required(name: &'static str) -> Self {
        Self {
            name,
            required: true,
        }
    }

    pub const fn optional(name: &'static str) -> Self {
        Self {
            name,
            required: false,
        }
    }
}

/// An output port: its name in graph files, what it carries, and what it's for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSpec {
    pub name: &'static str,
    /// How the output's [`crate::Tag`] is set from what the node produced. Whatever the rule
    /// leaves open comes from the node (by default its main input), so a delay on the red channel
    /// is still red video.
    pub tag: TagRule,
}

impl OutputSpec {
    /// An output that carries whatever the node's main input carries.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            tag: TagRule::INHERIT,
        }
    }

    pub const fn tag(mut self, tag: TagRule) -> Self {
        self.tag = tag;
        self
    }
}

/// How much a [`Diagnostic`] matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Something worth knowing, not a mistake: a signal used as something it wasn't made as
    /// (often the point, in a glitch), or a setting that has no effect here.
    Note,
    /// Something is lost or a setting can't be honoured: channels dropped, a setting ignored.
    Warning,
}

/// A remark about a node found while compiling. Never stops processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
}

impl Diagnostic {
    pub fn note(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Note,
            message: message.into(),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
        }
    }
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
    /// Layouts of the connected inputs as produced upstream (before rate matching), tags
    /// included. Unconnected optional inputs have the main input's layout.
    pub inputs: &'a [Layout],
    /// Which inputs are connected.
    pub connected: &'a [bool],
    /// Layouts of the named sources the host will supply.
    pub sources: &'a HashMap<String, Layout>,
    /// The layout the graph's output must have.
    pub output: Layout,
    /// The node's `layout` setting: which host signal a generator is shaped like.
    pub layout: crate::GeneratorLayout,
    /// How many outputs the node has.
    pub output_count: usize,
}

/// The project's musical tempo, which beat and bar units are measured in. Constant for now; a
/// tempo map can replace it later without changing what nodes see.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tempo {
    /// Beats per minute.
    pub bpm: f64,
    /// Beats in a bar (the time signature's numerator).
    pub beats_per_bar: u32,
    /// Seconds from the start of the video to the first beat.
    #[serde(default)]
    pub offset_secs: f64,
}

impl Default for Tempo {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            beats_per_bar: 4,
            offset_secs: 0.0,
        }
    }
}

impl Tempo {
    pub const MIN_BPM: f64 = 20.0;
    pub const MAX_BPM: f64 = 400.0;

    /// The same tempo with values forced into usable ranges, so a hand-edited file can't make
    /// a unit conversion divide by zero.
    pub fn sanitized(self) -> Self {
        Self {
            bpm: if self.bpm.is_finite() {
                self.bpm.clamp(Self::MIN_BPM, Self::MAX_BPM)
            } else {
                Self::default().bpm
            },
            beats_per_bar: self.beats_per_bar.clamp(1, 64),
            offset_secs: if self.offset_secs.is_finite() {
                self.offset_secs
            } else {
                0.0
            },
        }
    }

    pub fn seconds_per_beat(&self) -> f64 {
        60.0 / self.sanitized().bpm
    }

    pub fn seconds_per_bar(&self) -> f64 {
        self.seconds_per_beat() * f64::from(self.sanitized().beats_per_bar)
    }
}

/// Context for [`Node::prepare`], [`Node::latency`] and [`Node::warmup_frames`].
#[derive(Debug)]
pub struct PrepareContext<'a> {
    pub frame_rate: f64,
    pub tempo: Tempo,
    /// Input layouts as the node will receive them: every input has the main input's layout.
    pub inputs: &'a [Layout],
    pub outputs: &'a [Layout],
    /// Which inputs are connected. Unconnected optional inputs are all zeros.
    pub connected: &'a [bool],
    /// For each parameter (in the order of the node's specs), the range its value can move over
    /// when a signal modulates it, or `None` when it's constant. Empty means none are modulated.
    pub modulated: &'a [Option<(f64, f64)>],
    /// How big a render pixel is next to a project pixel's width: 0.5 for a half-resolution
    /// preview, 1 at full size. Only video signals scale; see [`Self::samples_per_pixel`].
    pub pixel_scale: f64,
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

    /// Samples in one pixel of the project's own resolution: the pixel's channels, scaled down
    /// with the preview for video so a small preview matches the full render. Anything that
    /// isn't video (audio) has no preview scale, and a pixel is one frame of its channels.
    pub fn samples_per_pixel(&self) -> f64 {
        let main = self.main();
        let scale = if main.tag.kind == crate::Kind::Video {
            self.pixel_scale
        } else {
            1.0
        };
        f64::from(main.samples_per_pixel) * scale
    }

    pub fn samples_per_frame(&self) -> usize {
        self.main().len()
    }

    /// Samples per second of the main signal.
    pub fn sample_rate(&self) -> f64 {
        self.samples_per_frame() as f64 * self.frame_rate
    }

    pub fn samples_per_beat(&self) -> f64 {
        self.tempo.seconds_per_beat() * self.sample_rate()
    }

    pub fn samples_per_bar(&self) -> f64 {
        self.tempo.seconds_per_bar() * self.sample_rate()
    }

    /// Samples from the start of the render to the first beat (negative if the first beat is
    /// before the start).
    pub fn beat_offset_samples(&self) -> f64 {
        self.tempo.sanitized().offset_secs * self.sample_rate()
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

    /// Output layouts for the given inputs, or an error message if the inputs can't be processed
    /// at all. The default passes the main input's layout, tag included, through.
    ///
    /// The tag is advisory: never fail because of it (see [`Self::diagnostics`]). Nodes that
    /// change the shape use [`Layout::reshaped`] to keep the tag; nodes that set a range (a
    /// conversion, a clamp, a generator) write it here.
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        Ok(vec![ctx.inputs[0]; ctx.output_count])
    }

    /// Remarks about the inputs that don't stop processing. Mostly [`Severity::Note`]s: a signal
    /// whose tag doesn't match what the node is meant for is often a deliberate glitch.
    fn diagnostics(&self, ctx: &LayoutContext) -> Vec<Diagnostic> {
        let _ = ctx;
        Vec::new()
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

    /// Frames of history this node needs before its output is valid after a reset: its real
    /// length, never shortened to what the host is willing to pre-render (the host applies its
    /// own limit). `UNBOUNDED_WARMUP` if it never settles.
    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        let _ = ctx;
        0
    }
}
