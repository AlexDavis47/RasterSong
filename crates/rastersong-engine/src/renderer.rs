//! The sequential renderer shared by preview and offline rendering.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rastersong_graph::nodes::{
    AUDIO_INPUT, DEFAULT_AUDIO, DEFAULT_VIDEO, LAYER_BELOW_SOURCE, NO_SOURCE, PORT_PARAM,
    TRACK_MIX_SOURCE, VIDEO_INPUT,
};
use rastersong_graph::{
    CompileOptions, GraphDesc, Layout, OutputLevel, ParamValue, Registry, Signal, Tempo,
};
use rastersong_media::{MediaBackend, MediaError, Rational, VideoSource};

use crate::EngineError;
use crate::audio::{AudioBlock, AudioSink, DEFAULT_AUDIO_RATE, frame_start};
use crate::project::{DEFAULT_MAX_WARMUP_FRAMES, LayerSet, MAX_WARMUP_FRAMES_LIMIT};
use crate::sources::{Modulator, fill_video, to_rgb8};
use crate::stack::{
    ItemPlan, LAYER_BELOW_AUDIO, NO_AUDIO_SOURCE, Stack, StackPlan, bind_inputs, is_special_source,
};
use crate::timeline::{Bus, Item, Timebase, item_at, items_end};

/// The source a video input reads by default.
pub const VIDEO_SOURCE: &str = DEFAULT_VIDEO;
/// The node type that reads audio tracks, and the track it reads by default.
pub const DEFAULT_AUDIO_TRACK: &str = DEFAULT_AUDIO;

/// What a track plays.
#[derive(Debug, Clone)]
pub enum TrackMedia {
    /// A video stream of a file (its best for `None`), opened by the renderer.
    Video {
        path: PathBuf,
        stream: Option<usize>,
    },
    /// Decoded audio.
    Audio(Arc<Modulator>),
}

/// A track the graph's input nodes can read, by name: its media, placed by its items.
#[derive(Debug, Clone)]
pub struct RenderTrack {
    pub name: String,
    pub media: TrackMedia,
    pub items: Vec<Item>,
    /// Whether a video track takes part in the track mix's picture: false when it is muted, or
    /// another track is soloed. Graphs read it either way.
    pub in_mix: bool,
}

impl RenderTrack {
    /// An audio track playing `modulator` from `position` seconds, whole.
    pub fn audio(name: impl Into<String>, modulator: Arc<Modulator>, position: f64) -> Self {
        Self {
            name: name.into(),
            media: TrackMedia::Audio(modulator),
            items: vec![Item::whole(position)],
            in_mix: true,
        }
    }

    /// A video track playing the file at `path` from the start, whole.
    pub fn video(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            media: TrackMedia::Video {
                path: path.into(),
                stream: None,
            },
            items: vec![Item::whole(0.0)],
            in_mix: true,
        }
    }
}

/// The size to render at.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum OutputSize {
    /// The project's size.
    #[default]
    Native,
    Exact(u32, u32),
    /// A fraction of the project's size, e.g. 0.5 for a half-resolution preview.
    Scaled(f32),
}

impl OutputSize {
    pub fn resolve(self, width: u32, height: u32) -> (u32, u32) {
        match self {
            Self::Native => (width, height),
            Self::Exact(w, h) => (w, h),
            Self::Scaled(f) => {
                // At least 2 pixels, so chroma subsampling still has something to work with.
                let scale = |v: u32| ((v as f32 * f).round() as u32).max(2);
                (scale(width), scale(height))
            }
        }
    }
}

/// What a render produces, known before the first frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderInfo {
    pub width: u32,
    pub height: u32,
    pub frame_rate: Rational,
    /// The project's length: frames up to the end of the last item.
    pub frames: usize,
    /// The project timebase in use, which a project without one of its own takes from its
    /// first video track.
    pub timebase: Timebase,
}

/// How far along rendering one frame is: reported before each source frame is processed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// Source frames processed so far for this output frame.
    pub done: usize,
    /// Source frames it takes in all, including the warm-up before it.
    pub total: usize,
    /// Whether the graph was reset first, so this includes warming up its history.
    pub restarted: bool,
}

/// A video track being read: the video conformed to the project's grid and size.
struct VideoReader {
    name: String,
    /// Whether the track mix's picture can show it.
    in_mix: bool,
    video: Box<dyn VideoSource>,
    items: Vec<Item>,
    /// The video's length in seconds, and its frame rate.
    duration: f64,
    fps: f64,
    /// Whether the source holds a decoded frame, so a damaged one can repeat it.
    have_frame: bool,
}

impl VideoReader {
    /// The video's frame shown at resource time `seconds`.
    fn frame_at(&self, seconds: f64) -> usize {
        let last = self.video.info().frame_count.saturating_sub(1);
        ((seconds * self.fps + 1e-6).floor().max(0.0) as usize).min(last)
    }
}

/// An audio track being read.
struct AudioReader {
    name: String,
    modulator: Arc<Modulator>,
    items: Vec<Item>,
}

/// The project's tracks as signals: one set of readers (and decoders) shared by every layer.
pub(crate) struct Tracks {
    videos: Vec<VideoReader>,
    audio: Vec<AudioReader>,
    fps: f64,
}

impl Tracks {
    /// Fills `out` with every track's signal for source frame `s` and the track mix's picture
    /// (when `out` has one). Before the start everything is zeros.
    pub(crate) fn fill(
        &mut self,
        s: i64,
        out: &mut HashMap<String, Signal>,
    ) -> Result<(), MediaError> {
        if s < 0 {
            out.values_mut().for_each(|signal| signal.data.fill(0.0));
            return Ok(());
        }
        let s = s as usize;
        let (start, end) = (s as f64 / self.fps, (s + 1) as f64 / self.fps);
        for reader in &mut self.videos {
            let signal = out.get_mut(&reader.name).unwrap();
            // A hair past the frame's start, so an item starting on it is found despite rounding.
            let Some((_, at)) = item_at(&reader.items, start + 1e-9, reader.duration) else {
                signal.data.fill(0.0);
                continue;
            };
            let index = reader.frame_at(at);
            match reader.video.frame(index) {
                Ok(frame) => {
                    fill_video(&frame, signal);
                    reader.have_frame = true;
                }
                // A damaged frame repeats the previous one rather than failing the render.
                Err(MediaError::FrameUnavailable(i)) if reader.have_frame => {
                    tracing::warn!(
                        frame = i,
                        "frame could not be decoded; repeating the previous frame"
                    );
                }
                Err(e) => return Err(e),
            }
        }
        // The track mix's picture: the top video track with an item now, or zeros.
        if out.contains_key(TRACK_MIX_SOURCE) {
            let top = self
                .videos
                .iter()
                .find(|v| {
                    v.name != TRACK_MIX_SOURCE
                        && v.in_mix
                        && item_at(&v.items, start + 1e-9, v.duration).is_some()
                })
                .map(|v| v.name.as_str());
            match top {
                Some(top) => {
                    if let [Some(mix), Some(video)] = out.get_disjoint_mut([TRACK_MIX_SOURCE, top])
                    {
                        mix.data.copy_from_slice(&video.data);
                    }
                }
                None => {
                    if let Some(mix) = out.get_mut(TRACK_MIX_SOURCE) {
                        mix.data.fill(0.0);
                    }
                }
            }
        }
        for track in &self.audio {
            let block = &mut out.get_mut(&track.name).unwrap().data;
            track.modulator.fill_items(&track.items, start, end, block);
        }
        Ok(())
    }
}

/// Renders output frames of the project's tracks through one compiled graph, or through the graph
/// items on the project's layers (see [Graph layers](../../../docs/engine.md#graph-layers)).
///
/// The graphs are stateful, so frames are produced by processing source frames in order. Asking
/// for the next frame is the fast path. Asking for any other frame resets the graphs and first
/// renders (and discards) [`Self::warmup_frames`] frames so stateful nodes have history. That
/// makes the result exact for nodes with finite memory and a close approximation for
/// infinite-memory ones (feedback, IIR filters). Rendering from frame 0 is always exact.
pub struct Renderer {
    tracks: Tracks,
    stack: Stack,
    /// What the graphs were compiled against, so editors can inspect other graphs the same way.
    options: CompileOptions,
    info: RenderInfo,
    fps: f64,
    latency: usize,
    warmup: usize,
    /// The most frames to pre-render before a seek, whatever the graph asks for.
    max_warmup: usize,
    /// The next source frame to process, if the graph's state is positioned somewhere.
    next_source: Option<usize>,
    rgb: Vec<u8>,
    /// The bus rendered: only the Audio Output writing to it is compiled, and its sound has
    /// the bus's channels.
    bus: Bus,
    /// The audio of the last rendered frame, when the graph renders sound.
    audio: Option<AudioBlock>,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("info", &self.info)
            .field("latency", &self.latency)
            .field("warmup", &self.warmup)
            .field("next_source", &self.next_source)
            .finish_non_exhaustive()
    }
}

/// The span of a graph item that plays everywhere: the single graph over the whole timeline.
const EVERYWHERE: (i64, i64) = (i64::MIN / 2, i64::MAX / 2);

impl Renderer {
    /// Opens the video tracks at `size` and compiles `graph` for the project's `tracks` on its
    /// `timebase`, rendering the sound of output bus `bus`. Without a timebase the first video
    /// track sets it, or [`Timebase::DEFAULT`] when there is none. Input nodes that name a track
    /// that isn't there read zeros. Video tracks are listed top first: the track mix's picture
    /// ([`TRACK_MIX_SOURCE`]) shows the first in the mix with an item at each frame.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        backend: &dyn MediaBackend,
        timebase: Option<Timebase>,
        tracks: &[RenderTrack],
        graph: &GraphDesc,
        tempo: Tempo,
        bus: &Bus,
        registry: &Registry,
        size: OutputSize,
    ) -> Result<Self, EngineError> {
        Self::with_layers(
            backend, timebase, tracks, graph, None, tempo, bus, registry, size,
        )
    }

    /// [`Self::new`] for a project with graph items: with `layers`, each item is compiled to its
    /// own graph and the layers compose in order over the track mix, and `graph` is only the
    /// description of the open graph (`layers.open`). Without them `graph` renders over the whole
    /// timeline.
    #[allow(clippy::too_many_arguments)]
    pub fn with_layers(
        backend: &dyn MediaBackend,
        timebase: Option<Timebase>,
        tracks: &[RenderTrack],
        graph: &GraphDesc,
        layers: Option<&LayerSet>,
        tempo: Tempo,
        bus: &Bus,
        registry: &Registry,
        size: OutputSize,
    ) -> Result<Self, EngineError> {
        let mut videos = Vec::new();
        let mut audio = Vec::new();
        for track in tracks {
            match &track.media {
                TrackMedia::Video { path, stream } => {
                    let video = backend.open_video_stream(path, *stream)?;
                    let info = video.info();
                    let fps = info.frame_rate.as_f64();
                    videos.push(VideoReader {
                        name: track.name.clone(),
                        in_mix: track.in_mix,
                        duration: info.frame_count as f64 / fps,
                        fps,
                        video,
                        items: track.items.clone(),
                        have_frame: false,
                    });
                }
                TrackMedia::Audio(modulator) => audio.push(AudioReader {
                    name: track.name.clone(),
                    modulator: modulator.clone(),
                    items: track.items.clone(),
                }),
            }
        }
        let timebase = timebase
            .or_else(|| {
                videos.first().map(|v| {
                    let info = v.video.info();
                    Timebase {
                        width: info.width,
                        height: info.height,
                        frame_rate: info.frame_rate,
                    }
                })
            })
            .unwrap_or(Timebase::DEFAULT);
        if !timebase.is_valid() {
            return Err(EngineError::Timebase(timebase));
        }
        let fps = timebase.fps();
        let (width, height) = size.resolve(timebase.width, timebase.height);
        for reader in &mut videos {
            let info = reader.video.info();
            let native = (info.width, info.height) == (width, height);
            reader
                .video
                .set_output_size((!native).then_some((width, height)));
        }
        let end = videos
            .iter()
            .map(|v| items_end(&v.items, v.duration))
            .chain(
                audio
                    .iter()
                    .map(|a| items_end(&a.items, a.modulator.duration_secs())),
            )
            .fold(0.0, f64::max);

        let plan = match layers {
            None => vec![vec![ItemPlan {
                graph_id: 0,
                desc: graph.clone(),
                first: EVERYWHERE.0,
                end: EVERYWHERE.1,
                frame_base: 0,
                pre_roll: true,
            }]],
            Some(set) => plan_layers(set, graph, fps),
        };
        let layered = layers.is_some();
        let open = layers.map_or(0, |set| set.open);

        for name in plan
            .iter()
            .flatten()
            .flat_map(|item| source_names(&item.desc, AUDIO_INPUT, DEFAULT_AUDIO))
        {
            if !is_special_source(&name)
                && !audio.iter().any(|t| t.name == name)
                && !videos.iter().any(|v| v.name == name)
            {
                audio.push(AudioReader {
                    name,
                    modulator: Arc::new(Modulator::silent()),
                    items: Vec::new(),
                });
            }
        }

        let video_layout = Layout::video(width, height);
        let mut layouts: HashMap<String, Layout> = videos
            .iter()
            .map(|v| (v.name.clone(), video_layout))
            .collect();
        for track in &audio {
            layouts.insert(track.name.clone(), track.modulator.layout(fps));
        }
        // A video input naming no track reads a picture of zeros, like a gap.
        for name in plan
            .iter()
            .flatten()
            .flat_map(|item| source_names(&item.desc, VIDEO_INPUT, DEFAULT_VIDEO))
        {
            layouts.entry(name).or_insert(video_layout);
        }
        // Generators take their layout from the main ports, which are there even when nothing
        // is called that: the picture, and the first audio track's shape (or silence at the
        // default rate when there is none).
        layouts
            .entry(DEFAULT_VIDEO.to_owned())
            .or_insert(video_layout);
        if !layouts.contains_key(DEFAULT_AUDIO) {
            let layout = audio.first().map_or_else(
                || Modulator::silent().layout(fps),
                |track| track.modulator.layout(fps),
            );
            layouts.insert(DEFAULT_AUDIO.to_owned(), layout);
        }
        let default_audio = layouts[DEFAULT_AUDIO];
        if layered {
            // The signals only layers supply: the track mix's picture, the layer below and
            // nothing, for pictures and for sound.
            for name in [TRACK_MIX_SOURCE, LAYER_BELOW_SOURCE, NO_SOURCE] {
                layouts.entry(name.to_owned()).or_insert(video_layout);
            }
            for name in [LAYER_BELOW_AUDIO, NO_AUDIO_SOURCE] {
                layouts.entry(name.to_owned()).or_insert(default_audio);
            }
        }
        let options = CompileOptions {
            frame_rate: fps,
            tempo,
            sources: layouts,
            output: video_layout,
            pixel_scale: f64::from(width) / f64::from(timebase.width),
            audio_bus: bus.name.clone(),
        };
        let stack = Stack::build(StackPlan {
            layers: plan,
            layered,
            registry,
            options: &options,
            bus,
            default_audio,
            fps,
            audio_rate: DEFAULT_AUDIO_RATE,
            open,
        })?;
        let bus = bus.clone().sanitized();

        let max_warmup = DEFAULT_MAX_WARMUP_FRAMES as usize;
        Ok(Self {
            tracks: Tracks { videos, audio, fps },
            info: RenderInfo {
                width,
                height,
                frame_rate: timebase.frame_rate,
                frames: timebase.frames_in(end),
                timebase,
            },
            fps,
            options,
            latency: stack.latency(),
            warmup: stack.warmup(max_warmup),
            max_warmup,
            stack,
            next_source: None,
            rgb: Vec::with_capacity(video_layout.len()),
            bus,
            audio: None,
        })
    }

    /// Limits the frames pre-rendered before a seek to `frames`. Rendering restarts from the
    /// next request.
    pub fn set_max_warmup_frames(&mut self, frames: u32) {
        self.max_warmup = frames.min(MAX_WARMUP_FRAMES_LIMIT) as usize;
        self.warmup = self.stack.warmup(self.max_warmup);
        self.next_source = None;
    }

    /// Renders the audio output at `rate` samples a second (48 kHz unless set).
    pub fn set_audio_rate(&mut self, rate: u32) {
        self.stack.set_audio_rate(rate.max(1), self.fps);
        self.warmup = self.stack.warmup(self.max_warmup);
        self.audio = None;
        self.next_source = None;
    }

    /// The output bus whose sound is rendered.
    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    /// What the render's audio is: the bus's track mix, a track passed through, or rendered
    /// sound.
    pub fn audio_sink(&self) -> AudioSink {
        if self.stack.has_audio() {
            let (sample_rate, channels) = self.stack.sound_format();
            return AudioSink::Rendered {
                sample_rate,
                channels,
            };
        }
        match self.stack.passthrough() {
            Some(track) => AudioSink::Passthrough(track.to_owned()),
            None => AudioSink::TrackMix,
        }
    }

    /// The rendered audio of the frame [`Self::render`] last returned, when the graph renders
    /// sound ([`AudioSink::Rendered`]).
    pub fn audio(&self) -> Option<&AudioBlock> {
        self.audio.as_ref()
    }

    /// The picture and sound of the frame [`Self::render`] last returned.
    pub fn output(&self) -> (&[u8], Option<&AudioBlock>) {
        (&self.rgb, self.audio.as_ref())
    }

    /// The output frame the graph's state is at, if it is positioned anywhere: the one
    /// [`Self::render`] last returned.
    pub fn rendered(&self) -> Option<usize> {
        self.next_source
            .and_then(|s| s.checked_sub(self.latency + 1))
    }

    /// What output `output` of node `node` produced in the last processed source frame, or
    /// `None` if there is no such output (the node doesn't feed the graph's output).
    /// Read-only: nothing about the render changes.
    ///
    /// With graph layers this reads the items that played at the last rendered frame, the open
    /// graph's first and then the top-most layer's down, and each item's graph at the last source
    /// frame it processed.
    pub fn tap(&self, node: &str, output: usize) -> Option<&Signal> {
        self.stack.tap(node, output)
    }

    /// The level of every node output in the last rendered frame. With graph layers, of the open
    /// graph's item playing then, else the top-most item playing; likewise for the costs, meters
    /// and parameter levels below.
    pub fn levels(&self) -> Vec<OutputLevel> {
        self.stack.levels()
    }

    /// How long every node took to process the last rendered frame.
    pub fn costs(&self) -> Vec<rastersong_graph::NodeCost> {
        self.stack.costs()
    }

    /// The meter values of the nodes that publish them, from the last rendered frame.
    pub fn meters(&self) -> Vec<rastersong_graph::NodeMeters> {
        self.stack.meters()
    }

    /// The value of every modulated parameter in the last rendered frame.
    pub fn param_levels(&self) -> Vec<rastersong_graph::ParamLevel> {
        self.stack.param_levels()
    }

    pub fn info(&self) -> &RenderInfo {
        &self.info
    }

    /// The sources, frame rate, tempo and output size the graph was compiled against.
    pub fn compile_options(&self) -> &CompileOptions {
        &self.options
    }

    /// Each node's own latency and warmup: the open graph's first item's, else the first item's.
    pub fn node_stats(&self) -> &[rastersong_graph::NodeStats] {
        self.stack.node_stats()
    }

    /// Frames between a source frame going in and its result coming out: with layers, the sum
    /// of the layers' latencies.
    pub fn latency_frames(&self) -> usize {
        self.latency
    }

    pub fn warmup_frames(&self) -> usize {
        self.warmup
    }

    /// Renders output frame `index` as packed RGB8.
    ///
    /// `cancel` is checked before each source frame is processed; if it returns true, rendering
    /// stops and `Ok(None)` is returned. The renderer stays consistent, so a later call carries on.
    pub fn render(
        &mut self,
        index: usize,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Option<&[u8]>, EngineError> {
        self.render_with(index, cancel, &|_| {})
    }

    /// [`Self::render`], reporting progress before each source frame is processed.
    pub fn render_with(
        &mut self,
        index: usize,
        cancel: &dyn Fn() -> bool,
        progress: &dyn Fn(Step),
    ) -> Result<Option<&[u8]>, EngineError> {
        let count = self.info.frames;
        if index >= count {
            return Err(MediaError::FrameOutOfRange { index, count }.into());
        }

        if self.rendered() == Some(index) {
            return Ok(Some(&self.rgb));
        }
        // Processing source frame `m` produces output frame `m - latency`.
        let next_output = self.next_source.and_then(|s| s.checked_sub(self.latency));
        let continue_forward =
            next_output.is_some_and(|next| next <= index && index - next <= self.warmup);
        if !continue_forward {
            self.stack.reset();
            self.next_source = Some(index.saturating_sub(self.warmup));
        }

        let target_source = index + self.latency;
        let first = self.next_source.unwrap_or(target_source);
        let total = (target_source + 1).saturating_sub(first);
        while let Some(source) = self.next_source.filter(|&s| s <= target_source) {
            if cancel() {
                return Ok(None);
            }
            progress(Step {
                done: source - first,
                total,
                restarted: !continue_forward,
            });
            self.process(source)?;
            self.next_source = Some(source + 1);
        }
        Ok(Some(&self.rgb))
    }

    /// Feeds source frame `m` (and its audio) through the graphs and keeps the output.
    fn process(&mut self, m: usize) -> Result<(), EngineError> {
        if let Err(e) = self.stack.step(m, &mut self.tracks, self.max_warmup) {
            self.next_source = None;
            return Err(e);
        }
        to_rgb8(self.stack.picture(), &mut self.rgb);
        // Like the picture, the sound comes out `latency` frames after its source. Frames no item
        // supplies sound for are silent.
        if self.stack.has_audio() {
            let frame = m as i64 - self.latency as i64;
            self.audio = Some(
                self.stack
                    .sound(frame)
                    .cloned()
                    .unwrap_or_else(|| self.silence(frame)),
            );
        }
        Ok(())
    }

    /// Silence for output frame `frame`, in the format of the rendered sound.
    fn silence(&self, frame: i64) -> AudioBlock {
        let (sample_rate, channels) = self.stack.sound_format();
        let rate = f64::from(sample_rate);
        let first = frame_start(frame, rate, self.fps);
        let end = frame_start(frame + 1, rate, self.fps);
        AudioBlock {
            start: first.max(0) as u64,
            sample_rate,
            channels,
            samples: if first < 0 {
                Vec::new()
            } else {
                vec![0.0; (end - first).max(0) as usize * channels as usize]
            },
        }
    }
}

/// The items of `set` as frames to compile, layer by layer from the bottom. Empty layers pass
/// everything through, so they are left out; so are items of a graph the project doesn't have.
fn plan_layers(set: &LayerSet, open: &GraphDesc, fps: f64) -> Vec<Vec<ItemPlan>> {
    // The first frame at or after a time, in the half-open spans items play (a hair of slack for
    // times written as multiples of the frame).
    let frame_of = |t: f64| (t * fps - 1e-6).ceil() as i64;
    set.layers
        .iter()
        .map(|layer| {
            layer
                .iter()
                .filter_map(|item| {
                    let desc = if item.graph == set.open {
                        open
                    } else {
                        &set.graphs.iter().find(|(id, _)| *id == item.graph)?.1
                    };
                    let (first, end) = (
                        frame_of(item.position),
                        frame_of(item.position + item.length),
                    );
                    (end > first).then(|| ItemPlan {
                        graph_id: item.graph,
                        desc: bind_inputs(desc, &item.bindings),
                        first,
                        end,
                        frame_base: (item.start * fps).round() as i64 - first,
                        pre_roll: item.pre_roll,
                    })
                })
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .collect()
}

/// The track names read by the graph's input nodes of type `kind`, which read `default` unless
/// set otherwise.
fn source_names(graph: &GraphDesc, kind: &str, default: &str) -> Vec<String> {
    graph
        .nodes
        .iter()
        .filter(|n| n.kind == kind)
        .map(|n| match n.params.get(PORT_PARAM) {
            Some(ParamValue::Text(name)) => name.clone(),
            _ => default.to_owned(),
        })
        .collect()
}
