//! The routing renderer's runtime: the track tree walked once per source frame.
//!
//! Each track plays its items, then the items' FX, then its own FX chain; each folder mixes the
//! tracks in it (the top one with a picture wins, the sound is summed at each track's volume)
//! and runs its FX chain; the master does the same with the top-level tracks. See
//! [Routing](../../../docs/engine.md#routing).
//!
//! Every stage has a latency: the frames between a source frame going in and its result coming
//! out. A stage's output for output frame `f` comes out at source step `f + latency`, and where
//! signals of different latencies meet (a folder's tracks, an FX's receives) the earlier ones
//! wait in rings so everything lines up.

use std::collections::{HashMap, VecDeque};

use rastersong_graph::nodes::{
    AUDIO_INPUT, AUDIO_OUTPUT, BUS_PARAM, DEFAULT_AUDIO, DEFAULT_VIDEO, OUTPUT, PORT_PARAM,
    VIDEO_INPUT,
};
use rastersong_graph::{
    CompileOptions, Connection, Graph, GraphDesc, GraphError, Layout, NodeCost, NodeDesc,
    NodeMeters, NodeStats, OutputLevel, ParamLevel, ParamValue, Registry, Signal, Sources,
    audio_output_bus,
};
use rastersong_lang::tr_args;

use crate::EngineError;
use crate::routing::{InputKind, Routing, is_main_port, port_of};
use crate::timeline::{Fx, Item, TrackKind};

/// The sound the folders and the master mix in: this many samples a second, in the bus's
/// channels, one block per frame. What leaves the render is resampled to the project's audio
/// rate.
pub const MIX_RATE: f64 = 48_000.0;

/// The layout folders and the master mix sound in, for `channels` at `fps`.
pub fn mix_layout(channels: u32, fps: f64) -> Layout {
    Layout::audio_channels((MIX_RATE / fps).round().max(1.0) as u32, channels.max(1))
}

/// Ids of the nodes added to an FX graph that has no output of its own for a stream, so the
/// stream passes through it at the graph's latency.
const THROUGH_VIDEO_IN: &str = "@through_video_in";
const THROUGH_VIDEO_OUT: &str = "@through_video_out";
const THROUGH_AUDIO_IN: &str = "@through_audio_in";
const THROUGH_AUDIO_OUT: &str = "@through_audio_out";

/// What flows along the tree for one frame.
#[derive(Debug, Clone, Default)]
pub(crate) struct Stream {
    /// The picture, in the render's layout; empty for black.
    pub video: Vec<f32>,
    /// Whether there is a picture: a folder shows the top one of its tracks that has one.
    pub picture: bool,
    /// The sound, in the stage's audio layout; empty for silence.
    pub audio: Vec<f32>,
}

impl Stream {
    pub fn clear(&mut self) {
        self.video.clear();
        self.audio.clear();
        self.picture = false;
    }

    fn assign(&mut self, other: &Self) {
        assign(&mut self.video, &other.video);
        assign(&mut self.audio, &other.audio);
        self.picture = other.picture;
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

/// Where an FX input port's signal comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Fill {
    HostVideo,
    HostAudio,
    /// The output of route node `node`: its picture, or its sound.
    Receive {
        node: usize,
        video: bool,
    },
    /// Nothing fills the port: zeros.
    Zeros,
}

/// One source signal an FX graph reads, by the name it was compiled with.
#[derive(Debug)]
struct Port {
    key: String,
    signal: Signal,
    fill: Fill,
}

/// The source signals of one FX graph.
struct PortSources<'a>(&'a [Port]);

impl Sources for PortSources<'_> {
    fn get(&self, name: &str) -> Option<&Signal> {
        self.0.iter().find(|p| p.key == name).map(|p| &p.signal)
    }
}

/// One FX of a chain, compiled.
struct FxRt {
    graph: Graph,
    graph_id: u32,
    /// What it was compiled against, for editors inspecting the graph the same way.
    options: CompileOptions,
    latency: usize,
    warmup: usize,
    /// Whether the graph has its own output for the picture; without one the host's passes
    /// through.
    renders_video: bool,
    /// The latency of its inputs: the latest of the chain before it and its receives.
    align: usize,
    /// Frames the chain before it is held back to line up with its receives.
    host_delay: usize,
    host_ring: Ring,
    ports: Vec<Port>,
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

/// An FX chain, compiled.
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

    /// Whether any of its FX has its own output for the picture.
    fn renders_video(&self) -> bool {
        self.fx.iter().any(|f| f.renders_video)
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
            graph
                .process((f_in + base).max(0) as u64, &PortSources(ports))
                .map_err(|e| (e, *graph_id))?;
            assign(&mut fx_out.video, &graph.output().data);
            match graph.audio_output() {
                Some(audio) => assign(&mut fx_out.audio, &audio.data),
                None => fx_out.audio.clear(),
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

/// An item with FX, compiled.
struct ItemFxRt {
    chain: ChainRt,
    /// The output frames it plays: `first..end`.
    first: i64,
    end: i64,
    /// Added to a source frame to get its graphs' own frame.
    base: i64,
    pre_roll: bool,
    /// Frames its output is held back to line up with the track's slowest item.
    lag: Ring,
    out: Stream,
}

impl ItemFxRt {
    /// Frames pre-rendered before the item, limited to `cap`.
    fn warm(&self, cap: usize) -> usize {
        self.chain.warmup().min(cap)
    }

    /// The first source step it processes: its pre-roll before the left edge, or the edge.
    fn from(&self, cap: usize) -> i64 {
        let pre = if self.pre_roll { self.warm(cap) } else { 0 };
        self.first.saturating_sub(pre as i64).max(0)
    }

    /// One past the last source step it processes.
    fn until(&self) -> i64 {
        self.end.saturating_add(self.chain.latency as i64)
    }
}

/// A track's items with FX: where one plays, its FX's output replaces the track's.
struct ItemsRt {
    items: Vec<ItemFxRt>,
    /// The slowest item's latency: the stage's.
    latency: usize,
    /// The track as it plays, held back by the latency.
    raw: Ring,
    /// The layouts of the track's sound as it plays and of the stage's: every item's sound is
    /// fitted to the stage's.
    raw_layout: Layout,
    layout: Layout,
    out: Stream,
}

impl ItemsRt {
    /// Runs the items playing at source step `m` on the track's `raw` stream (output frame `m`),
    /// and puts the stage's output (frame `m - latency`) in `self.out`.
    fn run(
        &mut self,
        m: i64,
        raw: &Stream,
        outputs: &OutputsView,
        cap: usize,
    ) -> Result<(), (GraphError, u32)> {
        let latency = self.latency;
        if latency > 0 {
            self.raw.push(m, raw);
        }
        for item in &mut self.items {
            let from = item.from(cap);
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
            None if latency > 0 => (self.raw.get(f), self.raw_layout),
            None => (Some(raw), self.raw_layout),
        };
        match source {
            Some(s) if from != self.layout && !s.audio.is_empty() => {
                assign(&mut self.out.video, &s.video);
                self.out.picture = s.picture;
                self.out.audio.clear();
                self.out.audio.resize(self.layout.len(), 0.0);
                mix_into(&s.audio, from, &mut self.out.audio, self.layout, 1.0);
            }
            Some(s) => self.out.assign(s),
            None => self.out.clear(),
        }
        // An item's FX show their picture wherever the item plays.
        if supplier.is_some_and(|i| self.items[i].chain.renders_video()) {
            self.out.picture = !self.out.video.is_empty();
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

/// One node of the tree: a track, a folder, or the master.
enum NodeRt {
    Track {
        /// Its media among the readers, if it plays any.
        media: Option<usize>,
        raw: Stream,
        items: Option<ItemsRt>,
        chain: ChainRt,
    },
    Mix {
        /// The nodes mixed, top first: each with its sound's gain, or `None` when its sound
        /// goes elsewhere (another bus).
        inputs: Vec<(usize, Option<f32>)>,
        audio_layout: Layout,
        host: Stream,
        chain: ChainRt,
    },
}

impl NodeRt {
    fn chains_mut(&mut self) -> Vec<&mut ChainRt> {
        match self {
            Self::Track { items, chain, .. } => std::iter::once(chain)
                .chain(
                    items
                        .iter_mut()
                        .flat_map(|i| i.items.iter_mut().map(|i| &mut i.chain)),
                )
                .collect(),
            Self::Mix { chain, .. } => vec![chain],
        }
    }

    /// Every FX of the node: its own chain's, then its items'.
    fn fx(&self) -> Box<dyn Iterator<Item = &FxRt> + '_> {
        match self {
            Self::Track { items, chain, .. } => Box::new(
                chain.fx.iter().chain(
                    items
                        .iter()
                        .flat_map(|i| i.items.iter().flat_map(|i| i.chain.fx.iter())),
                ),
            ),
            Self::Mix { chain, .. } => Box::new(chain.fx.iter()),
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

/// A track's media as the route reads it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MediaInfo {
    /// Its reader.
    pub reader: usize,
    /// Its sound's layout, for an audio track.
    pub audio: Option<Layout>,
    /// The resource's length in seconds.
    pub duration: f64,
}

/// What a route is built from.
pub(crate) struct RoutePlan<'a> {
    pub routing: &'a Routing,
    /// The open graph's description, which the routing leaves out.
    pub open: &'a GraphDesc,
    /// Each routing track's media, if it plays any that could be opened.
    pub media: Vec<Option<MediaInfo>>,
    /// Each routing track's items.
    pub items: Vec<Vec<Item>>,
    pub registry: &'a Registry,
    /// What every graph is compiled against, but for the sources.
    pub options: &'a CompileOptions,
    pub bus: &'a crate::timeline::Bus,
    pub fps: f64,
    /// A render of one graph, whose compile errors are the graph's rather than an FX's.
    pub single: bool,
}

/// The compiled routing.
pub(crate) struct Route {
    nodes: Vec<NodeRt>,
    outputs: Vec<NodeOut>,
    /// The nodes that run, in order; the master last.
    order: Vec<usize>,
    master: usize,
    open: u32,
    single: bool,
    /// The master's latency: the route's.
    latency: usize,
    warmup: usize,
    renders_audio: bool,
    passthrough: Option<String>,
}

impl Route {
    pub fn build(plan: RoutePlan) -> Result<Self, EngineError> {
        Builder::new(&plan).build()
    }

    pub fn latency(&self) -> usize {
        self.latency
    }

    /// Source steps to pre-render before a seek: the longest run of FX warmups to the master,
    /// limited to `cap`.
    pub fn warmup(&self, cap: usize) -> usize {
        self.warmup.min(cap)
    }

    /// Whether any FX renders sound of its own, so the render's sound isn't the plain mix.
    pub fn renders_audio(&self) -> bool {
        self.renders_audio
    }

    /// The track the render's sound is, as it is, when the only FX rendering sound wires a
    /// receive straight to its Audio Output.
    pub fn passthrough(&self) -> Option<&str> {
        self.passthrough.as_deref()
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
            if let NodeRt::Track {
                items: Some(items), ..
            } = node
            {
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
        for &n in &self.order {
            let (before, rest) = self.outputs.split_at_mut(n);
            let (this, after) = rest.split_first_mut().expect("node in range");
            let view = OutputsView {
                before,
                after,
                running: n,
            };
            match &mut self.nodes[n] {
                NodeRt::Track {
                    media: reader,
                    raw,
                    items,
                    chain,
                } => {
                    match reader {
                        Some(r) => media.read(*r, m, raw)?,
                        None => raw.clear(),
                    }
                    let host = match items {
                        Some(items) => {
                            items
                                .run(m, raw, &view, cap)
                                .map_err(|(e, id)| fx_error(e, id, false))?;
                            &items.out
                        }
                        None => &*raw,
                    };
                    chain
                        .run(m, host, &view, 0, &mut this.out)
                        .map_err(|(e, id)| fx_error(e, id, single))?;
                }
                NodeRt::Mix {
                    inputs,
                    audio_layout,
                    host,
                    chain,
                } => {
                    let f_in = m - chain.latency_in as i64;
                    host.clear();
                    host.audio.resize(audio_layout.len(), 0.0);
                    for &(input, gain) in inputs.iter() {
                        let Some(child) = view.get(input) else {
                            continue;
                        };
                        let Some(s) = child.ring.get(f_in) else {
                            continue;
                        };
                        if !host.picture && s.picture && !s.video.is_empty() {
                            assign(&mut host.video, &s.video);
                            host.picture = true;
                        }
                        if let Some(gain) = gain {
                            mix_into(
                                &s.audio,
                                child.audio_layout,
                                &mut host.audio,
                                *audio_layout,
                                gain,
                            );
                        }
                    }
                    chain
                        .run(m, host, &view, 0, &mut this.out)
                        .map_err(|(e, id)| fx_error(e, id, single))?;
                }
            }
            let frame = m - this.latency as i64;
            this.ring.push(frame, &this.out);
        }
        Ok(())
    }

    /// Every FX that ran in the last step, the master's first, then up from the bottom of the
    /// tree.
    fn active(&self) -> impl Iterator<Item = &FxRt> {
        self.all_fx().filter(|f| f.active)
    }

    fn all_fx(&self) -> impl Iterator<Item = &FxRt> {
        self.order
            .iter()
            .rev()
            .flat_map(move |&n| self.nodes[n].fx())
    }

    /// The FX whose graph editors inspect: the first active one playing the open graph, else
    /// the first active one.
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

    /// The first FX playing the open graph, else the first FX.
    fn stats_fx(&self) -> Option<&FxRt> {
        self.all_fx()
            .find(|f| f.graph_id == self.open)
            .or_else(|| self.all_fx().next())
    }

    /// Each node's own latency and warmup, for the open graph's first FX.
    pub fn node_stats(&self) -> &[NodeStats] {
        self.stats_fx().map_or(&[], |f| f.graph.node_stats())
    }

    /// What the open graph's first FX was compiled against.
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

/// Compiles a route.
struct Builder<'a> {
    plan: &'a RoutePlan<'a>,
    graphs: HashMap<u32, &'a GraphDesc>,
    /// Each track's parent node: a folder, or the master.
    parents: Vec<usize>,
    master: usize,
    video_layout: Layout,
    mix: Layout,
    /// Compiled outputs' latency and sound layout, by node, as they are compiled.
    compiled: Vec<Option<(usize, Layout)>>,
    /// Who reads each node, and at what latency.
    reads: Vec<Vec<usize>>,
    renders_audio: Vec<(usize, Option<String>)>,
}

impl<'a> Builder<'a> {
    fn new(plan: &'a RoutePlan<'a>) -> Self {
        let tracks = &plan.routing.tracks;
        let master = tracks.len();
        let parents = (0..tracks.len())
            .map(|i| {
                (0..i)
                    .rev()
                    .find(|&j| tracks[j].depth < tracks[i].depth)
                    .unwrap_or(master)
            })
            .collect();
        let mut graphs: HashMap<u32, &GraphDesc> =
            plan.routing.graphs.iter().map(|(id, g)| (*id, g)).collect();
        graphs.insert(plan.routing.open, plan.open);
        Self {
            plan,
            graphs,
            parents,
            master,
            video_layout: plan.options.output,
            mix: mix_layout(plan.bus.channels, plan.fps),
            compiled: vec![None; master + 1],
            reads: vec![Vec::new(); master + 1],
            renders_audio: Vec::new(),
        }
    }

    fn tracks(&self) -> &'a [crate::routing::RouteTrack] {
        &self.plan.routing.tracks
    }

    /// The FX of node `n` that run: on, and with a graph the project has.
    fn chain_of(&self, n: usize) -> Vec<&'a Fx> {
        let list = if n == self.master {
            &self.plan.routing.master_fx
        } else {
            &self.tracks()[n].fx
        };
        self.running(list)
    }

    fn running(&self, list: &'a [Fx]) -> Vec<&'a Fx> {
        list.iter()
            .filter(|f| !f.bypass && self.graphs.contains_key(&f.graph))
            .collect()
    }

    /// The items of track `n` whose FX run.
    fn items_with_fx(&self, n: usize) -> Vec<&'a Item> {
        if n == self.master {
            return Vec::new();
        }
        self.plan.items[n]
            .iter()
            .filter(|i| !i.muted && !self.running(&i.fx).is_empty())
            .collect()
    }

    /// The tracks node `n`'s FX (and its items' FX) receive from.
    fn receives_of(&self, n: usize) -> Vec<usize> {
        let mut chains: Vec<Vec<&Fx>> = vec![self.chain_of(n)];
        for item in self.items_with_fx(n) {
            chains.push(self.running(&item.fx));
        }
        let mut sources = Vec::new();
        for fx in chains.into_iter().flatten() {
            let desc = self.graphs[&fx.graph];
            for (port, track) in &fx.receives {
                let read = desc
                    .nodes
                    .iter()
                    .filter_map(|n| port_of(&n.kind, &n.params))
                    .any(|(name, _)| name == port);
                if let Some(t) = self.tracks().iter().position(|t| &t.name == track)
                    && read
                    && !sources.contains(&t)
                {
                    sources.push(t);
                }
            }
        }
        sources
    }

    fn build(mut self) -> Result<Route, EngineError> {
        let tracks = self.tracks();
        let master = self.master;
        // What the master needs: tracks that reach it, and what their FX receive from.
        let mut needed = vec![false; master + 1];
        needed[master] = true;
        loop {
            let mut changed = false;
            for i in 0..tracks.len() {
                if !needed[i] && needed[self.parents[i]] && tracks[i].sends() {
                    needed[i] = true;
                    changed = true;
                }
            }
            for n in 0..=master {
                if !needed[n] {
                    continue;
                }
                for t in self.receives_of(n) {
                    if !needed[t] {
                        needed[t] = true;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        // Each node's inputs: the tracks it mixes, and the tracks it receives from.
        let mixed: Vec<Vec<usize>> = (0..=master)
            .map(|n| {
                (0..tracks.len())
                    .filter(|&i| self.parents[i] == n && needed[i] && tracks[i].sends())
                    .collect()
            })
            .collect();
        let mut deps: Vec<Vec<usize>> = vec![Vec::new(); master + 1];
        for n in (0..=master).filter(|&n| needed[n]) {
            let mut d = mixed[n].clone();
            for t in self.receives_of(n) {
                if !d.contains(&t) {
                    d.push(t);
                }
            }
            deps[n] = d;
        }
        let order = topological(&deps, master).map_err(|cycle| {
            let names: Vec<&str> = cycle
                .iter()
                .map(|&n| tracks.get(n).map_or("master", |t| t.name.as_str()))
                .collect();
            EngineError::Routing(tr_args(
                "error.routing.cycle",
                &[("tracks", &names.join(" → "))],
            ))
        })?;

        let mut nodes: Vec<Option<NodeRt>> = (0..=master).map(|_| None).collect();
        let mut warmups = vec![0usize; master + 1];
        for &n in &order {
            let dep_warmup = deps[n].iter().map(|&d| warmups[d]).max().unwrap_or(0);
            let (node, latency, layout, warmup) = if n == master || tracks[n].folder {
                self.build_mix(n, &mixed[n])?
            } else {
                self.build_track(n)?
            };
            warmups[n] = dep_warmup + warmup;
            self.compiled[n] = Some((latency, layout));
            nodes[n] = Some(node);
        }

        // Each output is kept as long as its slowest reader needs it.
        let mut outputs = Vec::with_capacity(master + 1);
        for n in 0..=master {
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
        let nodes: Vec<NodeRt> = nodes
            .into_iter()
            .map(|n| {
                n.unwrap_or(NodeRt::Mix {
                    inputs: Vec::new(),
                    audio_layout: self.mix,
                    host: Stream::default(),
                    chain: ChainRt::default(),
                })
            })
            .collect();
        let passthrough = match self.renders_audio.as_slice() {
            [(node, Some(track))] if *node == master => Some(track.clone()),
            _ => None,
        };
        Ok(Route {
            latency: outputs[master].latency,
            nodes,
            outputs,
            order,
            master,
            open: self.plan.routing.open,
            single: self.plan.single,
            warmup: warmups[master],
            renders_audio: !self.renders_audio.is_empty(),
            passthrough,
        })
    }

    /// A track: its media, its items' FX, its FX chain. Returns the node, its latency, its
    /// sound's layout and its warmup.
    fn build_track(&mut self, n: usize) -> Result<(NodeRt, usize, Layout, usize), EngineError> {
        let media = self.plan.media[n];
        let raw_audio = media.and_then(|m| m.audio).unwrap_or(self.mix);
        let fps = self.plan.fps;
        let frame_of = |t: f64| (t * fps - 1e-6).ceil() as i64;
        let mut items = Vec::new();
        for item in self.items_with_fx(n) {
            let duration = media.map_or(0.0, |m| m.duration);
            let (first, end) = (
                frame_of(item.position),
                frame_of(item.timeline_end(duration)),
            );
            if end <= first {
                continue;
            }
            let chain = self.build_chain(n, &self.running(&item.fx), 0, raw_audio)?;
            items.push(ItemFxRt {
                base: (item.start / item.rate * fps).round() as i64 - first,
                first,
                end,
                pre_roll: item.pre_roll,
                lag: Ring::default(),
                out: Stream::default(),
                chain,
            });
        }
        // Items' FX may change the sound's layout; where several do, the first one's is used
        // and everything else is fitted to it.
        let host_audio = items.first().map_or(raw_audio, |i| i.chain.audio_layout);
        let items = (!items.is_empty()).then(|| {
            let latency = items.iter().map(|i| i.chain.latency).max().unwrap_or(0);
            for item in &mut items {
                let lag = latency - item.chain.latency;
                item.lag = Ring::new(if lag > 0 { lag + 1 } else { 0 });
            }
            ItemsRt {
                latency,
                raw: Ring::new(if latency > 0 { latency + 1 } else { 0 }),
                raw_layout: raw_audio,
                layout: host_audio,
                out: Stream::default(),
                items,
            }
        });
        let items_latency = items.as_ref().map_or(0, |i| i.latency);
        let items_warmup = items.as_ref().map_or(0, |i| {
            i.items.iter().map(|i| i.chain.warmup()).max().unwrap_or(0)
        });
        let chain = self.build_chain(n, &self.chain_of(n), items_latency, host_audio)?;
        let (latency, layout, warmup) = (
            chain.latency,
            chain.audio_layout,
            items_warmup + chain.warmup(),
        );
        Ok((
            NodeRt::Track {
                media: media.map(|m| m.reader),
                raw: Stream::default(),
                items,
                chain,
            },
            latency,
            layout,
            warmup,
        ))
    }

    /// A folder or the master: the tracks it mixes, then its FX chain.
    fn build_mix(
        &mut self,
        n: usize,
        children: &[usize],
    ) -> Result<(NodeRt, usize, Layout, usize), EngineError> {
        let tracks = self.tracks();
        let latency_in = children
            .iter()
            .map(|&c| self.compiled[c].map_or(0, |(l, _)| l))
            .max()
            .unwrap_or(0);
        let inputs = children
            .iter()
            .map(|&c| {
                let track = &tracks[c];
                // At the top, a track's sound goes to its bus; only the rendered bus's is mixed.
                let on_bus = n != self.master || track.bus == self.plan.bus.name;
                let gain = match track.kind {
                    Some(TrackKind::Video) => 1.0,
                    _ => track.volume,
                };
                (c, on_bus.then_some(gain))
            })
            .collect::<Vec<_>>();
        for &c in children {
            self.reads[c].push(latency_in);
        }
        let chain = self.build_chain(n, &self.chain_of(n), latency_in, self.mix)?;
        let (latency, layout, warmup) = (chain.latency, chain.audio_layout, chain.warmup());
        Ok((
            NodeRt::Mix {
                inputs,
                audio_layout: self.mix,
                host: Stream::default(),
                chain,
            },
            latency,
            layout,
            warmup,
        ))
    }

    /// Compiles FX chain `list` of node `n`, whose input comes at `latency_in` with its sound in
    /// `host_audio`.
    fn build_chain(
        &mut self,
        n: usize,
        list: &[&Fx],
        latency_in: usize,
        host_audio: Layout,
    ) -> Result<ChainRt, EngineError> {
        let mut latency = latency_in;
        let mut audio = host_audio;
        let mut fx_rts = Vec::new();
        for fx in list {
            let desc = self.graphs[&fx.graph];
            let ports: Vec<(String, InputKind)> = crate::routing::input_ports(desc)
                .into_iter()
                .map(|p| (p.name, p.kind))
                .collect();
            let both = |name: &str| {
                ports
                    .iter()
                    .any(|(p, k)| p == name && *k == InputKind::Video)
                    && ports
                        .iter()
                        .any(|(p, k)| p == name && *k == InputKind::Audio)
            };
            let key = |name: &str, kind: InputKind| match kind {
                InputKind::Audio if both(name) => format!("{name}\u{0}sound"),
                _ => name.to_owned(),
            };
            let receive = |name: &str| {
                fx.receives.get(name).and_then(|track| {
                    self.tracks()
                        .iter()
                        .position(|t| &t.name == track)
                        .filter(|&t| self.compiled[t].is_some())
                })
            };
            let received: Vec<Option<usize>> =
                ports.iter().map(|(name, _)| receive(name)).collect();
            let align = received
                .iter()
                .flatten()
                .map(|&t| self.compiled[t].expect("compiled").0)
                .fold(latency, usize::max);
            let mut port_rts = Vec::new();
            let mut sources = HashMap::new();
            for ((name, kind), received) in ports.iter().zip(&received) {
                let video = *kind == InputKind::Video;
                let (fill, layout) = match *received {
                    Some(t) => {
                        self.reads[t].push(align);
                        let layout = if video {
                            self.video_layout
                        } else {
                            self.compiled[t].expect("compiled").1
                        };
                        (Fill::Receive { node: t, video }, layout)
                    }
                    None if is_main_port(name, *kind) && video => {
                        (Fill::HostVideo, self.video_layout)
                    }
                    None if is_main_port(name, *kind) => (Fill::HostAudio, audio),
                    None if video => (Fill::Zeros, self.video_layout),
                    None => (Fill::Zeros, self.mix),
                };
                let key = key(name, *kind);
                sources.insert(key.clone(), layout);
                port_rts.push(Port {
                    key,
                    signal: Signal::zeros(layout),
                    fill,
                });
            }
            // Generators take their layout from the main ports, which are there even when the
            // graph doesn't read them.
            for (name, layout, fill) in [
                (DEFAULT_VIDEO, self.video_layout, Fill::HostVideo),
                (DEFAULT_AUDIO, audio, Fill::HostAudio),
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
            let renders_video = desc.nodes.iter().any(|n| n.kind == OUTPUT);
            let renders_audio = desc
                .nodes
                .iter()
                .any(|n| n.kind == AUDIO_OUTPUT && audio_output_bus(n) == self.plan.bus.name);
            let keyed = keyed(
                desc,
                &|name, kind| key(name, kind),
                renders_video,
                renders_audio,
                &self.plan.bus.name,
            );
            let options = CompileOptions {
                sources,
                ..self.plan.options.clone()
            };
            let graph = Graph::compile(&keyed, self.plan.registry, &options)
                .map_err(|e| fx_error(e, fx.graph, self.plan.single))?;
            let passthrough = renders_audio
                .then(|| graph.audio_passthrough())
                .flatten()
                .map(|key| key.split('\u{0}').next().unwrap_or(key).to_owned())
                .and_then(|port| fx.receives.get(&port).cloned());
            if renders_audio {
                self.renders_audio.push((n, passthrough));
            }
            let own = graph.latency_frames() as usize;
            let host_delay = align - latency;
            audio = graph.audio_layout().unwrap_or(audio);
            fx_rts.push(FxRt {
                graph_id: fx.graph,
                options,
                latency: own,
                warmup: graph.warmup_frames() as usize,
                renders_video,
                align,
                host_delay,
                host_ring: Ring::new(if host_delay > 0 { host_delay + 1 } else { 0 }),
                ports: port_rts,
                flags: VecDeque::new(),
                out: Stream::default(),
                active: false,
                graph,
            });
            latency = align + own;
        }
        Ok(ChainRt {
            fx: fx_rts,
            latency_in,
            latency,
            audio_layout: audio,
        })
    }
}

/// `desc` ready to compile as an FX: each input port reading the source `key` names, and an
/// input wired straight to an output for a stream it has no output of its own for.
fn keyed(
    desc: &GraphDesc,
    key: &dyn Fn(&str, InputKind) -> String,
    renders_video: bool,
    renders_audio: bool,
    bus: &str,
) -> GraphDesc {
    let mut desc = desc.clone();
    for node in &mut desc.nodes {
        if let Some((name, kind)) = port_of(&node.kind, &node.params) {
            let key = key(name, kind);
            node.params
                .insert(PORT_PARAM.to_owned(), ParamValue::Text(key));
        }
    }
    let mut through = |input: &str, kind: &str, port: String, output: &str, out_kind: &str| {
        let mut source = NodeDesc::new(input.to_owned(), kind);
        source
            .params
            .insert(PORT_PARAM.to_owned(), ParamValue::Text(port));
        let mut sink = NodeDesc::new(output.to_owned(), out_kind);
        if out_kind == AUDIO_OUTPUT {
            sink.params
                .insert(BUS_PARAM.to_owned(), ParamValue::Text(bus.to_owned()));
        }
        desc.nodes.push(source);
        desc.nodes.push(sink);
        desc.connections.push(Connection {
            from: input.to_owned(),
            to: output.to_owned(),
        });
    };
    if !renders_video {
        through(
            THROUGH_VIDEO_IN,
            VIDEO_INPUT,
            key(DEFAULT_VIDEO, InputKind::Video),
            THROUGH_VIDEO_OUT,
            OUTPUT,
        );
    }
    if !renders_audio {
        through(
            THROUGH_AUDIO_IN,
            AUDIO_INPUT,
            key(DEFAULT_AUDIO, InputKind::Audio),
            THROUGH_AUDIO_OUT,
            AUDIO_OUTPUT,
        );
    }
    desc
}

/// The nodes `deps` lead to from `root`, each after what it depends on (`root` last), or the
/// nodes of a cycle.
fn topological(deps: &[Vec<usize>], root: usize) -> Result<Vec<usize>, Vec<usize>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        Open,
        Done,
    }
    fn visit(
        n: usize,
        deps: &[Vec<usize>],
        marks: &mut [Mark],
        path: &mut Vec<usize>,
        order: &mut Vec<usize>,
    ) -> Result<(), Vec<usize>> {
        match marks[n] {
            Mark::Done => return Ok(()),
            Mark::Open => {
                let start = path.iter().position(|&p| p == n).unwrap_or(0);
                let mut cycle = path[start..].to_vec();
                cycle.push(n);
                return Err(cycle);
            }
            Mark::New => {}
        }
        marks[n] = Mark::Open;
        path.push(n);
        for &d in &deps[n] {
            visit(d, deps, marks, path, order)?;
        }
        path.pop();
        marks[n] = Mark::Done;
        order.push(n);
        Ok(())
    }
    let mut marks = vec![Mark::New; deps.len()];
    let mut order = Vec::new();
    visit(root, deps, &mut marks, &mut Vec::new(), &mut order)?;
    Ok(order)
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

    #[test]
    fn a_cycle_is_found_and_order_follows_dependencies() {
        // 0 reads 1, 1 reads 2, master (3) mixes 0.
        let deps = vec![vec![1], vec![2], vec![], vec![0]];
        assert_eq!(topological(&deps, 3), Ok(vec![2, 1, 0, 3]));
        let cyclic = vec![vec![1], vec![0], vec![], vec![0]];
        assert_eq!(topological(&cyclic, 3), Err(vec![0, 1, 0]));
    }
}
