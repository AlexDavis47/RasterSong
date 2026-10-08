//! The sequential renderer shared by preview and offline rendering.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rastersong_graph::nodes::{
    AUDIO_INPUT, DEFAULT_AUDIO, DEFAULT_VIDEO, SOURCE_PARAM, VIDEO_INPUT,
};
use rastersong_graph::{
    CompileOptions, Graph, GraphDesc, Layout, OutputLevel, ParamValue, Registry, Signal, Tempo,
};
use rastersong_media::{MediaBackend, MediaError, Rational, VideoSource};

use crate::EngineError;
use crate::audio::{AudioBlock, AudioSink, DEFAULT_AUDIO_RATE, SinkResampler};
use crate::project::{DEFAULT_MAX_WARMUP_FRAMES, MAX_WARMUP_FRAMES_LIMIT};
use crate::sources::{Modulator, fill_video, to_rgb8};
use crate::timeline::{Item, Timebase, item_at, items_end};

/// The source a video input reads by default.
pub const VIDEO_SOURCE: &str = DEFAULT_VIDEO;
/// The node type that reads audio tracks, and the track it reads by default.
pub const DEFAULT_AUDIO_TRACK: &str = DEFAULT_AUDIO;

/// What a track plays.
#[derive(Debug, Clone)]
pub enum TrackMedia {
    /// A video file, opened by the renderer.
    Video(PathBuf),
    /// Decoded audio.
    Audio(Arc<Modulator>),
}

/// A track the graph's input nodes can read, by name: its media, placed by its items.
#[derive(Debug, Clone)]
pub struct RenderTrack {
    pub name: String,
    pub media: TrackMedia,
    pub items: Vec<Item>,
}

impl RenderTrack {
    /// An audio track playing `modulator` from `position` seconds, whole.
    pub fn audio(name: impl Into<String>, modulator: Arc<Modulator>, position: f64) -> Self {
        Self {
            name: name.into(),
            media: TrackMedia::Audio(modulator),
            items: vec![Item::whole(position)],
        }
    }

    /// A video track playing the file at `path` from the start, whole.
    pub fn video(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            media: TrackMedia::Video(path.into()),
            items: vec![Item::whole(0.0)],
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

/// Renders output frames of the project's tracks through one compiled graph.
///
/// The graph is stateful, so frames are produced by processing source frames in order. Asking for
/// the next frame is the fast path. Asking for any other frame resets the graph and first renders
/// (and discards) [`Graph::warmup_frames`] frames so stateful nodes have history. That makes the
/// result exact for nodes with finite memory and a close approximation for infinite-memory ones
/// (feedback, IIR filters). Rendering from frame 0 is always exact.
pub struct Renderer {
    videos: Vec<VideoReader>,
    tracks: Vec<AudioReader>,
    graph: Graph,
    /// What the graph was compiled against, so editors can inspect other graphs the same way.
    options: CompileOptions,
    info: RenderInfo,
    fps: f64,
    latency: usize,
    warmup: usize,
    /// The most frames to pre-render before a seek, whatever the graph asks for.
    max_warmup: usize,
    sources: HashMap<String, Signal>,
    /// The next source frame to process, if the graph's state is positioned somewhere.
    next_source: Option<usize>,
    rgb: Vec<u8>,
    /// Turns the audio output into audio at the project rate, when the graph renders sound.
    resampler: Option<SinkResampler>,
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

impl Renderer {
    /// Opens the video tracks at `size` and compiles `graph` for the project's `tracks` on its
    /// `timebase`. Without a timebase the first video track sets it, or [`Timebase::DEFAULT`]
    /// when there is none. Input nodes that name a track that isn't there read zeros.
    pub fn new(
        backend: &dyn MediaBackend,
        timebase: Option<Timebase>,
        tracks: &[RenderTrack],
        graph: &GraphDesc,
        tempo: Tempo,
        registry: &Registry,
        size: OutputSize,
    ) -> Result<Self, EngineError> {
        let mut videos = Vec::new();
        let mut audio = Vec::new();
        for track in tracks {
            match &track.media {
                TrackMedia::Video(path) => {
                    let video = backend.open_video(path)?;
                    let info = video.info();
                    let fps = info.frame_rate.as_f64();
                    videos.push(VideoReader {
                        name: track.name.clone(),
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

        for name in source_names(graph, AUDIO_INPUT, DEFAULT_AUDIO) {
            if !audio.iter().any(|t| t.name == name) && !videos.iter().any(|v| v.name == name) {
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
        for name in source_names(graph, VIDEO_INPUT, DEFAULT_VIDEO) {
            layouts.entry(name).or_insert(video_layout);
        }
        // A generator set to the audio layout needs one even when no track has the default
        // name: the first track's shape, or silence at the default rate when there is none.
        if !layouts.contains_key(DEFAULT_AUDIO) {
            let layout = audio.first().map_or_else(
                || Modulator::silent().layout(fps),
                |track| track.modulator.layout(fps),
            );
            layouts.insert(DEFAULT_AUDIO.to_owned(), layout);
        }
        let options = CompileOptions {
            frame_rate: fps,
            tempo,
            sources: layouts.clone(),
            output: video_layout,
            pixel_scale: f64::from(width) / f64::from(timebase.width),
        };
        let graph = Graph::compile(graph, registry, &options)?;
        let resampler = Self::resampler(&graph, fps, DEFAULT_AUDIO_RATE);

        Ok(Self {
            videos,
            tracks: audio,
            info: RenderInfo {
                width,
                height,
                frame_rate: timebase.frame_rate,
                frames: timebase.frames_in(end),
                timebase,
            },
            fps,
            options,
            latency: graph.latency_frames() as usize,
            warmup: Self::warmup(
                &graph,
                resampler.is_some(),
                DEFAULT_MAX_WARMUP_FRAMES as usize,
            ),
            max_warmup: DEFAULT_MAX_WARMUP_FRAMES as usize,
            graph,
            sources: layouts
                .into_iter()
                .map(|(name, layout)| (name, Signal::zeros(layout)))
                .collect(),
            next_source: None,
            rgb: Vec::with_capacity(video_layout.len()),
            resampler,
            audio: None,
        })
    }

    /// A resampler for the graph's audio output, unless there is none or it just passes a track
    /// through.
    fn resampler(graph: &Graph, fps: f64, rate: u32) -> Option<SinkResampler> {
        let layout = graph.audio_layout()?;
        if graph.audio_passthrough().is_some() {
            return None;
        }
        Some(SinkResampler::new(
            layout.len(),
            layout.samples_per_pixel,
            fps,
            rate,
        ))
    }

    /// Frames to render before a seek: the graph's, limited to `cap`, and at least one when
    /// rendering sound, so the resampler has history and a seek gives the same audio as playing
    /// through. The cap only limits this pre-render; nodes keep their real memory.
    fn warmup(graph: &Graph, resampling: bool, cap: usize) -> usize {
        let warmup = (graph.warmup_frames() as usize).min(cap);
        if resampling { warmup.max(1) } else { warmup }
    }

    /// Limits the frames pre-rendered before a seek to `frames`. Rendering restarts from the
    /// next request.
    pub fn set_max_warmup_frames(&mut self, frames: u32) {
        self.max_warmup = frames.min(MAX_WARMUP_FRAMES_LIMIT) as usize;
        self.warmup = Self::warmup(&self.graph, self.resampler.is_some(), self.max_warmup);
        self.next_source = None;
    }

    /// Renders the audio output at `rate` samples a second (48 kHz unless set).
    pub fn set_audio_rate(&mut self, rate: u32) {
        self.resampler = Self::resampler(&self.graph, self.fps, rate.max(1));
        self.warmup = Self::warmup(&self.graph, self.resampler.is_some(), self.max_warmup);
        self.audio = None;
        self.next_source = None;
    }

    /// What the render's audio is: the source's, a track passed through, or rendered sound.
    pub fn audio_sink(&self) -> AudioSink {
        match (&self.resampler, self.graph.audio_passthrough()) {
            (Some(r), _) => AudioSink::Rendered {
                sample_rate: r.sample_rate(),
                channels: r.channels(),
            },
            (None, Some(track)) => AudioSink::Passthrough(track.to_owned()),
            (None, None) => AudioSink::Source,
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
    /// `None` if the graph has no such output (the node doesn't feed the graph's output).
    /// Read-only: nothing about the render changes.
    pub fn tap(&self, node: &str, output: usize) -> Option<&Signal> {
        self.graph.tap(node, output)
    }

    /// The level of every node output in the last rendered frame.
    pub fn levels(&self) -> Vec<OutputLevel> {
        self.graph.levels()
    }

    /// How long every node took to process the last rendered frame.
    pub fn costs(&self) -> Vec<rastersong_graph::NodeCost> {
        self.graph.costs()
    }

    /// The meter values of the nodes that publish them, from the last rendered frame.
    pub fn meters(&self) -> Vec<rastersong_graph::NodeMeters> {
        self.graph.meters()
    }

    /// The value of every modulated parameter in the last rendered frame.
    pub fn param_levels(&self) -> Vec<rastersong_graph::ParamLevel> {
        self.graph.param_levels()
    }

    pub fn info(&self) -> &RenderInfo {
        &self.info
    }

    /// The sources, frame rate, tempo and output size the graph was compiled against.
    pub fn compile_options(&self) -> &CompileOptions {
        &self.options
    }

    /// Each node's own latency and warmup.
    pub fn node_stats(&self) -> &[rastersong_graph::NodeStats] {
        self.graph.node_stats()
    }

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
            self.graph.reset();
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

    /// Feeds source frame `m` (and its audio) through the graph and keeps the output.
    fn process(&mut self, m: usize) -> Result<(), EngineError> {
        let (start, end) = (m as f64 / self.fps, (m + 1) as f64 / self.fps);
        for reader in &mut self.videos {
            let out = self.sources.get_mut(&reader.name).unwrap();
            // A hair past the frame's start, so an item starting on it is found despite rounding.
            let Some((_, at)) = item_at(&reader.items, start + 1e-9, reader.duration) else {
                out.data.fill(0.0);
                continue;
            };
            let index = reader.frame_at(at);
            match reader.video.frame(index) {
                Ok(frame) => {
                    fill_video(&frame, out);
                    reader.have_frame = true;
                }
                // A damaged frame repeats the previous one rather than failing the render.
                Err(MediaError::FrameUnavailable(i)) if reader.have_frame => {
                    tracing::warn!(
                        frame = i,
                        "frame could not be decoded; repeating the previous frame"
                    );
                }
                Err(e) => {
                    self.next_source = None;
                    return Err(e.into());
                }
            }
        }
        for track in &self.tracks {
            let block = &mut self.sources.get_mut(&track.name).unwrap().data;
            track.modulator.fill_items(&track.items, start, end, block);
        }

        match self.graph.process(m as u64, &self.sources) {
            Ok(output) => {
                to_rgb8(output, &mut self.rgb);
                // Like the picture, the sound comes out `latency` frames after its source.
                if let (Some(resampler), Some(sound)) =
                    (&mut self.resampler, self.graph.audio_output())
                {
                    let frame = m as i64 - self.latency as i64;
                    self.audio = Some(resampler.push(frame, &sound.data));
                }
                Ok(())
            }
            Err(e) => {
                self.next_source = None;
                Err(e.into())
            }
        }
    }
}

/// The track names read by the graph's input nodes of type `kind`, which read `default` unless
/// set otherwise.
fn source_names(graph: &GraphDesc, kind: &str, default: &str) -> Vec<String> {
    graph
        .nodes
        .iter()
        .filter(|n| n.kind == kind)
        .map(|n| match n.params.get(SOURCE_PARAM) {
            Some(ParamValue::Text(name)) => name.clone(),
            _ => default.to_owned(),
        })
        .collect()
}
