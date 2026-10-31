//! The composition's runtime, run once per source frame: each track's stem, the video and audio
//! mix, then the graph tracks from the bottom up. See
//! [Composition](../../../docs/engine.md#composition).
//!
//! A graph track shows its item's output where one plays and what is below it elsewhere; each
//! item's input nodes read the sources they name (what is below, the mix, a stem, a graph track
//! further down, or one of its named outputs). The top graph track's output is the master, or the
//! mix with none.
//!
//! Every stage has a latency: the frames between a source frame going in and its result coming
//! out. A stage's output for output frame `f` comes out at source step `f + latency`, and where
//! signals of different latencies meet (an item's sources, a graph track's items) the earlier
//! ones wait in rings so everything lines up.

use std::collections::{HashMap, VecDeque};

use rastersong_graph::nodes::{AUDIO_OUTPUT, DEFAULT_AUDIO, DEFAULT_VIDEO};
use rastersong_graph::{
    CompileOptions, Graph, GraphDesc, GraphError, Layout, NodeCost, NodeMeters, NodeStats,
    OutputLevel, ParamLevel, Registry, Signal, Sources, Span, audio_output_bus,
};

use crate::EngineError;
use crate::composition::{Composition, InputKind, input_ports};
use crate::timeline::{GraphItem, Source, TrackKind};

/// The sound the mix and the graph tracks are in: this many samples a second, in the bus's
/// channels, one block per frame. What leaves the render is resampled to the project's audio
/// rate.
pub const MIX_RATE: f64 = 48_000.0;

/// The layout the mix's sound is in, for `channels` at `fps`.
pub fn mix_layout(channels: u32, fps: f64) -> Layout {
    Layout::audio_channels((MIX_RATE / fps).round().max(1.0) as u32, channels.max(1))
}

/// What flows from node to node for one frame.
#[derive(Debug, Clone, Default)]
pub(crate) struct Stream {
    /// The picture, in the render's layout; empty for black.
    pub video: Vec<f32>,
    /// Whether there is a picture: the mix shows the top track that has one.
    pub picture: bool,
    /// The sound, in the stage's audio layout; empty for silence.
    pub audio: Vec<f32>,
    /// A graph track's named outputs, in its slots' order; empty for zeros.
    pub named: Vec<Vec<f32>>,
}

impl Stream {
    pub fn clear(&mut self) {
        self.video.clear();
        self.audio.clear();
        self.named.iter_mut().for_each(Vec::clear);
        self.picture = false;
    }

    fn assign(&mut self, other: &Self) {
        assign(&mut self.video, &other.video);
        assign(&mut self.audio, &other.audio);
        self.assign_named(&other.named);
        self.picture = other.picture;
    }

    fn assign_named(&mut self, named: &[Vec<f32>]) {
        self.named.resize_with(named.len(), Vec::new);
        for (dst, src) in self.named.iter_mut().zip(named) {
            assign(dst, src);
        }
    }
}

/// Replaces the contents of `dst` with `src`, keeping its allocation.
fn assign(dst: &mut Vec<f32>, src: &[f32]) {
    dst.clear();
    dst.extend_from_slice(src);
}

/// Streams kept by output frame, to delay one by a few frames.
#[derive(Debug, Default)]
struct Ring {
    depth: usize,
    entries: VecDeque<(i64, Stream)>,
}

impl Ring {
    fn new(depth: usize) -> Self {
        Self {
            depth,
            entries: VecDeque::new(),
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    /// Keeps `stream` as frame `frame`, forgetting the oldest beyond the depth.
    fn push(&mut self, frame: i64, stream: &Stream) {
        if self.depth == 0 {
            return;
        }
        let mut entry = if self.entries.len() >= self.depth {
            self.entries.pop_front().expect("not empty")
        } else {
            (frame, Stream::default())
        };
        entry.0 = frame;
        entry.1.assign(stream);
        self.entries.push_back(entry);
    }

    fn get(&self, frame: i64) -> Option<&Stream> {
        self.entries
            .iter()
            .rev()
            .find(|(f, _)| *f == frame)
            .map(|(_, s)| s)
    }
}

/// Adds `src` (in `from`) to `dst` (in `to`) at `gain`: resampled linearly across the block when
/// the lengths differ, a mono source in every channel, and other channels in order (those the
/// destination doesn't have are dropped). Non-finite samples are left out.
pub(crate) fn mix_into(src: &[f32], from: Layout, dst: &mut [f32], to: Layout, gain: f32) {
    let sc = from.samples_per_pixel.max(1) as usize;
    let dc = to.samples_per_pixel.max(1) as usize;
    let (sn, dn) = (src.len() / sc, dst.len() / dc);
    if sn == 0 || dn == 0 || gain == 0.0 {
        return;
    }
    for k in 0..dn {
        let (i, frac) = if sn == dn {
            (k, 0.0)
        } else {
            let pos = ((k as f64 + 0.5) * sn as f64 / dn as f64 - 0.5).clamp(0.0, (sn - 1) as f64);
            (pos as usize, (pos - pos.floor()) as f32)
        };
        let j = (i + 1).min(sn - 1);
        for c in 0..dc {
            let from_channel = match sc {
                1 => 0,
                _ if c < sc => c,
                _ => continue,
            };
            let (a, b) = (src[i * sc + from_channel], src[j * sc + from_channel]);
            let x = a + (b - a) * frac;
            if x.is_finite() {
                dst[k * dc + c] += x * gain;
            }
        }
    }
}

/// Where an item's input port gets its signal.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Fill {
    HostVideo,
    HostAudio,
    /// The output of node `node`: its picture, or its sound.
    Receive {
        node: usize,
        video: bool,
    },
    /// Named output slot `slot` of graph-track node `node`.
    Named {
        node: usize,
        slot: usize,
    },
    /// Nothing fills the port: zeros.
    Zeros,
}

/// One source signal an item's graph reads, by the name it was compiled with.
#[derive(Debug)]
struct Port {
    key: String,
    signal: Signal,
    fill: Fill,
}

/// The source signals of one item's graph.
struct PortSources<'a>(&'a [Port]);

impl Sources for PortSources<'_> {
    fn get(&self, name: &str) -> Option<&Signal> {
        self.0.iter().find(|p| p.key == name).map(|p| &p.signal)
    }
}

/// One item's graph, compiled.
struct FxRt {
    graph: Graph,
    graph_id: u32,
    /// What it was compiled against, for editors inspecting the graph the same way.
    options: CompileOptions,
    latency: usize,
    warmup: usize,
    /// The latency of its inputs: the latest of what is below and its other sources.
    align: usize,
    /// Frames what is below is held back to line up with its other sources.
    host_delay: usize,
    host_ring: Ring,
    ports: Vec<Port>,
    /// Its graph track's named output slots, each with whether the graph fills it: it has a
    /// named output of that name and the slot's layout.
    named: Vec<(String, bool)>,
    /// Whether its input had a picture, by input frame, until the output for it comes out.
    flags: VecDeque<(i64, bool)>,
    out: Stream,
    /// Whether it processed the last step.
    active: bool,
}

impl FxRt {
    fn reset(&mut self) {
        self.graph.reset();
        self.host_ring.clear();
        self.flags.clear();
        self.out.clear();
        self.active = false;
    }
}

/// The graphs an item runs in order (one, for now), compiled.
#[derive(Default)]
struct ChainRt {
    fx: Vec<FxRt>,
    /// The latency of what comes into it, and of what comes out.
    latency_in: usize,
    latency: usize,
    /// The sound's layout coming out.
    audio_layout: Layout,
}

impl ChainRt {
    fn warmup(&self) -> usize {
        self.fx.iter().map(|f| f.warmup).sum()
    }

    fn reset(&mut self) {
        self.fx.iter_mut().for_each(FxRt::reset);
    }

    /// Runs the chain for source step `m` on `host` (output frame `m - latency_in`) and puts what
    /// comes out (output frame `m - latency`) in `out`. Graphs count frames from `base`. A graph
    /// that fails is named by its id.
    fn run(
        &mut self,
        m: i64,
        host: &Stream,
        outputs: &OutputsView,
        base: i64,
        out: &mut Stream,
    ) -> Result<(), (GraphError, u32)> {
        let mut latency = self.latency_in;
        for k in 0..self.fx.len() {
            let (before, rest) = self.fx.split_at_mut(k);
            let current = before.last().map_or(host, |f| &f.out);
            let FxRt {
                graph,
                graph_id,
                latency: own,
                align,
                host_delay,
                host_ring,
                ports,
                named,
                flags,
                out: fx_out,
                active,
                ..
            } = &mut rest[0];
            let f_in = m - *align as i64;
            let host_now = if *host_delay == 0 {
                Some(current)
            } else {
                host_ring.push(m - latency as i64, current);
                host_ring.get(f_in)
            };
            for port in ports.iter_mut() {
                let source = match port.fill {
                    Fill::HostVideo => host_now.map(|h| &h.video[..]),
                    Fill::HostAudio => host_now.map(|h| &h.audio[..]),
                    Fill::Receive { node, video } => outputs
                        .get(node)
                        .and_then(|o| o.ring.get(f_in))
                        .map(|s| if video { &s.video[..] } else { &s.audio[..] }),
                    Fill::Named { node, slot } => outputs
                        .get(node)
                        .and_then(|o| o.ring.get(f_in))
                        .and_then(|s| s.named.get(slot))
                        .map(|v| &v[..]),
                    Fill::Zeros => None,
                };
                match source {
                    Some(data) if data.len() == port.signal.data.len() => {
                        port.signal.data.copy_from_slice(data);
                    }
                    _ => port.signal.data.fill(0.0),
                }
            }
            flags.push_back((f_in, host_now.is_some_and(|h| h.picture)));
            while flags.len() > *own + 1 {
                flags.pop_front();
            }
            let _span = tracing::trace_span!("graph", id = *graph_id).entered();
            graph
                .process((f_in + base).max(0) as u64, &PortSources(ports))
                .map_err(|e| (e, *graph_id))?;
            assign(&mut fx_out.video, &graph.output().data);
            match graph.audio_output() {
                Some(audio) => assign(&mut fx_out.audio, &audio.data),
                None => fx_out.audio.clear(),
            }
            fx_out.named.resize_with(named.len(), Vec::new);
            for (out, (name, fills)) in fx_out.named.iter_mut().zip(named.iter()) {
                match graph.named_output(name).filter(|_| *fills) {
                    Some(signal) => assign(out, &signal.data),
                    None => out.clear(),
                }
            }
            let f_out = f_in - *own as i64;
            fx_out.picture = flags.iter().any(|&(f, p)| f == f_out && p);
            *active = true;
            latency = *align + *own;
        }
        match self.fx.last() {
            Some(last) => out.assign(&last.out),
            None => out.assign(host),
        }
        Ok(())
    }
}

/// A graph item, compiled.
struct ItemFxRt {
    chain: ChainRt,
    /// The output frames it plays: `first..end`.
    first: i64,
    end: i64,
    /// Added to a source frame to get its graphs' own frame.
    base: i64,
    pre_roll: bool,
    /// Frames its output is held back to line up with the graph track's slowest item.
    lag: Ring,
    out: Stream,
}

impl ItemFxRt {
    /// Frames pre-rendered before the item, limited to `cap`.
    fn warm(&self, cap: usize) -> usize {
        self.chain.warmup().min(cap)
    }

    /// The first source step it processes, when what is below comes at `latency_in`: its
    /// pre-roll before the left edge, or the edge.
    fn from(&self, cap: usize, latency_in: usize) -> i64 {
        let pre = if self.pre_roll { self.warm(cap) } else { 0 };
        (self.first + latency_in as i64 - pre as i64).max(0)
    }

    /// One past the last source step it processes.
    fn until(&self) -> i64 {
        self.end.saturating_add(self.chain.latency as i64)
    }
}

/// A graph track's items: where one plays, its graph's output replaces what is below.
struct ItemsRt {
    items: Vec<ItemFxRt>,
    /// The slowest item's latency, or what is below's: the stage's.
    latency: usize,
    /// The latency of what is below.
    latency_in: usize,
    /// What is below, held back to the stage's latency.
    raw: Ring,
    /// The layouts of what is below's sound and of the stage's: every item's sound is fitted to
    /// the stage's.
    raw_layout: Layout,
    layout: Layout,
    /// How many named output slots the track has.
    slots: usize,
    out: Stream,
}

impl ItemsRt {
    /// Runs the items playing at source step `m` on what is below, `raw` (output frame
    /// `m - latency_in`), and puts the stage's output (frame `m - latency`) in `self.out`.
    fn run(
        &mut self,
        m: i64,
        raw: &Stream,
        outputs: &OutputsView,
        cap: usize,
    ) -> Result<(), (GraphError, u32)> {
        let (latency, latency_in) = (self.latency, self.latency_in);
        if latency > latency_in {
            self.raw.push(m - latency_in as i64, raw);
        }
        for item in &mut self.items {
            let from = item.from(cap, latency_in);
            if m < from || m >= item.until() {
                continue;
            }
            if m == from {
                item.chain.reset();
                item.lag.clear();
            }
            item.chain.run(m, raw, outputs, item.base, &mut item.out)?;
            if item.chain.latency < latency {
                item.lag.push(m - item.chain.latency as i64, &item.out);
            }
        }
        let f = m - latency as i64;
        let supplier = self
            .items
            .iter()
            .rposition(|it| it.first <= f && f < it.end);
        for (i, item) in self.items.iter_mut().enumerate() {
            if Some(i) != supplier {
                item.chain.fx.iter_mut().for_each(|f| f.active = false);
            }
        }
        let (source, from) = match supplier {
            Some(i) => {
                let item = &self.items[i];
                let source = if item.chain.latency < latency {
                    item.lag.get(f)
                } else {
                    Some(&item.out)
                };
                (source, item.chain.audio_layout)
            }
            None if latency > latency_in => (self.raw.get(f), self.raw_layout),
            None => (Some(raw), self.raw_layout),
        };
        match source {
            Some(s) if from != self.layout && !s.audio.is_empty() => {
                assign(&mut self.out.video, &s.video);
                self.out.picture = s.picture;
                self.out.audio.clear();
                self.out.audio.resize(self.layout.len(), 0.0);
                mix_into(&s.audio, from, &mut self.out.audio, self.layout, 1.0);
                self.out.assign_named(&s.named);
            }
            Some(s) => self.out.assign(s),
            None => self.out.clear(),
        }
        // An item's graph shows its picture wherever the item plays; where none plays, its
        // named outputs read zeros (what is below has none of its own).
        if supplier.is_some() {
            self.out.picture = !self.out.video.is_empty();
            self.out.named.resize_with(self.slots, Vec::new);
        } else {
            self.out.named.clear();
            self.out.named.resize_with(self.slots, Vec::new);
        }
        Ok(())
    }
}

/// What reads a track's media for a source frame.
pub(crate) trait MediaReader {
    /// Fills `out` with track `track`'s picture or sound at source frame `s` (nothing before
    /// the start).
    fn read(&mut self, track: usize, s: i64, out: &mut Stream) -> Result<(), crate::MediaError>;
}

/// One node of the composition: a track's stem, the mix, or a graph track.
enum NodeRt {
    /// A video or audio track as it plays: its stem.
    Track {
        /// Its media among the readers, if it plays any and something reads it.
        media: Option<usize>,
    },
    /// The video and audio mix of the tracks.
    Mix {
        /// The tracks mixed, top first: each with its sound's gain, or `None` when its sound
        /// goes to another bus.
        inputs: Vec<(usize, Option<f32>)>,
        audio_layout: Layout,
    },
    /// A graph track: where one of its items plays, the item's output; elsewhere what is
    /// below it, the node `host`.
    Graphs {
        host: usize,
        /// The latency of what comes from `host`.
        host_latency: usize,
        items: ItemsRt,
    },
}

impl NodeRt {
    fn chains_mut(&mut self) -> Vec<&mut ChainRt> {
        match self {
            Self::Graphs { items, .. } => items.items.iter_mut().map(|i| &mut i.chain).collect(),
            _ => Vec::new(),
        }
    }

    /// Every FX of the node: its items' graphs.
    fn fx(&self) -> Box<dyn Iterator<Item = &FxRt> + '_> {
        match self {
            Self::Graphs { items, .. } => {
                Box::new(items.items.iter().flat_map(|i| i.chain.fx.iter()))
            }
            _ => Box::new(std::iter::empty()),
        }
    }
}

/// What a node put out, kept as long as anything reads it.
struct NodeOut {
    ring: Ring,
    latency: usize,
    audio_layout: Layout,
    /// The last step's output.
    out: Stream,
}

/// Every node's output but the one running.
struct OutputsView<'a> {
    before: &'a [NodeOut],
    after: &'a [NodeOut],
    running: usize,
}

impl OutputsView<'_> {
    fn get(&self, node: usize) -> Option<&NodeOut> {
        match node.cmp(&self.running) {
            std::cmp::Ordering::Less => self.before.get(node),
            std::cmp::Ordering::Greater => self.after.get(node - self.running - 1),
            std::cmp::Ordering::Equal => None,
        }
    }
}

/// A track's media as the composition reads it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MediaInfo {
    /// Its reader.
    pub reader: usize,
    /// Its sound's layout, for an audio track.
    pub audio: Option<Layout>,
}

/// What a composition is built from.
pub(crate) struct ComposePlan<'a> {
    pub composition: &'a Composition,
    /// The open graph's description, which the composition leaves out.
    pub open: &'a GraphDesc,
    /// Each composition track's media, if it plays any that could be opened.
    pub media: Vec<Option<MediaInfo>>,
    pub registry: &'a Registry,
    /// What every graph is compiled against, but for the sources.
    pub options: &'a CompileOptions,
    pub bus: &'a crate::timeline::Bus,
    pub fps: f64,
    /// A render of one graph, whose compile errors are the graph's rather than an item's.
    pub single: bool,
}

/// The compiled composition: the nodes run in index order (the tracks, the mix, then the graph
/// tracks from the bottom up), so everything a node reads has run before it.
pub(crate) struct Compose {
    nodes: Vec<NodeRt>,
    outputs: Vec<NodeOut>,
    /// The node whose output is the master: the top graph track, or the mix.
    master: usize,
    open: u32,
    single: bool,
    /// The master's latency: the composition's.
    latency: usize,
    warmup: usize,
    renders_audio: bool,
}

impl Compose {
    pub fn build(plan: ComposePlan) -> Result<Self, EngineError> {
        Builder::new(&plan).build()
    }

    pub fn latency(&self) -> usize {
        self.latency
    }

    /// Source steps to pre-render before a seek: the graph tracks' warmups added up from the
    /// bottom, limited to `cap`.
    pub fn warmup(&self, cap: usize) -> usize {
        self.warmup.min(cap)
    }

    /// Whether the master's sound isn't the plain audio mix: some graph item renders sound of
    /// its own, or none where one plays.
    pub fn renders_audio(&self) -> bool {
        self.renders_audio
    }

    /// The layout of the master's sound.
    pub fn audio_layout(&self) -> Layout {
        self.outputs[self.master].audio_layout
    }

    /// The master's picture and sound from the last step.
    pub fn output(&self) -> &Stream {
        &self.outputs[self.master].out
    }

    /// Clears all state, as if no frame had been processed.
    pub fn reset(&mut self) {
        for node in &mut self.nodes {
            if let NodeRt::Graphs { items, .. } = node {
                items.raw.clear();
                for item in &mut items.items {
                    item.lag.clear();
                }
            }
            for chain in node.chains_mut() {
                chain.reset();
            }
        }
        for out in &mut self.outputs {
            out.ring.clear();
            out.out.clear();
        }
    }

    /// Processes source step `m`. `cap` limits item pre-rolls.
    pub fn step(
        &mut self,
        m: usize,
        media: &mut dyn MediaReader,
        cap: usize,
    ) -> Result<(), EngineError> {
        let m = m as i64;
        for node in &mut self.nodes {
            for chain in node.chains_mut() {
                chain.fx.iter_mut().for_each(|f| f.active = false);
            }
        }
        let single = self.single;
        let empty = Stream::default();
        for n in 0..self.nodes.len() {
            let (before, rest) = self.outputs.split_at_mut(n);
            let (this, after) = rest.split_first_mut().expect("node in range");
            let view = OutputsView {
                before,
                after,
                running: n,
            };
            match &mut self.nodes[n] {
                NodeRt::Track { media: reader } => match reader {
                    Some(r) => media.read(*r, m, &mut this.out)?,
                    None => this.out.clear(),
                },
                NodeRt::Mix {
                    inputs,
                    audio_layout,
                } => {
                    let out = &mut this.out;
                    out.clear();
                    out.audio.resize(audio_layout.len(), 0.0);
                    for &(input, gain) in inputs.iter() {
                        let Some(child) = view.get(input) else {
                            continue;
                        };
                        let Some(s) = child.ring.get(m) else {
                            continue;
                        };
                        if !out.picture && s.picture && !s.video.is_empty() {
                            assign(&mut out.video, &s.video);
                            out.picture = true;
                        }
                        if let Some(gain) = gain {
                            mix_into(
                                &s.audio,
                                child.audio_layout,
                                &mut out.audio,
                                *audio_layout,
                                gain,
                            );
                        }
                    }
                }
                NodeRt::Graphs {
                    host,
                    host_latency,
                    items,
                } => {
                    let below = view
                        .get(*host)
                        .and_then(|o| o.ring.get(m - *host_latency as i64))
                        .unwrap_or(&empty);
                    items
                        .run(m, below, &view, cap)
                        .map_err(|(e, id)| fx_error(e, id, single))?;
                    this.out.assign(&items.out);
                }
            }
            let frame = m - this.latency as i64;
            this.ring.push(frame, &this.out);
        }
        Ok(())
    }

    /// Every graph that ran in the last step, the top graph track's first.
    fn active(&self) -> impl Iterator<Item = &FxRt> {
        self.all_fx().filter(|f| f.active)
    }

    fn all_fx(&self) -> impl Iterator<Item = &FxRt> {
        self.nodes.iter().rev().flat_map(NodeRt::fx)
    }

    /// The graph editors inspect: the first active one playing the open graph, else the first
    /// active one.
    fn inspected(&self) -> Option<&FxRt> {
        self.active()
            .find(|f| f.graph_id == self.open)
            .or_else(|| self.active().next())
    }

    pub fn tap(&self, node: &str, output: usize) -> Option<&Signal> {
        self.active()
            .filter(|f| f.graph_id == self.open)
            .chain(self.active())
            .find_map(|f| f.graph.tap(node, output))
    }

    pub fn levels(&self) -> Vec<OutputLevel> {
        self.inspected()
            .map(|f| f.graph.levels())
            .unwrap_or_default()
    }

    pub fn costs(&self) -> Vec<NodeCost> {
        self.inspected()
            .map(|f| f.graph.costs())
            .unwrap_or_default()
    }

    pub fn meters(&self) -> Vec<NodeMeters> {
        self.inspected()
            .map(|f| f.graph.meters())
            .unwrap_or_default()
    }

    pub fn param_levels(&self) -> Vec<ParamLevel> {
        self.inspected()
            .map(|f| f.graph.param_levels())
            .unwrap_or_default()
    }

    /// The first item playing the open graph, else the first item.
    fn stats_fx(&self) -> Option<&FxRt> {
        self.all_fx()
            .find(|f| f.graph_id == self.open)
            .or_else(|| self.all_fx().next())
    }

    /// Each node's own latency and warmup, for the open graph's first item.
    pub fn node_stats(&self) -> &[NodeStats] {
        self.stats_fx().map_or(&[], |f| f.graph.node_stats())
    }

    /// What the open graph's first item was compiled against.
    pub fn compile_options(&self) -> Option<&CompileOptions> {
        self.all_fx()
            .find(|f| f.graph_id == self.open)
            .map(|f| &f.options)
    }
}

fn fx_error(error: GraphError, graph: u32, single: bool) -> EngineError {
    if single {
        EngineError::Graph(error)
    } else {
        EngineError::Fx { graph, error }
    }
}

/// The frame a time in seconds falls in, at `fps`: the first frame that starts at or after it.
/// An infinite time is the last frame there is.
fn frame_of(t: f64, fps: f64) -> i64 {
    (t * fps - 1e-6).ceil() as i64
}

/// What is added to an output frame to get `item`'s graph's own frame: its graph counts from the
/// item's trimmed start, not from the start of the project.
fn item_base(item: &GraphItem, fps: f64) -> i64 {
    (item.start * fps).round() as i64 - frame_of(item.position, fps)
}

/// Compiles a composition.
struct Builder<'a> {
    plan: &'a ComposePlan<'a>,
    graphs: HashMap<u32, &'a GraphDesc>,
    video_layout: Layout,
    mix: Layout,
    /// The mix's node; the tracks' are before it, the graph tracks' after it.
    mix_node: usize,
    /// Compiled outputs' latency and sound layout, by node, as they are compiled.
    compiled: Vec<Option<(usize, Layout)>>,
    /// Who reads each node, and at what latency.
    reads: Vec<Vec<usize>>,
    /// Whether each node's sound is the plain audio mix, as far as is known.
    mix_sound: Vec<bool>,
    /// Each graph track node's named output slots: the names its items' graphs have, each
    /// with the layout of the first item that has it.
    named: Vec<Vec<(String, Layout)>>,
}

impl<'a> Builder<'a> {
    fn new(plan: &'a ComposePlan<'a>) -> Self {
        let comp = plan.composition;
        let nodes = comp.tracks.len() + 1 + comp.graph_tracks.len();
        let mut graphs: HashMap<u32, &GraphDesc> =
            comp.graphs.iter().map(|(id, g)| (*id, g)).collect();
        graphs.insert(comp.open, plan.open);
        Self {
            plan,
            graphs,
            video_layout: plan.options.output,
            mix: mix_layout(plan.bus.channels, plan.fps),
            mix_node: comp.tracks.len(),
            compiled: vec![None; nodes],
            reads: vec![Vec::new(); nodes],
            mix_sound: vec![false; nodes],
            named: vec![Vec::new(); nodes],
        }
    }

    fn comp(&self) -> &'a Composition {
        self.plan.composition
    }

    /// The node of the graph track at `index` (top first) in the composition.
    fn graph_node(&self, index: usize) -> usize {
        self.mix_node + self.comp().graph_tracks.len() - index
    }

    /// The items of graph track `index` that play: with a graph the project has, and a length.
    fn playing(&self, index: usize) -> Vec<&'a GraphItem> {
        self.comp().graph_tracks[index]
            .items
            .iter()
            .filter(|i| !i.muted && i.length > 0.0 && self.graphs.contains_key(&i.graph))
            .collect()
    }

    fn build(mut self) -> Result<Compose, EngineError> {
        let comp = self.comp();
        let mix_node = self.mix_node;
        // A track is read when it is in the mix or a graph that plays reads its stem.
        let stems: Vec<String> = (0..comp.graph_tracks.len())
            .flat_map(|g| self.playing(g))
            .flat_map(|i| input_ports(self.graphs[&i.graph]))
            .filter_map(|p| match p.source {
                Source::Track(name) => Some(name),
                _ => None,
            })
            .collect();
        let read: Vec<bool> = comp
            .tracks
            .iter()
            .map(|t| t.in_mix || stems.contains(&t.name))
            .collect();
        let mut nodes = Vec::new();
        let mut warmups = vec![0usize; self.compiled.len()];
        for (i, &read) in read.iter().enumerate() {
            let media = self.plan.media[i].filter(|_| read);
            let layout = media.and_then(|m| m.audio).unwrap_or(self.mix);
            self.compiled[i] = Some((0, layout));
            nodes.push(NodeRt::Track {
                media: media.map(|m| m.reader),
            });
        }
        // The mix: video tracks top first, the top one with a picture winning, and the audio
        // tracks on the rendered bus summed at their volumes.
        let inputs: Vec<(usize, Option<f32>)> = comp
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.in_mix)
            .map(|(i, t)| {
                let gain = match t.kind {
                    TrackKind::Video => Some(1.0),
                    TrackKind::Audio => (t.bus == self.plan.bus.name).then_some(t.volume),
                };
                (i, gain)
            })
            .collect();
        for &(i, _) in &inputs {
            self.reads[i].push(0);
        }
        self.compiled[mix_node] = Some((0, self.mix));
        self.mix_sound[mix_node] = true;
        nodes.push(NodeRt::Mix {
            inputs,
            audio_layout: self.mix,
        });
        // The graph tracks, bottom first, each on the one below.
        for index in (0..comp.graph_tracks.len()).rev() {
            let node = self.graph_node(index);
            let host = node - 1;
            let (host_latency, _) = self.compiled[host].expect("below is compiled first");
            self.reads[host].push(host_latency);
            let fps = self.plan.fps;
            let mut items = Vec::new();
            let mut mix_sound = self.mix_sound[host];
            for item in self.playing(index) {
                let (first, end) = (frame_of(item.position, fps), frame_of(item.end(), fps));
                if end <= first {
                    continue;
                }
                let (chain, passes_mix) = self.build_item(node, item, host_latency)?;
                mix_sound &= passes_mix;
                items.push(ItemFxRt {
                    base: item_base(item, fps),
                    first,
                    end,
                    pre_roll: item.pre_roll,
                    lag: Ring::default(),
                    out: Stream::default(),
                    chain,
                });
            }
            // The track's named outputs: each name once, in the layout of the first item that
            // has it; an item whose output of that name has another layout fills zeros.
            let mut slots: Vec<(String, Layout)> = Vec::new();
            for item in &items {
                for fx in &item.chain.fx {
                    for (name, layout) in fx.graph.named_layouts() {
                        if !slots.iter().any(|(n, _)| n == name) {
                            slots.push((name.to_owned(), layout));
                        }
                    }
                }
            }
            for item in &mut items {
                for fx in &mut item.chain.fx {
                    fx.named = slots
                        .iter()
                        .map(|(name, layout)| {
                            let fills = fx
                                .graph
                                .named_layouts()
                                .any(|(n, l)| n == name && l == *layout);
                            (name.clone(), fills)
                        })
                        .collect();
                }
            }
            let slot_count = slots.len();
            self.named[node] = slots;
            let latency = items
                .iter()
                .map(|i| i.chain.latency)
                .fold(host_latency, usize::max);
            for item in &mut items {
                let lag = latency - item.chain.latency;
                item.lag = Ring::new(if lag > 0 { lag + 1 } else { 0 });
            }
            let item_warmup = items.iter().map(|i| i.chain.warmup()).max().unwrap_or(0);
            warmups[node] = warmups[host] + item_warmup;
            let lead = latency - host_latency;
            self.compiled[node] = Some((latency, self.mix));
            self.mix_sound[node] = mix_sound;
            nodes.push(NodeRt::Graphs {
                host,
                host_latency,
                items: ItemsRt {
                    latency,
                    latency_in: host_latency,
                    raw: Ring::new(if lead > 0 { lead + 1 } else { 0 }),
                    raw_layout: self.mix,
                    layout: self.mix,
                    slots: slot_count,
                    out: Stream::default(),
                    items,
                },
            });
        }

        // Each output is kept as long as its slowest reader needs it.
        let mut outputs = Vec::with_capacity(nodes.len());
        for n in 0..nodes.len() {
            let (latency, audio_layout) = self.compiled[n].unwrap_or((0, self.mix));
            let depth = self.reads[n]
                .iter()
                .map(|&r| r.saturating_sub(latency) + 1)
                .max()
                .unwrap_or(0);
            outputs.push(NodeOut {
                ring: Ring::new(depth),
                latency,
                audio_layout,
                out: Stream::default(),
            });
        }
        let master = nodes.len() - 1;
        Ok(Compose {
            latency: outputs[master].latency,
            nodes,
            outputs,
            master,
            open: comp.open,
            single: self.plan.single,
            warmup: warmups[master],
            renders_audio: !self.mix_sound[master],
        })
    }

    /// Compiles `item`'s graph for graph-track node `node`, whose input (what is below) comes at
    /// `latency_in`. Also says whether the item's sound is the plain audio mix: its Audio Output
    /// wired straight from a port that reads the mix.
    fn build_item(
        &mut self,
        node: usize,
        item: &GraphItem,
        latency_in: usize,
    ) -> Result<(ChainRt, bool), EngineError> {
        let desc = self.graphs[&item.graph];
        let ports = input_ports(desc);
        let fills: Vec<Fill> = ports
            .iter()
            .map(|p| self.fill(node, &p.source, p.kind))
            .collect();
        let align = fills
            .iter()
            .filter_map(|f| match f {
                Fill::Receive { node, .. } | Fill::Named { node, .. } => {
                    Some(self.compiled[*node].expect("compiled").0)
                }
                _ => None,
            })
            .fold(latency_in, usize::max);
        let mut port_rts = Vec::new();
        let mut sources = HashMap::new();
        for (port, fill) in ports.iter().zip(&fills) {
            if let Fill::Receive { node, .. } | Fill::Named { node, .. } = *fill {
                self.reads[node].push(align);
            }
            let layout = match *fill {
                Fill::Named { node, slot } => self.named[node][slot].1,
                _ if port.kind == InputKind::Video => self.video_layout,
                Fill::Receive { node, .. } => self.compiled[node].expect("compiled").1,
                _ => self.mix,
            };
            sources.insert(port.key.clone(), layout);
            port_rts.push(Port {
                key: port.key.clone(),
                signal: Signal::zeros(layout),
                fill: *fill,
            });
        }
        // Generators take their layout from the main ports, which are there even when the graph
        // doesn't read them.
        for (name, layout, fill) in [
            (DEFAULT_VIDEO, self.video_layout, Fill::HostVideo),
            (DEFAULT_AUDIO, self.mix, Fill::HostAudio),
        ] {
            if !sources.contains_key(name) {
                sources.insert(name.to_owned(), layout);
                port_rts.push(Port {
                    key: name.to_owned(),
                    signal: Signal::zeros(layout),
                    fill,
                });
            }
        }
        let renders_audio = desc
            .nodes
            .iter()
            .any(|n| n.kind == AUDIO_OUTPUT && audio_output_bus(n) == self.plan.bus.name);
        // The project's beat grid, in the graph's own time: its first beat moves by as much as
        // the graph's frames are shifted, so beats stay on the metronome wherever the item is.
        let fps = self.plan.fps;
        let mut tempo = self.plan.options.tempo;
        tempo.offset_secs += item_base(item, fps) as f64 / fps;
        // What the item plays of the graph's own time, in whole frames as it is rendered; an
        // item without end (a render of one graph) plays the project.
        let span = if item.length.is_finite() {
            let first = frame_of(item.position, fps);
            Span {
                start: (first + item_base(item, fps)) as f64 / fps,
                length: (frame_of(item.end(), fps) - first) as f64 / fps,
            }
        } else {
            self.plan.options.span
        };
        let options = CompileOptions {
            sources,
            tempo,
            span,
            ..self.plan.options.clone()
        };
        let graph = Graph::compile(desc, self.plan.registry, &options)
            .map_err(|e| fx_error(e, item.graph, self.plan.single))?;
        // The item's sound is the mix when its Audio Output is wired straight from a port that
        // reads the mix: what is below, when that is the mix, or the mix itself. A render of one
        // graph keeps the old rule that a graph without an Audio Output leaves the sound alone.
        let passes_mix = (self.plan.single && !renders_audio)
            || renders_audio
                && graph.audio_passthrough().is_some_and(|key| {
                    port_rts.iter().any(|p| {
                        p.key == key
                            && match p.fill {
                                Fill::HostAudio => self.mix_sound[node - 1],
                                Fill::Receive { node, video } => !video && self.mix_sound[node],
                                _ => false,
                            }
                    })
                });
        let own = graph.latency_frames() as usize;
        let host_delay = align - latency_in;
        let audio_layout = graph.audio_layout().unwrap_or(self.mix);
        let fx = FxRt {
            graph_id: item.graph,
            options,
            latency: own,
            warmup: graph.warmup_frames() as usize,
            align,
            host_delay,
            host_ring: Ring::new(if host_delay > 0 { host_delay + 1 } else { 0 }),
            ports: port_rts,
            named: Vec::new(),
            flags: VecDeque::new(),
            out: Stream::default(),
            active: false,
            graph,
        };
        Ok((
            ChainRt {
                fx: vec![fx],
                latency_in,
                latency: align + own,
                audio_layout,
            },
            passes_mix,
        ))
    }

    /// Where an input node reading `source` as `kind`, in an item on graph-track node `node`,
    /// gets its signal.
    fn fill(&self, node: usize, source: &Source, kind: InputKind) -> Fill {
        let video = kind == InputKind::Video;
        let comp = self.comp();
        match source {
            Source::Below if video => Fill::HostVideo,
            Source::Below => Fill::HostAudio,
            Source::Mix => Fill::Receive {
                node: self.mix_node,
                video,
            },
            Source::Track(track) => comp
                .tracks
                .iter()
                .position(|t| t.name == *track)
                .map_or(Fill::Zeros, |node| Fill::Receive { node, video }),
            Source::Graph { track, port } => {
                let Some(below) = comp
                    .graph_tracks
                    .iter()
                    .position(|t| t.name == *track)
                    .map(|index| self.graph_node(index))
                    .filter(|&below| below < node)
                else {
                    return Fill::Zeros;
                };
                let main = if video { DEFAULT_VIDEO } else { DEFAULT_AUDIO };
                if port == main {
                    return Fill::Receive { node: below, video };
                }
                self.named[below]
                    .iter()
                    .position(|(name, _)| name == port)
                    .map_or(Fill::Zeros, |slot| Fill::Named { node: below, slot })
            }
            Source::Nothing => Fill::Zeros,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_finds_recent_frames_and_forgets_old_ones() {
        let mut ring = Ring::new(3);
        for f in 0..5 {
            ring.push(
                f,
                &Stream {
                    video: vec![f as f32],
                    ..Stream::default()
                },
            );
        }
        assert!(ring.get(1).is_none());
        assert_eq!(ring.get(2).unwrap().video, [2.0]);
        assert_eq!(ring.get(4).unwrap().video, [4.0]);
        ring.clear();
        assert!(ring.get(4).is_none());
    }

    #[test]
    fn sound_is_fitted_to_the_mix() {
        let mono = Layout::audio_channels(2, 1);
        let stereo = Layout::audio_channels(4, 2);
        let mut mix = vec![0.0; 8];
        // Mono goes to both channels, stretched across the block.
        mix_into(&[0.0, 1.0], mono, &mut mix, stereo, 0.5);
        assert_eq!(mix[0], 0.0);
        assert_eq!(mix[6], 0.5);
        assert_eq!(mix[6], mix[7]);
        // Channels the mix doesn't have are dropped.
        let mut mono_mix = vec![0.0; 2];
        mix_into(
            &[1.0, 9.0, 1.0, 9.0],
            Layout::audio_channels(2, 2),
            &mut mono_mix,
            mono,
            1.0,
        );
        assert_eq!(mono_mix, [1.0, 1.0]);
    }
}
