//! The sequential renderer shared by preview and offline rendering.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use rastersong_graph::nodes::{AUDIO_INPUT, DEFAULT_AUDIO, DEFAULT_VIDEO, SOURCE_PARAM};
use rastersong_graph::{
    CompileOptions, Graph, GraphDesc, Layout, OutputLevel, ParamValue, Registry, Signal, Tempo,
};
use rastersong_media::{MediaBackend, MediaError, Rational, VideoSource};

use crate::EngineError;
use crate::audio::{AudioBlock, AudioSink, DEFAULT_AUDIO_RATE, SinkResampler};
use crate::project::{DEFAULT_MAX_WARMUP_FRAMES, MAX_WARMUP_FRAMES_LIMIT};
use crate::sources::{Modulator, fill_video, to_rgb8};

/// The source name of the video.
pub const VIDEO_SOURCE: &str = DEFAULT_VIDEO;
/// The node type that reads audio tracks, and the track it reads by default.
pub const DEFAULT_AUDIO_TRACK: &str = DEFAULT_AUDIO;

/// An audio track the graph's audio inputs can read, by name.
#[derive(Debug, Clone)]
pub struct AudioTrack {
    pub name: String,
    pub modulator: Arc<Modulator>,
    /// Seconds the track starts after the video (before it, if negative).
    pub offset: f64,
}

/// The size to render at.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum OutputSize {
    /// The video's display size.
    #[default]
    Native,
    Exact(u32, u32),
    /// A fraction of the display size, e.g. 0.5 for a half-resolution preview.
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
    pub frames: usize,
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

/// Renders output frames of one video through one compiled graph.
///
/// The graph is stateful, so frames are produced by processing source frames in order. Asking for
/// the next frame is the fast path. Asking for any other frame resets the graph and first renders
/// (and discards) [`Graph::warmup_frames`] frames so stateful nodes have history. That makes the
/// result exact for nodes with finite memory and a close approximation for infinite-memory ones
/// (feedback, IIR filters). Rendering from frame 0 is always exact.
pub struct Renderer {
    video: Box<dyn VideoSource>,
    tracks: Vec<AudioTrack>,
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
    have_video: bool,
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
    /// Opens `video_path` at `size` and compiles `graph` for it, with `tracks` as the audio.
    /// Audio inputs that name a track that isn't there get silence.
    pub fn new(
        backend: &dyn MediaBackend,
        video_path: &Path,
        tracks: &[AudioTrack],
        graph: &GraphDesc,
        tempo: Tempo,
        registry: &Registry,
        size: OutputSize,
    ) -> Result<Self, EngineError> {
        let mut video = backend.open_video(video_path)?;
        let source = video.info().clone();
        let (width, height) = size.resolve(source.width, source.height);
        if size != OutputSize::Native {
            video.set_output_size(Some((width, height)));
        }
        let fps = source.frame_rate.as_f64();

        let mut tracks = tracks.to_vec();
        for name in audio_inputs(graph) {
            if !tracks.iter().any(|t| t.name == name) {
                tracks.push(AudioTrack {
                    name,
                    modulator: Arc::new(Modulator::silent()),
                    offset: 0.0,
                });
            }
        }

        let video_layout = Layout::video(width, height);
        let mut layouts = HashMap::from([(VIDEO_SOURCE.to_owned(), video_layout)]);
        for track in &tracks {
            layouts.insert(track.name.clone(), track.modulator.layout(fps));
        }
        // A generator set to the audio layout needs one even when no track has the default
        // name: the first track's shape, or silence at the default rate when there is none.
        if !layouts.contains_key(DEFAULT_AUDIO) {
            let layout = tracks.first().map_or_else(
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
            pixel_scale: f64::from(width) / f64::from(source.width.max(1)),
        };
        let graph = Graph::compile(graph, registry, &options)?;
        let resampler = Self::resampler(&graph, fps, DEFAULT_AUDIO_RATE);

        Ok(Self {
            video,
            tracks,
            info: RenderInfo {
                width,
                height,
                frame_rate: source.frame_rate,
                frames: source.frame_count,
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
            have_video: false,
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
        // Past the end (latency pre-roll) the last frame repeats.
        let last = self.info.frames - 1;
        match self.video.frame(m.min(last)) {
            Ok(frame) => {
                fill_video(&frame, self.sources.get_mut(VIDEO_SOURCE).unwrap());
                self.have_video = true;
            }
            // A damaged frame repeats the previous one rather than failing the render.
            Err(MediaError::FrameUnavailable(i)) if self.have_video => {
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
        let (start, end) = (self.frame_time(m), self.frame_time(m + 1));
        for track in &self.tracks {
            let block = &mut self.sources.get_mut(&track.name).unwrap().data;
            track
                .modulator
                .fill_block(start - track.offset, end - track.offset, block);
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

    /// Start time of source frame `m`. Past the end, frames continue at the nominal rate.
    fn frame_time(&self, m: usize) -> f64 {
        let last = self.info.frames - 1;
        let time = |n: usize| snap_to_grid(self.video.frame_time(n), n, self.fps);
        if m <= last {
            time(m)
        } else {
            time(last) + (m - last) as f64 / self.fps
        }
    }
}

/// Track names read by the graph's audio inputs.
fn audio_inputs(graph: &GraphDesc) -> Vec<String> {
    graph
        .nodes
        .iter()
        .filter(|n| n.kind == AUDIO_INPUT)
        .map(|n| match n.params.get(SOURCE_PARAM) {
            Some(ParamValue::Text(name)) => name.clone(),
            _ => DEFAULT_AUDIO_TRACK.to_owned(),
        })
        .collect()
}

/// How far a timestamp may be from the nominal frame grid and still be treated as on it.
/// Containers round timestamps (Matroska to whole milliseconds), which would otherwise make each
/// frame's audio span jitter between e.g. 33 and 34 ms and wobble the modulation.
const GRID_TOLERANCE_SECS: f64 = 0.001;

/// Snaps frame `n`'s timestamp to `n / fps` when it is within container rounding of it. Frames of
/// genuinely variable-frame-rate video are far off the grid and keep their real time.
fn snap_to_grid(time: f64, n: usize, fps: f64) -> f64 {
    let nominal = n as f64 / fps;
    if (time - nominal).abs() <= GRID_TOLERANCE_SECS {
        nominal
    } else {
        time
    }
}

#[cfg(test)]
mod tests {
    use super::snap_to_grid;

    #[test]
    fn rounded_timestamps_snap_to_the_frame_grid() {
        // Matroska stores frame 46 at 30 fps as 1.533 s instead of 1.5333… s.
        assert_eq!(snap_to_grid(1.533, 46, 30.0), 46.0 / 30.0);
        assert_eq!(snap_to_grid(0.0, 0, 30.0), 0.0);
        // A variable-frame-rate frame well off the grid keeps its time.
        assert_eq!(snap_to_grid(1.0667, 31, 30.0), 1.0667);
    }
}
