//! The sequential renderer shared by preview and offline rendering.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rastersong_graph::nodes::{DEFAULT_AUDIO, DEFAULT_VIDEO};
use rastersong_graph::{CompileOptions, GraphDesc, Layout, OutputLevel, Registry, Signal, Tempo};
use rastersong_media::{MediaBackend, MediaError, Rational, VideoSource};

use crate::EngineError;
use crate::audio::{AudioBlock, AudioSink, DEFAULT_AUDIO_RATE, SinkResampler};
use crate::project::{DEFAULT_MAX_WARMUP_FRAMES, MAX_WARMUP_FRAMES_LIMIT};
use crate::route::{MediaInfo, MediaReader, Route, RoutePlan, Stream, mix_layout};
use crate::routing::{RouteTrack, Routing, input_ports};
use crate::sources::{Modulator, fill_video, to_rgb8};
use crate::timeline::{Bus, Fx, Item, Timebase, TrackKind, item_at, items_end};

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
    modulator: Arc<Modulator>,
    items: Vec<Item>,
    layout: Layout,
}

enum Reader {
    Video(VideoReader),
    Audio(AudioReader),
}

/// The project's tracks as signals, read for the route.
pub(crate) struct Tracks {
    readers: Vec<Reader>,
    fps: f64,
    video_layout: Layout,
}

impl MediaReader for Tracks {
    fn read(&mut self, track: usize, s: i64, out: &mut Stream) -> Result<(), MediaError> {
        out.picture = false;
        if s < 0 {
            out.clear();
            return Ok(());
        }
        let (start, end) = (s as f64 / self.fps, (s + 1) as f64 / self.fps);
        match &mut self.readers[track] {
            Reader::Video(reader) => {
                out.audio.clear();
                // A hair past the frame's start, so an item starting on it is found despite
                // rounding.
                let Some((_, at)) = item_at(&reader.items, start + 1e-9, reader.duration) else {
                    out.video.clear();
                    return Ok(());
                };
                let index = reader.frame_at(at);
                match reader.video.frame(index) {
                    Ok(frame) => {
                        let mut signal = Signal {
                            data: std::mem::take(&mut out.video),
                            layout: self.video_layout,
                        };
                        signal.data.resize(self.video_layout.len(), 0.0);
                        fill_video(&frame, &mut signal);
                        out.video = signal.data;
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
                out.picture = !out.video.is_empty();
            }
            Reader::Audio(reader) => {
                out.video.clear();
                out.audio.resize(reader.layout.len(), 0.0);
                reader
                    .modulator
                    .fill_items(&reader.items, start, end, &mut out.audio);
            }
        }
        Ok(())
    }
}

/// Renders output frames of the project's tracks through the routing: each track's items and
/// FX, the folders' mixes and FX, and the master's (see [Routing](../../../docs/engine.md#routing)).
///
/// The graphs are stateful, so frames are produced by processing source frames in order. Asking
/// for the next frame is the fast path. Asking for any other frame resets the graphs and first
/// renders (and discards) [`Self::warmup_frames`] frames so stateful nodes have history. That
/// makes the result exact for nodes with finite memory and a close approximation for
/// infinite-memory ones (feedback, IIR filters). Rendering from frame 0 is always exact.
pub struct Renderer {
    tracks: Tracks,
    route: Route,
    /// What graphs are compiled against when nothing places the open graph: the main ports and
    /// each track by name.
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
    /// The bus rendered: only the Audio Outputs writing to it are compiled, and its sound has
    /// the bus's channels.
    bus: Bus,
    audio_rate: u32,
    /// What turns the master's sound into audio at the project's rate, when FX render sound.
    resampler: Option<SinkResampler>,
    /// The audio of the last rendered frame, when FX render sound.
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

/// The routing of a render of one graph: every track at the top, and `graph` (id 0) as the
/// master's FX, its ports named after tracks filled by receives from them.
pub fn single_routing(tracks: &[RenderTrack], graph: &GraphDesc) -> Routing {
    let receives = input_ports(graph)
        .into_iter()
        .filter(|p| tracks.iter().any(|t| t.name == p.name))
        .map(|p| (p.name.clone(), p.name))
        .collect();
    Routing {
        tracks: tracks
            .iter()
            .map(|t| RouteTrack {
                muted: !t.in_mix,
                ..RouteTrack::new(
                    t.name.clone(),
                    Some(match t.media {
                        TrackMedia::Video { .. } => TrackKind::Video,
                        TrackMedia::Audio(_) => TrackKind::Audio,
                    }),
                )
            })
            .collect(),
        master_fx: vec![Fx {
            receives,
            ..Fx::new(0)
        }],
        graphs: Vec::new(),
        open: 0,
    }
}

impl Renderer {
    /// Opens the video tracks at `size` and renders `graph` over the project's `tracks` on its
    /// `timebase`, with the sound of output bus `bus`: the graph is the master's FX, and its
    /// ports named after tracks read those tracks (see [`single_routing`]). Without a timebase
    /// the first video track sets it, or [`Timebase::DEFAULT`] when there is none. Ports that
    /// name no track read zeros.
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
        let routing = single_routing(tracks, graph);
        Self::build(
            backend, timebase, tracks, &routing, graph, true, tempo, bus, registry, size,
        )
    }

    /// [`Self::new`] for a project's routing: `tracks` are the tracks that play media, by the
    /// names `routing` gives them, and `open` is the open graph's description, which the
    /// routing leaves out.
    #[allow(clippy::too_many_arguments)]
    pub fn with_routing(
        backend: &dyn MediaBackend,
        timebase: Option<Timebase>,
        tracks: &[RenderTrack],
        routing: &Routing,
        open: &GraphDesc,
        tempo: Tempo,
        bus: &Bus,
        registry: &Registry,
        size: OutputSize,
    ) -> Result<Self, EngineError> {
        Self::build(
            backend, timebase, tracks, routing, open, false, tempo, bus, registry, size,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        backend: &dyn MediaBackend,
        timebase: Option<Timebase>,
        tracks: &[RenderTrack],
        routing: &Routing,
        open: &GraphDesc,
        single: bool,
        tempo: Tempo,
        bus: &Bus,
        registry: &Registry,
        size: OutputSize,
    ) -> Result<Self, EngineError> {
        // The readers, in the order of `tracks`, with each one's length.
        let mut readers = Vec::new();
        let mut durations = Vec::new();
        let mut first_video = None;
        for track in tracks {
            match &track.media {
                TrackMedia::Video { path, stream } => {
                    let video = backend.open_video_stream(path, *stream)?;
                    let info = video.info();
                    let fps = info.frame_rate.as_f64();
                    let duration = info.frame_count as f64 / fps;
                    first_video.get_or_insert(Timebase {
                        width: info.width,
                        height: info.height,
                        frame_rate: info.frame_rate,
                    });
                    durations.push(duration);
                    readers.push(Reader::Video(VideoReader {
                        duration,
                        fps,
                        video,
                        items: track.items.clone(),
                        have_frame: false,
                    }));
                }
                TrackMedia::Audio(modulator) => {
                    durations.push(modulator.duration_secs());
                    readers.push(Reader::Audio(AudioReader {
                        modulator: modulator.clone(),
                        items: track.items.clone(),
                        layout: Layout::EMPTY,
                    }));
                }
            }
        }
        let timebase = timebase.or(first_video).unwrap_or(Timebase::DEFAULT);
        if !timebase.is_valid() {
            return Err(EngineError::Timebase(timebase));
        }
        let fps = timebase.fps();
        let (width, height) = size.resolve(timebase.width, timebase.height);
        for reader in &mut readers {
            match reader {
                Reader::Video(v) => {
                    let info = v.video.info();
                    let native = (info.width, info.height) == (width, height);
                    v.video
                        .set_output_size((!native).then_some((width, height)));
                }
                Reader::Audio(a) => a.layout = a.modulator.layout(fps),
            }
        }
        let end = tracks
            .iter()
            .zip(&durations)
            .map(|(t, &d)| items_end(&t.items, d))
            .fold(0.0, f64::max);

        let video_layout = Layout::video(width, height);
        let bus = bus.clone().sanitized();
        // What a graph placed nowhere is inspected against: the main ports and every track by
        // name.
        let mut sources = HashMap::new();
        sources.insert(DEFAULT_VIDEO.to_owned(), video_layout);
        sources.insert(DEFAULT_AUDIO.to_owned(), mix_layout(bus.channels, fps));
        for (track, reader) in tracks.iter().zip(&readers) {
            let layout = match reader {
                Reader::Video(_) => video_layout,
                Reader::Audio(a) => a.layout,
            };
            sources.entry(track.name.clone()).or_insert(layout);
        }
        let options = CompileOptions {
            frame_rate: fps,
            tempo,
            sources,
            output: video_layout,
            pixel_scale: f64::from(width) / f64::from(timebase.width),
            audio_bus: bus.name.clone(),
        };
        let media = routing
            .tracks
            .iter()
            .map(|t| {
                let i = tracks.iter().position(|r| r.name == t.name)?;
                Some(MediaInfo {
                    reader: i,
                    audio: match &readers[i] {
                        Reader::Audio(a) => Some(a.layout),
                        Reader::Video(_) => None,
                    },
                    duration: durations[i],
                })
            })
            .collect();
        let items = routing
            .tracks
            .iter()
            .map(|t| {
                tracks
                    .iter()
                    .find(|r| r.name == t.name)
                    .map(|r| r.items.clone())
                    .unwrap_or_default()
            })
            .collect();
        let route = Route::build(RoutePlan {
            routing,
            open,
            media,
            items,
            registry,
            options: &options,
            bus: &bus,
            fps,
            single,
        })?;

        let max_warmup = DEFAULT_MAX_WARMUP_FRAMES as usize;
        let mut renderer = Self {
            tracks: Tracks {
                readers,
                fps,
                video_layout,
            },
            info: RenderInfo {
                width,
                height,
                frame_rate: timebase.frame_rate,
                frames: timebase.frames_in(end),
                timebase,
            },
            fps,
            options,
            latency: route.latency(),
            warmup: 0,
            max_warmup,
            route,
            next_source: None,
            rgb: Vec::with_capacity(video_layout.len()),
            bus,
            audio_rate: DEFAULT_AUDIO_RATE,
            resampler: None,
            audio: None,
        };
        renderer.set_audio_rate(DEFAULT_AUDIO_RATE);
        Ok(renderer)
    }

    /// Whether the render's sound is rendered here: some FX renders sound, and it isn't just a
    /// track passed through.
    fn renders_audio(&self) -> bool {
        self.route.renders_audio() && self.route.passthrough().is_none()
    }

    fn update_warmup(&mut self) {
        let warm = self.route.warmup(self.max_warmup);
        self.warmup = if self.renders_audio() {
            warm.max(1)
        } else {
            warm
        };
    }

    /// Limits the frames pre-rendered before a seek to `frames`. Rendering restarts from the
    /// next request.
    pub fn set_max_warmup_frames(&mut self, frames: u32) {
        self.max_warmup = frames.min(MAX_WARMUP_FRAMES_LIMIT) as usize;
        self.update_warmup();
        self.next_source = None;
    }

    /// Renders the sound at `rate` samples a second (48 kHz unless set).
    pub fn set_audio_rate(&mut self, rate: u32) {
        self.audio_rate = rate.max(1);
        let layout = self.route.audio_layout();
        self.resampler = self.renders_audio().then(|| {
            SinkResampler::new(
                layout.len(),
                layout.samples_per_pixel,
                self.bus.channels,
                self.fps,
                self.audio_rate,
            )
        });
        self.update_warmup();
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
        match self.route.passthrough() {
            Some(track) if self.route.renders_audio() => AudioSink::Passthrough(track.to_owned()),
            _ if self.renders_audio() => AudioSink::Rendered {
                sample_rate: self.audio_rate,
                channels: self.bus.channels,
            },
            _ => AudioSink::TrackMix,
        }
    }

    /// The rendered audio of the frame [`Self::render`] last returned, when FX render sound
    /// ([`AudioSink::Rendered`]).
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
    /// This reads the FX that ran in the last processed source frame, those playing the open
    /// graph first, the master's before the tracks'.
    pub fn tap(&self, node: &str, output: usize) -> Option<&Signal> {
        self.route.tap(node, output)
    }

    /// The level of every node output in the last rendered frame: of the first FX that ran
    /// playing the open graph, else the first FX that ran; likewise for the costs, meters and
    /// parameter levels below.
    pub fn levels(&self) -> Vec<OutputLevel> {
        self.route.levels()
    }

    /// How long every node took to process the last rendered frame.
    pub fn costs(&self) -> Vec<rastersong_graph::NodeCost> {
        self.route.costs()
    }

    /// The meter values of the nodes that publish them, from the last rendered frame.
    pub fn meters(&self) -> Vec<rastersong_graph::NodeMeters> {
        self.route.meters()
    }

    /// The value of every modulated parameter in the last rendered frame.
    pub fn param_levels(&self) -> Vec<rastersong_graph::ParamLevel> {
        self.route.param_levels()
    }

    pub fn info(&self) -> &RenderInfo {
        &self.info
    }

    /// The sources, frame rate, tempo and output size the open graph was compiled against: as
    /// its first FX, else as the master's.
    pub fn compile_options(&self) -> &CompileOptions {
        self.route.compile_options().unwrap_or(&self.options)
    }

    /// Each node's own latency and warmup: the open graph's first FX's, else the first FX's.
    pub fn node_stats(&self) -> &[rastersong_graph::NodeStats] {
        self.route.node_stats()
    }

    /// Frames between a source frame going in and its result coming out: the longest path of
    /// FX latencies to the master.
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
            self.route.reset();
            if let Some(resampler) = &mut self.resampler {
                resampler.reset();
            }
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

    /// Feeds source frame `m` (and its audio) through the routing and keeps the output.
    fn process(&mut self, m: usize) -> Result<(), EngineError> {
        if let Err(e) = self.route.step(m, &mut self.tracks, self.max_warmup) {
            self.next_source = None;
            return Err(e);
        }
        let out = self.route.output();
        if out.video.is_empty() {
            self.rgb.clear();
            self.rgb.resize(self.tracks.video_layout.len(), 0);
        } else {
            to_rgb8(&out.video, &mut self.rgb);
        }
        // Like the picture, the sound comes out `latency` frames after its source.
        if let Some(resampler) = &mut self.resampler {
            let frame = m as i64 - self.latency as i64;
            let layout = self.route.audio_layout();
            let block = if out.audio.len() == layout.len() {
                resampler.push(frame, &out.audio)
            } else {
                resampler.push(frame, &vec![0.0; layout.len()])
            };
            self.audio = Some(block);
        }
        Ok(())
    }
}
