//! The graph layers of a render: every graph item compiled to its own graph and state, run on a
//! pipeline schedule so the layers compose in order.
//!
//! See [Graph layers](../../../docs/engine.md#graph-layers). Without layers the renderer runs a
//! stack of one layer holding one item that spans all time, which is the single-graph render.

use std::collections::{BTreeMap, HashMap, VecDeque};

use rastersong_graph::nodes::{LAYER_BELOW_SOURCE, NO_SOURCE, PORT_PARAM, TRACK_MIX_SOURCE};
use rastersong_graph::{
    CompileOptions, Graph, GraphDesc, Layout, NodeStats, OutputLevel, ParamValue, Registry, Signal,
    Sources,
};

use crate::audio::{AudioBlock, SinkResampler};
use crate::project::{Binding, InputKind, port_of};
use crate::renderer::Tracks;
use crate::timeline::Bus;
use crate::{EngineError, MediaError};

/// The host signal an audio input bound to *Layer below* reads. Video and audio inputs can't share
/// one name, because a name has one layout, so sound has its own.
pub const LAYER_BELOW_AUDIO: &str = "@layer_below_audio";
/// The host signal of an audio input nothing is bound to: silence.
pub const NO_AUDIO_SOURCE: &str = "@none_audio";

/// Whether `name` is a host signal the stack supplies itself rather than a track.
pub fn is_special_source(name: &str) -> bool {
    [
        TRACK_MIX_SOURCE,
        LAYER_BELOW_SOURCE,
        LAYER_BELOW_AUDIO,
        NO_SOURCE,
        NO_AUDIO_SOURCE,
    ]
    .contains(&name)
}

/// `desc` with every input node reading what `bindings` say of its port: the bound track, the
/// layer below, or nothing (zeros).
pub fn bind_inputs(desc: &GraphDesc, bindings: &BTreeMap<String, Binding>) -> GraphDesc {
    let mut desc = desc.clone();
    for node in &mut desc.nodes {
        let Some((port, kind)) = port_of(&node.kind, &node.params) else {
            continue;
        };
        let audio = kind == InputKind::Audio;
        let source = match (bindings.get(port).cloned(), audio) {
            (Some(Binding::Track(name)), _) => name,
            (Some(Binding::LayerBelow), false) => LAYER_BELOW_SOURCE.to_owned(),
            (Some(Binding::LayerBelow), true) => LAYER_BELOW_AUDIO.to_owned(),
            (None, false) => NO_SOURCE.to_owned(),
            (None, true) => NO_AUDIO_SOURCE.to_owned(),
        };
        node.params
            .insert(PORT_PARAM.to_owned(), ParamValue::Text(source));
    }
    desc
}

/// An item to compile, in frames.
pub struct ItemPlan {
    pub graph_id: u32,
    /// The graph with its inputs already bound.
    pub desc: GraphDesc,
    /// The output frames the item plays: `first..end`.
    pub first: i64,
    pub end: i64,
    /// Added to a source frame to get the graph's own frame, so time-based nodes count from the
    /// item's `start` at its first frame.
    pub frame_base: i64,
    pub pre_roll: bool,
}

/// What a stack is built from.
pub struct StackPlan<'a> {
    /// Layers bottom first.
    pub layers: Vec<Vec<ItemPlan>>,
    /// Whether this is a real layer set (items bound to track names, errors name the graph) rather
    /// than the single graph over the whole timeline.
    pub layered: bool,
    pub registry: &'a Registry,
    /// What every graph is compiled against.
    pub options: &'a CompileOptions,
    pub bus: &'a Bus,
    pub default_audio: Layout,
    pub fps: f64,
    pub audio_rate: u32,
    /// The graph the editor has open, whose nodes taps and statistics prefer.
    pub open: u32,
}

/// Frames kept by number, to delay a picture and sound by a few frames.
#[derive(Default)]
struct Ring {
    depth: usize,
    entries: VecDeque<RingEntry>,
}

struct RingEntry {
    frame: i64,
    video: Vec<f32>,
    audio: Vec<f32>,
}

impl Ring {
    fn new(depth: usize) -> Self {
        Self {
            depth,
            entries: VecDeque::with_capacity(depth),
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    /// Keeps `video` and `audio` as frame `frame`, forgetting the oldest beyond the depth.
    fn push(&mut self, frame: i64, video: &[f32], audio: &[f32]) {
        let mut entry = if self.entries.len() >= self.depth.max(1) {
            self.entries.pop_front().expect("not empty")
        } else {
            RingEntry {
                frame,
                video: Vec::new(),
                audio: Vec::new(),
            }
        };
        entry.frame = frame;
        entry.video.clear();
        entry.video.extend_from_slice(video);
        entry.audio.clear();
        entry.audio.extend_from_slice(audio);
        self.entries.push_back(entry);
    }

    fn get(&self, frame: i64) -> Option<&RingEntry> {
        self.entries.iter().rev().find(|e| e.frame == frame)
    }
}

/// The sound a graph renders, and what turns it into audio at the project's rate.
struct ItemAudio {
    layout: Layout,
    resampler: SinkResampler,
}

/// One graph item, compiled.
struct ItemRt {
    graph: Graph,
    graph_id: u32,
    /// The output frames it plays: `first..end`.
    first: i64,
    end: i64,
    /// Added to a source frame to get the graph's own frame.
    base: i64,
    pre_roll: bool,
    latency: usize,
    warmup: usize,
    audio: Option<ItemAudio>,
    /// Frames its output is held back to line up with the layer's latest item.
    lag_frames: usize,
    lag: Ring,
    passthrough: Option<String>,
}

impl ItemRt {
    /// Frames pre-rendered before the item, limited to `cap`; at least one when rendering sound,
    /// so the resampler has history.
    fn warm(&self, cap: usize) -> usize {
        let warm = self.warmup.min(cap);
        if self.audio.is_some() {
            warm.max(1)
        } else {
            warm
        }
    }

    /// The first source frame the item processes: its pre-roll before the left edge, or the edge.
    fn from(&self, cap: usize) -> i64 {
        let pre = if self.pre_roll { self.warm(cap) } else { 0 };
        self.first.saturating_sub(pre as i64).max(0)
    }

    /// One past the last source frame it processes: its last output frame comes out `latency`
    /// frames after its source.
    fn until(&self) -> i64 {
        self.end.saturating_add(self.latency as i64)
    }
}

/// One layer: the items on a lane and what flows through it.
struct LayerRt {
    items: Vec<ItemRt>,
    /// The latest of its items' latencies; the layer emits output frame `source - latency`.
    latency: usize,
    /// Latency of all the layers below: the layer processes source frame `step - offset`.
    offset: usize,
    /// Which set of track signals it reads.
    host: usize,
    /// Latency of all the layers above, which the layer's sound is held back by.
    sound_delay: usize,
    /// The layer below's picture and sound for the source frame being processed.
    below: Signal,
    below_audio: Signal,
    below_ring: Ring,
    /// What the layer emitted for its output frame, picture and sound.
    out_video: Signal,
    out_audio: Option<Signal>,
    /// The sound of the items playing, by output frame, kept until the whole stack catches up.
    sounds: VecDeque<(i64, Option<AudioBlock>)>,
    /// The item that supplied the last output frame.
    active: Option<usize>,
}

/// Track signals at one offset into the pipeline.
struct Host {
    offset: usize,
    signals: HashMap<String, Signal>,
    filled: bool,
}

/// The signals a graph sees: the host's, with the layer below standing in for its names.
struct View<'a> {
    host: &'a HashMap<String, Signal>,
    below: &'a Signal,
    below_audio: &'a Signal,
}

impl Sources for View<'_> {
    fn get(&self, name: &str) -> Option<&Signal> {
        match name {
            LAYER_BELOW_SOURCE => Some(self.below),
            LAYER_BELOW_AUDIO => Some(self.below_audio),
            _ => self.host.get(name),
        }
    }
}

/// Replaces the contents of `dst` with `src`, keeping its allocation.
fn assign(dst: &mut Vec<f32>, src: &[f32]) {
    dst.clear();
    dst.extend_from_slice(src);
}

/// Puts `data` in `slot`, keeping its allocation when the layout matches.
fn set_signal(slot: &mut Option<Signal>, layout: Layout, data: &[f32]) {
    match slot {
        Some(s) if s.layout == layout && s.data.len() == data.len() => {
            s.data.copy_from_slice(data);
        }
        _ => {
            *slot = Some(Signal {
                data: data.to_vec(),
                layout,
            });
        }
    }
}

/// The compiled graph layers.
pub struct Stack {
    layers: Vec<LayerRt>,
    hosts: Vec<Host>,
    layered: bool,
    latency: usize,
    open: u32,
    /// The item whose node statistics are reported: the open graph's first, else the first.
    stats_item: Option<(usize, usize)>,
    audio_rate: u32,
    channels: u32,
}

impl Stack {
    /// Compiles every item against `plan.options`, layer by layer from the bottom.
    pub fn build(plan: StackPlan) -> Result<Self, EngineError> {
        let video_layout = plan.options.output;
        let bus = plan.bus.clone().sanitized();
        let mut layers = Vec::new();
        let mut below_layout = plan.default_audio;
        let mut offset = 0;
        for items in plan.layers {
            let mut options = plan.options.clone();
            if plan.layered {
                options
                    .sources
                    .insert(LAYER_BELOW_AUDIO.to_owned(), below_layout);
            }
            let mut compiled = Vec::with_capacity(items.len());
            for item in items {
                let graph = Graph::compile(&item.desc, plan.registry, &options).map_err(|e| {
                    if plan.layered {
                        EngineError::Layer {
                            graph: item.graph_id,
                            error: e,
                        }
                    } else {
                        EngineError::Graph(e)
                    }
                })?;
                let passthrough = graph.audio_passthrough().map(str::to_owned);
                // A layered item's audio output is always rendered: a track wired straight to it
                // can't be played "as it is" when only the item's span should sound.
                let audio = graph
                    .audio_layout()
                    .filter(|_| plan.layered || passthrough.is_none())
                    .map(|layout| ItemAudio {
                        layout,
                        resampler: SinkResampler::new(
                            layout.len(),
                            layout.samples_per_pixel,
                            bus.channels,
                            plan.fps,
                            plan.audio_rate,
                        ),
                    });
                compiled.push(ItemRt {
                    graph_id: item.graph_id,
                    first: item.first,
                    end: item.end,
                    base: item.frame_base,
                    pre_roll: item.pre_roll,
                    latency: graph.latency_frames() as usize,
                    warmup: graph.warmup_frames() as usize,
                    audio,
                    lag_frames: 0,
                    lag: Ring::default(),
                    passthrough,
                    graph,
                });
            }
            let latency = compiled.iter().map(|i| i.latency).max().unwrap_or(0);
            for item in &mut compiled {
                item.lag_frames = latency - item.latency;
                item.lag = Ring::new(if item.lag_frames > 0 {
                    item.lag_frames + 1
                } else {
                    0
                });
            }
            let this_below = below_layout;
            // What the layer above reads as sound: the first sound an item here renders.
            below_layout = compiled
                .iter()
                .find_map(|i| i.audio.as_ref().map(|a| a.layout))
                .unwrap_or(plan.default_audio);
            layers.push(LayerRt {
                items: compiled,
                latency,
                offset,
                host: 0,
                sound_delay: 0,
                below: Signal::zeros(video_layout),
                below_audio: Signal::zeros(this_below),
                below_ring: Ring::new(if latency > 0 { latency + 1 } else { 0 }),
                out_video: Signal::zeros(video_layout),
                out_audio: None,
                sounds: VecDeque::new(),
                active: None,
            });
            offset += latency;
        }
        let total = offset;
        let mut above = 0;
        for layer in layers.iter_mut().rev() {
            layer.sound_delay = above;
            above += layer.latency;
        }

        let signals: HashMap<String, Signal> = plan
            .options
            .sources
            .iter()
            .map(|(name, layout)| (name.clone(), Signal::zeros(*layout)))
            .collect();
        let mut hosts = vec![Host {
            offset: 0,
            signals: signals.clone(),
            filled: false,
        }];
        for layer in &mut layers {
            if layer.offset != hosts.last().expect("one host").offset {
                hosts.push(Host {
                    offset: layer.offset,
                    signals: signals.clone(),
                    filled: false,
                });
            }
            layer.host = hosts.len() - 1;
        }

        let stats_item = layers
            .iter()
            .enumerate()
            .find_map(|(k, l)| {
                l.items
                    .iter()
                    .position(|i| i.graph_id == plan.open)
                    .map(|i| (k, i))
            })
            .or_else(|| {
                layers
                    .iter()
                    .position(|l| !l.items.is_empty())
                    .map(|k| (k, 0))
            });
        Ok(Self {
            layers,
            hosts,
            layered: plan.layered,
            latency: total,
            open: plan.open,
            stats_item,
            audio_rate: plan.audio_rate,
            channels: bus.channels,
        })
    }

    /// Frames between a source frame going in and its result coming out.
    pub fn latency(&self) -> usize {
        self.latency
    }

    /// Whether any item renders sound.
    pub fn has_audio(&self) -> bool {
        self.layers
            .iter()
            .flat_map(|l| &l.items)
            .any(|i| i.audio.is_some())
    }

    /// The rate and channel count of the rendered sound.
    pub fn sound_format(&self) -> (u32, u32) {
        (self.audio_rate, self.channels)
    }

    /// The track every item that plays sound passes straight through, for a render of one graph
    /// over the whole timeline.
    pub fn passthrough(&self) -> Option<&str> {
        if self.layered {
            return None;
        }
        self.layers
            .first()
            .and_then(|l| l.items.first())
            .and_then(|i| i.passthrough.as_deref())
    }

    /// Plays the sound at `rate` samples a second.
    pub fn set_audio_rate(&mut self, rate: u32, fps: f64) {
        self.audio_rate = rate;
        for layer in &mut self.layers {
            for item in &mut layer.items {
                if let Some(audio) = &mut item.audio {
                    audio.resampler = SinkResampler::new(
                        audio.layout.len(),
                        audio.layout.samples_per_pixel,
                        self.channels,
                        fps,
                        rate,
                    );
                }
            }
            layer.sounds.clear();
        }
    }

    /// Source steps to pre-render before a seek: each layer's longest warmup, added up and limited
    /// to `cap`, and at least one when rendering sound.
    pub fn warmup(&self, cap: usize) -> usize {
        let sum: usize = self
            .layers
            .iter()
            .map(|l| l.items.iter().map(|i| i.warm(cap)).max().unwrap_or(0))
            .sum();
        let warm = sum.min(cap);
        if self.has_audio() { warm.max(1) } else { warm }
    }

    /// Clears all state, as if no frame had been processed.
    pub fn reset(&mut self) {
        for host in &mut self.hosts {
            host.filled = false;
        }
        for layer in &mut self.layers {
            layer.below_ring.clear();
            layer.sounds.clear();
            layer.active = None;
            layer.out_audio = None;
            for item in &mut layer.items {
                item.graph.reset();
                item.lag.clear();
                if let Some(audio) = &mut item.audio {
                    audio.resampler.reset();
                }
            }
        }
    }

    /// The picture of the last step: the top layer's output, or the track mix with no layers.
    pub fn picture(&self) -> &Signal {
        match self.layers.last() {
            Some(top) => &top.out_video,
            None => self.hosts[0]
                .signals
                .get(TRACK_MIX_SOURCE)
                .expect("the track mix is always supplied"),
        }
    }

    /// The sound of output frame `n`: that of the top-most layer whose item playing then renders
    /// sound.
    pub fn sound(&self, n: i64) -> Option<&AudioBlock> {
        self.layers.iter().rev().find_map(|l| {
            l.sounds
                .iter()
                .rev()
                .find(|(frame, _)| *frame == n)
                .and_then(|(_, block)| block.as_ref())
        })
    }

    /// The items that supplied the last output frame, top-most layer first.
    fn active_items(&self) -> impl Iterator<Item = &ItemRt> {
        self.layers
            .iter()
            .rev()
            .filter_map(|l| l.active.map(|i| &l.items[i]))
    }

    /// The graph of the top-most active item playing the open graph, else the top-most active item.
    fn inspected(&self) -> Option<&ItemRt> {
        self.active_items()
            .find(|i| i.graph_id == self.open)
            .or_else(|| self.active_items().next())
    }

    pub fn tap(&self, node: &str, output: usize) -> Option<&Signal> {
        self.active_items()
            .filter(|i| i.graph_id == self.open)
            .chain(self.active_items())
            .find_map(|i| i.graph.tap(node, output))
    }

    pub fn levels(&self) -> Vec<OutputLevel> {
        self.inspected()
            .map(|i| i.graph.levels())
            .unwrap_or_default()
    }

    pub fn costs(&self) -> Vec<rastersong_graph::NodeCost> {
        self.inspected()
            .map(|i| i.graph.costs())
            .unwrap_or_default()
    }

    pub fn meters(&self) -> Vec<rastersong_graph::NodeMeters> {
        self.inspected()
            .map(|i| i.graph.meters())
            .unwrap_or_default()
    }

    pub fn param_levels(&self) -> Vec<rastersong_graph::ParamLevel> {
        self.inspected()
            .map(|i| i.graph.param_levels())
            .unwrap_or_default()
    }

    /// Each node's own latency and warmup, for the open graph's first item.
    pub fn node_stats(&self) -> &[NodeStats] {
        match self.stats_item {
            Some((k, i)) => self.layers[k].items[i].graph.node_stats(),
            None => &[],
        }
    }

    /// Processes source step `m` through every layer. `cap` limits item pre-rolls.
    pub fn step(&mut self, m: usize, tracks: &mut Tracks, cap: usize) -> Result<(), EngineError> {
        let Self {
            layers,
            hosts,
            layered,
            ..
        } = self;
        let m = m as i64;
        for host in hosts.iter_mut() {
            host.filled = false;
        }
        // Layer 0 reads the track mix, and with no layers it is the picture.
        fill_host(&mut hosts[0], m, tracks)?;

        for k in 0..layers.len() {
            let (lower, rest) = layers.split_at_mut(k);
            let layer = &mut rest[0];
            let LayerRt {
                items,
                latency,
                offset,
                host,
                sound_delay,
                below,
                below_audio,
                below_ring,
                out_video,
                out_audio,
                sounds,
                active,
            } = layer;
            let s = m - *offset as i64;

            // The layer below's output for this source frame.
            if *layered || k > 0 {
                match lower.last() {
                    None => match hosts[0].signals.get(TRACK_MIX_SOURCE) {
                        Some(mix) => assign(&mut below.data, &mix.data),
                        None => below.data.fill(0.0),
                    },
                    Some(prev) => assign(&mut below.data, &prev.out_video.data),
                }
                match lower.last().and_then(|p| p.out_audio.as_ref()) {
                    Some(a) if a.layout.same_shape(&below_audio.layout) => {
                        assign(&mut below_audio.data, &a.data);
                    }
                    _ => below_audio.data.fill(0.0),
                }
                if *latency > 0 && s >= 0 {
                    below_ring.push(s, &below.data, &below_audio.data);
                }
            }

            let f_out = s - *latency as i64;
            let supplier = items
                .iter()
                .position(|it| it.first <= f_out && f_out < it.end);
            let mut have_video = false;
            for (i, item) in items.iter_mut().enumerate() {
                let from = item.from(cap);
                if s < from || s >= item.until() {
                    continue;
                }
                fill_host(&mut hosts[*host], m, tracks)?;
                if s == from {
                    item.graph.reset();
                    item.lag.clear();
                    if let Some(audio) = &mut item.audio {
                        audio.resampler.reset();
                    }
                }
                let view = View {
                    host: &hosts[*host].signals,
                    below: &*below,
                    below_audio: &*below_audio,
                };
                let frame = (s + item.base).max(0) as u64;
                item.graph.process(frame, &view)?;
                let output = item.graph.output();
                if item.lag_frames > 0 {
                    let audio = item.graph.audio_output().map_or(&[][..], |a| &a.data[..]);
                    item.lag.push(s - item.latency as i64, &output.data, audio);
                } else if supplier == Some(i) {
                    assign(&mut out_video.data, &output.data);
                    have_video = true;
                }
            }

            *active = supplier;
            let mut sound = None;
            match supplier {
                Some(i) => {
                    let item = &mut items[i];
                    let entry = if item.lag_frames > 0 {
                        item.lag.get(f_out)
                    } else {
                        None
                    };
                    if let Some(e) = entry {
                        assign(&mut out_video.data, &e.video);
                        have_video = true;
                    }
                    let data: Option<&[f32]> = if item.lag_frames > 0 {
                        entry.map(|e| &e.audio[..])
                    } else {
                        item.graph.audio_output().map(|a| &a.data[..])
                    };
                    match (&mut item.audio, data) {
                        (Some(audio), Some(data)) if data.len() == audio.layout.len() => {
                            sound = Some(audio.resampler.push(f_out, data));
                            set_signal(out_audio, audio.layout, data);
                        }
                        _ => *out_audio = None,
                    }
                }
                None => {
                    // No item plays: the layer below shows through, as it was `latency` frames ago.
                    let held = if *latency > 0 {
                        below_ring.get(f_out)
                    } else {
                        None
                    };
                    match (held, *latency) {
                        (Some(e), _) => {
                            set_signal(out_audio, below_audio.layout, &e.audio);
                        }
                        (None, 0) => set_signal(out_audio, below_audio.layout, &below_audio.data),
                        (None, _) => *out_audio = None,
                    }
                }
            }
            if !have_video {
                let held = if *latency > 0 {
                    below_ring.get(f_out)
                } else {
                    None
                };
                match held {
                    Some(e) => assign(&mut out_video.data, &e.video),
                    None if *latency == 0 && supplier.is_none() => {
                        assign(&mut out_video.data, &below.data);
                    }
                    None => out_video.data.fill(0.0),
                }
            }
            sounds.push_back((f_out, sound));
            while sounds.len() > *sound_delay + 1 {
                sounds.pop_front();
            }
        }
        Ok(())
    }
}

/// Reads the tracks at the host's offset, once a step.
fn fill_host(host: &mut Host, m: i64, tracks: &mut Tracks) -> Result<(), MediaError> {
    if host.filled {
        return Ok(());
    }
    host.filled = true;
    tracks.fill(m - host.offset as i64, &mut host.signals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_read_what_the_bindings_say() {
        let desc = GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [
                { "id": "a", "type": "video_input" }, { "id": "a2", "type": "video_input" },
                { "id": "b", "type": "video_input", "params": { "port": "Kick" } },
                { "id": "c", "type": "video_input", "params": { "port": "x" } },
                { "id": "s", "type": "audio_input" },
                { "id": "t", "type": "audio_input", "params": { "port": "Kick" } },
                { "id": "o", "type": "output" } ] }"#,
        )
        .unwrap();
        let bindings = BTreeMap::from([
            ("Video".to_owned(), Binding::LayerBelow),
            ("Audio".to_owned(), Binding::LayerBelow),
            ("Kick".to_owned(), Binding::Track("kick".into())),
        ]);
        let bound = bind_inputs(&desc, &bindings);
        let source = |id: &str| {
            let node = bound.nodes.iter().find(|n| n.id == id).unwrap();
            node.params.get(PORT_PARAM).cloned()
        };
        let text = |s: &str| Some(ParamValue::Text(s.to_owned()));
        // Nodes reading one port read the same thing.
        assert_eq!(source("a"), text(LAYER_BELOW_SOURCE));
        assert_eq!(source("a2"), text(LAYER_BELOW_SOURCE));
        assert_eq!(source("b"), text("kick"));
        assert_eq!(source("t"), text("kick"));
        // An unbound port reads nothing.
        assert_eq!(source("c"), text(NO_SOURCE));
        assert_eq!(source("s"), text(LAYER_BELOW_AUDIO));
        assert_eq!(source("o"), None);
    }

    #[test]
    fn a_ring_finds_recent_frames_and_forgets_old_ones() {
        let mut ring = Ring::new(3);
        for f in 0..5 {
            ring.push(f, &[f as f32], &[]);
        }
        assert!(ring.get(1).is_none());
        assert_eq!(ring.get(2).unwrap().video, [2.0]);
        assert_eq!(ring.get(4).unwrap().video, [4.0]);
        ring.clear();
        assert!(ring.get(4).is_none());
    }
}
