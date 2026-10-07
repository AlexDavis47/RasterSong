//! The preview render service: a background thread that keeps rendering ahead of the playhead
//! into the frame cache, whether or not playback is running.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use rastersong_graph::{CompileOptions, GraphDesc, NodeStats, Registry, Tempo, render_form};
use rastersong_lang::{tr, tr_args};
use rastersong_media::{AudioClip, AudioOptions, MediaBackend};

use crate::audio::{AudioBlock, AudioSink, DEFAULT_AUDIO_RATE, SinkResampler};
use crate::cache::{CacheKey, Frame, FrameCache};
use crate::listen::{self, ListenTarget, Listened};
use crate::playback::RenderedSource;
use crate::project::{DEFAULT_MAX_WARMUP_FRAMES, MAX_WARMUP_FRAMES_LIMIT};
use crate::renderer;
use crate::sources::Modulator;
use crate::tap::{self, TapOutcome, TapRequest};
use crate::waveform::Waveform;
use crate::{AudioTrack, EngineError, OutputSize, RenderInfo, Renderer};

/// Preview resolution. Processing cost scales with pixel count, so a quarter-scale preview is
/// about 16× cheaper. Because parameters are in normalized units, it looks like a scaled-down
/// version of the full render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PreviewScale {
    #[default]
    Full,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
}

impl PreviewScale {
    pub const ALL: [PreviewScale; 5] = [
        Self::Full,
        Self::Half,
        Self::Quarter,
        Self::Eighth,
        Self::Sixteenth,
    ];

    /// How many times smaller than full resolution each side is.
    pub fn divisor(self) -> u32 {
        match self {
            Self::Full => 1,
            Self::Half => 2,
            Self::Quarter => 4,
            Self::Eighth => 8,
            Self::Sixteenth => 16,
        }
    }

    /// The name shown for this scale: Full, Half, Quarter, Eighth or Sixteenth.
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => tr("preview_scale.full"),
            Self::Half => tr("preview_scale.half"),
            Self::Quarter => tr("preview_scale.quarter"),
            Self::Eighth => tr("preview_scale.eighth"),
            Self::Sixteenth => tr("preview_scale.sixteenth"),
        }
    }

    /// The scale with the given divisor, if there is one.
    pub fn from_divisor(divisor: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.divisor() == divisor)
    }

    pub fn output_size(self) -> OutputSize {
        match self {
            Self::Full => OutputSize::Native,
            scaled => OutputSize::Scaled(1.0 / scaled.divisor() as f32),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Memory for rendered frames.
    pub cache_bytes: usize,
    /// How far ahead of the playhead to render, memory permitting.
    pub lookahead_secs: f64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            cache_bytes: 1 << 30,
            lookahead_secs: 10.0,
        }
    }
}

/// An audio track as the project describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioTrackSpec {
    /// The name audio input nodes select it by.
    pub name: String,
    pub path: PathBuf,
    /// Seconds the track starts after the video (before it, if negative).
    pub offset: f64,
}

/// A decoded audio track, for playback and display.
#[derive(Debug, Clone)]
pub struct LoadedTrack {
    pub name: String,
    pub clip: Arc<AudioClip>,
    /// Peaks for drawing the track.
    pub waveform: Arc<Waveform>,
}

/// Why the project can't be rendered as it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub message: String,
    /// The node at fault, when the problem is with one node of the graph.
    pub node: Option<String>,
    /// The message without the "node `id`:" lead-in, for showing under the node's name.
    pub detail: String,
}

impl Failure {
    fn new(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            detail: message.clone(),
            message,
            node: None,
        }
    }

    fn from_error(error: &EngineError) -> Self {
        Self {
            message: error.to_string(),
            detail: match error {
                EngineError::Graph(e) => e.detail(),
                other => other.to_string(),
            },
            node: match error {
                EngineError::Graph(e) => e.node().map(str::to_owned),
                _ => None,
            },
        }
    }
}

/// What the render thread is working on right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderProgress {
    /// The frame being rendered.
    pub frame: usize,
    /// Source frames processed so far for it, and how many it takes.
    pub done: usize,
    pub total: usize,
    /// Whether this is a fresh start (after a seek or an edit) that has to process history first,
    /// rather than the next frame in a run.
    pub warming: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineStatus {
    /// No video or graph yet.
    Idle,
    /// Opening media and compiling the graph.
    Loading,
    Ready,
    /// Cleared by the next change.
    Failed(Failure),
}

type Callback = Arc<dyn Fn() + Send + Sync>;

/// The render service. Cheap calls that only record what the user wants; all decoding and
/// rendering happens on a background thread.
pub struct Engine {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

struct Shared {
    backend: Arc<dyn MediaBackend>,
    config: Mutex<EngineConfig>,
    state: Mutex<State>,
    /// Signalled on every change to `state`.
    changed: Condvar,
    cache: Mutex<FrameCache>,
    /// Copies of state the worker checks between frames, without locking.
    edits: AtomicU64,
    playhead: AtomicUsize,
    on_update: Mutex<Option<Callback>>,
    progress: Mutex<Option<RenderProgress>>,
    /// Bumped whenever a different tap is asked for, so the worker drops one that is stale.
    tap_serial: AtomicU64,
    /// The answer to the last tap the worker finished.
    tap_done: Mutex<Option<TapDone>>,
    /// The sound rendered for the connection being listened to.
    listened: Mutex<Listened>,
    /// The frame listening is at, so the worker renders sound ahead of it.
    listen_frame: AtomicUsize,
    /// Bumped whenever the connection listened to changes.
    listen_serial: AtomicU64,
}

/// A tap's answer and the project version it was made for.
struct TapDone {
    request: TapRequest,
    key: CacheKey,
    outcome: TapOutcome,
}

struct State {
    video: Option<PathBuf>,
    tracks: Vec<AudioTrackSpec>,
    graph: Option<GraphDesc>,
    /// Skips the whole graph: the video goes straight to the output.
    bypass_all: bool,
    tempo: Tempo,
    key: CacheKey,
    playhead: usize,
    /// Frames playback repeats, so rendering ahead wraps round them too.
    looping: Option<Range<usize>>,
    status: EngineStatus,
    info: Option<RenderInfo>,
    loaded: Vec<LoadedTrack>,
    /// What each node of the current graph costs, once it has compiled.
    node_stats: Vec<NodeStats>,
    /// What the last graph that compiled was compiled against.
    compile_options: Option<CompileOptions>,
    /// The rate the graph's sound is rendered at.
    audio_rate: u32,
    /// The most frames pre-rendered after a seek.
    max_warmup_frames: u32,
    /// What the current render's audio is, once it has compiled.
    audio_sink: AudioSink,
    /// The tap waiting for the worker, if any. Only the latest is kept.
    tap: Option<TapRequest>,
    /// The connection being listened to, if any.
    listen: Option<ListenTarget>,
    /// Bumped on every change, so the worker knows when to look again.
    changes: u64,
    shutdown: bool,
}

/// The parts of the project a renderer is built from.
struct Snapshot {
    video: PathBuf,
    tracks: Vec<AudioTrackSpec>,
    graph: GraphDesc,
    tempo: Tempo,
    audio_rate: u32,
    max_warmup_frames: u32,
}

/// A renderer built for one version of the project.
struct Built {
    key: CacheKey,
    renderer: Renderer,
}

impl Engine {
    pub fn new(backend: Arc<dyn MediaBackend>, config: EngineConfig) -> Self {
        let key = CacheKey {
            version: 0,
            scale: PreviewScale::default(),
        };
        let shared = Arc::new(Shared {
            backend,
            cache: Mutex::new(FrameCache::new(key, config.cache_bytes)),
            config: Mutex::new(config),
            state: Mutex::new(State {
                video: None,
                tracks: Vec::new(),
                graph: None,
                bypass_all: false,
                tempo: Tempo::default(),
                key,
                playhead: 0,
                looping: None,
                status: EngineStatus::Idle,
                info: None,
                loaded: Vec::new(),
                node_stats: Vec::new(),
                compile_options: None,
                audio_rate: DEFAULT_AUDIO_RATE,
                max_warmup_frames: DEFAULT_MAX_WARMUP_FRAMES,
                audio_sink: AudioSink::Source,
                tap: None,
                listen: None,
                changes: 0,
                shutdown: false,
            }),
            changed: Condvar::new(),
            edits: AtomicU64::new(0),
            playhead: AtomicUsize::new(0),
            on_update: Mutex::new(None),
            progress: Mutex::new(None),
            tap_serial: AtomicU64::new(0),
            tap_done: Mutex::new(None),
            listened: Mutex::new(Listened::default()),
            listen_frame: AtomicUsize::new(0),
            listen_serial: AtomicU64::new(0),
        });
        let worker = std::thread::Builder::new()
            .name("rastersong-render".into())
            .spawn({
                let shared = shared.clone();
                move || Worker::new(shared).run()
            })
            .expect("failed to start the render thread");
        Self {
            shared,
            worker: Some(worker),
        }
    }

    /// Changes the cache budget and lookahead while running. Rendered frames are kept; a smaller
    /// budget drops the frames farthest from the playhead.
    pub fn set_config(&self, config: EngineConfig) {
        let playhead = self.shared.playhead.load(Ordering::SeqCst);
        lock(&self.shared.cache).set_budget(config.cache_bytes, playhead);
        *lock(&self.shared.config) = config;
        self.shared.changed.notify_all();
    }

    /// Called from the render thread whenever new frames or a status change are available
    /// (e.g. to request a repaint).
    pub fn on_update(&self, callback: impl Fn() + Send + Sync + 'static) {
        *lock(&self.shared.on_update) = Some(Arc::new(callback));
    }

    pub fn set_video(&self, video: Option<PathBuf>) {
        self.edit(|state| state.video = video);
    }

    /// The audio tracks. Audio inputs naming a track that isn't here read silence.
    pub fn set_audio_tracks(&self, tracks: Vec<AudioTrackSpec>) {
        if lock(&self.shared.state).tracks == tracks {
            return;
        }
        self.edit(|state| state.tracks = tracks);
    }

    /// The graph to render. Setting a graph that renders the same as the one in use changes
    /// nothing: edits to nodes that don't feed the output, labels and positions are not edits as
    /// far as rendered frames are concerned.
    pub fn set_graph(&self, graph: GraphDesc) {
        {
            let state = lock(&self.shared.state);
            if let Some(old) = &state.graph {
                let registry = Registry::shared();
                if render_form(old, registry, state.bypass_all)
                    == render_form(&graph, registry, state.bypass_all)
                {
                    return;
                }
            }
        }
        self.edit(|state| state.graph = Some(graph));
    }

    /// Skips the whole graph, as if the video were plugged straight into the output.
    pub fn set_bypass_all(&self, bypass: bool) {
        if lock(&self.shared.state).bypass_all == bypass {
            return;
        }
        self.edit(|state| state.bypass_all = bypass);
    }

    /// The project tempo that beat and bar units follow. Setting the tempo already in use changes
    /// nothing.
    pub fn set_tempo(&self, tempo: Tempo) {
        let tempo = tempo.sanitized();
        if lock(&self.shared.state).tempo == tempo {
            return;
        }
        self.edit(|state| state.tempo = tempo);
    }

    pub fn set_preview_scale(&self, scale: PreviewScale) {
        if lock(&self.shared.state).key.scale == scale {
            return;
        }
        self.edit(|state| state.key.scale = scale);
    }

    /// The frames playback loops over, if any. Near the loop's end, rendering ahead continues
    /// from its start instead of past its end. Not an edit: nothing rendered is invalidated.
    pub fn set_loop(&self, frames: Option<Range<usize>>) {
        let mut state = lock(&self.shared.state);
        if state.looping != frames {
            state.looping = frames;
            state.changes += 1;
            self.shared.changed.notify_all();
        }
    }

    pub fn set_playhead(&self, frame: usize) {
        let mut state = lock(&self.shared.state);
        if state.playhead != frame {
            state.playhead = frame;
            self.shared.playhead.store(frame, Ordering::SeqCst);
            state.changes += 1;
            self.shared.changed.notify_all();
        }
    }

    /// A rendered frame for the current project, if it's ready. Never a frame from before the
    /// latest edit.
    pub fn frame(&self, index: usize) -> Option<Arc<Frame>> {
        lock(&self.shared.cache).get(index)
    }

    /// How many consecutive frames are rendered starting at `index`.
    pub fn buffered_from(&self, index: usize) -> usize {
        lock(&self.shared.cache).run_from(index)
    }

    /// The rendered frames, as ranges of frame indices.
    pub fn cached_ranges(&self) -> Vec<Range<usize>> {
        lock(&self.shared.cache).ranges()
    }

    /// The frame being rendered and how far along it is, if rendering is under way. Use it to
    /// tell the user the engine is warming up after a seek.
    pub fn progress(&self) -> Option<RenderProgress> {
        *lock(&self.shared.progress)
    }

    pub fn status(&self) -> EngineStatus {
        lock(&self.shared.state).status.clone()
    }

    /// Size, frame rate and length of the current preview, once loaded.
    pub fn info(&self) -> Option<RenderInfo> {
        lock(&self.shared.state).info
    }

    /// Each node's own latency and warmup in the compiled graph, once it has compiled. Nodes that
    /// don't feed the output aren't listed.
    pub fn node_stats(&self) -> Vec<NodeStats> {
        lock(&self.shared.state).node_stats.clone()
    }

    /// The sources, frame rate, tempo and output size the last graph that compiled was
    /// compiled against, for inspecting the edited graph ([`rastersong_graph::Graph::inspect`]).
    /// Kept when a later graph fails to compile.
    pub fn compile_options(&self) -> Option<CompileOptions> {
        lock(&self.shared.state).compile_options.clone()
    }

    /// Renders the graph's sound at `rate` samples a second. Setting the rate already in use
    /// changes nothing.
    pub fn set_audio_rate(&self, rate: u32) {
        let rate = rate.max(1);
        if lock(&self.shared.state).audio_rate == rate {
            return;
        }
        self.edit(|state| state.audio_rate = rate);
    }

    /// Limits the frames pre-rendered after a seek, which only affects how exact a seek into
    /// long-memory nodes is, never what the nodes do. Setting the limit already in use changes
    /// nothing; changing it re-renders, like an edit.
    pub fn set_max_warmup_frames(&self, frames: u32) {
        let frames = frames.min(MAX_WARMUP_FRAMES_LIMIT);
        if lock(&self.shared.state).max_warmup_frames == frames {
            return;
        }
        self.edit(|state| state.max_warmup_frames = frames);
    }

    /// What the current render's audio is: the source tracks (no audio output), one track passed
    /// through, or the graph's rendered sound, which [`Self::rendered_audio`] plays.
    pub fn audio_sink(&self) -> AudioSink {
        lock(&self.shared.state).audio_sink.clone()
    }

    /// The rendered sound of the cached frames, for playback. Always reads the current cache, so
    /// it never plays sound from before an edit.
    pub fn rendered_audio(&self) -> Arc<dyn RenderedSource> {
        Arc::new(RenderedAudio {
            shared: self.shared.clone(),
        })
    }

    /// What connection `request` carries, read without touching the render or the cache. The
    /// first call for a request returns [`TapOutcome::Pending`] and the answer comes from the
    /// render thread, which calls [`Self::on_update`]; ask again then. Only the latest request
    /// is kept, and an answer is dropped when the project is edited.
    pub fn tap(&self, request: &TapRequest) -> TapOutcome {
        let mut state = lock(&self.shared.state);
        if matches!(state.status, EngineStatus::Failed(_))
            || state.video.is_none()
            || state.graph.is_none()
        {
            return TapOutcome::NotRendered;
        }
        if let Some(done) = &*lock(&self.shared.tap_done)
            && done.request == *request
            && done.key == state.key
        {
            return done.outcome.clone();
        }
        if state.tap.as_ref() != Some(request) {
            state.tap = Some(request.clone());
            self.shared.tap_serial.fetch_add(1, Ordering::SeqCst);
            state.changes += 1;
            self.shared.changed.notify_all();
        }
        TapOutcome::Pending
    }

    /// Listens to a connection (or to nothing, with `None`) from video frame `frame`: the render
    /// thread renders its sound a little ahead of `frame`, which the caller keeps moving as
    /// listening goes on. Read with [`Self::listened_audio`]. Never touches the render or the
    /// cache.
    pub fn listen(&self, target: Option<ListenTarget>, frame: usize) {
        let mut state = lock(&self.shared.state);
        let moved = self.shared.listen_frame.swap(frame, Ordering::SeqCst) != frame;
        if state.listen != target {
            state.listen = target;
            self.shared.listen_serial.fetch_add(1, Ordering::SeqCst);
            lock(&self.shared.listened).blocks.clear();
        } else if !moved || state.listen.is_none() {
            return;
        }
        state.changes += 1;
        self.shared.changed.notify_all();
    }

    /// The sound of the connection being listened to, for a [`crate::playback::Mixer`].
    pub fn listened_audio(&self) -> Arc<dyn RenderedSource> {
        Arc::new(ListenedAudio {
            shared: self.shared.clone(),
        })
    }

    /// Forgets a tap that is no longer wanted, so the render thread doesn't spend time on it.
    pub fn cancel_tap(&self) {
        let mut state = lock(&self.shared.state);
        if state.tap.take().is_some() {
            self.shared.tap_serial.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// The decoded audio tracks, once loaded.
    pub fn loaded_tracks(&self) -> Vec<LoadedTrack> {
        lock(&self.shared.state).loaded.clone()
    }

    /// Records an edit: everything rendered so far is invalid, and in-flight work is cancelled.
    fn edit(&self, change: impl FnOnce(&mut State)) {
        let mut state = lock(&self.shared.state);
        change(&mut state);
        state.key.version += 1;
        state.changes += 1;
        // Lock order is always state, then cache.
        lock(&self.shared.cache).set_key(state.key);
        lock(&self.shared.listened).blocks.clear();
        self.shared.edits.fetch_add(1, Ordering::SeqCst);
        self.shared.changed.notify_all();
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        lock(&self.shared.state).shutdown = true;
        self.shared.changed.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The engine's cached sound, as playback reads it.
struct RenderedAudio {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for RenderedAudio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderedAudio").finish_non_exhaustive()
    }
}

impl RenderedSource for RenderedAudio {
    fn blocks(&self, frames: Range<usize>) -> Vec<Option<Arc<AudioBlock>>> {
        let cache = lock(&self.shared.cache);
        frames
            .map(|i| cache.get(i).and_then(|f| f.audio.clone()))
            .collect()
    }

    fn frame_rate(&self) -> f64 {
        lock(&self.shared.state)
            .info
            .map_or(0.0, |info| info.frame_rate.as_f64())
    }
}

/// The sound of the connection being listened to, as playback reads it.
struct ListenedAudio {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for ListenedAudio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListenedAudio").finish_non_exhaustive()
    }
}

impl RenderedSource for ListenedAudio {
    fn blocks(&self, frames: Range<usize>) -> Vec<Option<Arc<AudioBlock>>> {
        let listened = lock(&self.shared.listened);
        frames.map(|i| listened.blocks.get(&i).cloned()).collect()
    }

    fn frame_rate(&self) -> f64 {
        lock(&self.shared.state)
            .info
            .map_or(0.0, |info| info.frame_rate.as_f64())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic on the render thread must not take the UI down with it.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The frames worth rendering ahead: from the playhead for a window's worth, and when looping,
/// round from the loop's end to its start.
#[derive(Debug, Clone, PartialEq)]
struct Wanted {
    ahead: Range<usize>,
    wrapped: Range<usize>,
}

impl Wanted {
    fn new(playhead: usize, window: usize, looping: Option<Range<usize>>, frames: usize) -> Self {
        let looping = looping.filter(|l| playhead < l.end);
        let limit = looping.as_ref().map_or(frames, |l| l.end).min(frames);
        let ahead = playhead..playhead.saturating_add(window).min(limit).max(playhead);
        let wrapped = match looping {
            Some(l) => {
                let left = window.saturating_sub(ahead.len());
                l.start..l.start.saturating_add(left).min(playhead).min(l.end)
            }
            None => 0..0,
        };
        Self { ahead, wrapped }
    }

    fn frames(&self) -> impl Iterator<Item = usize> {
        self.ahead.clone().chain(self.wrapped.clone())
    }

    fn contains(&self, frame: usize) -> bool {
        self.ahead.contains(&frame) || self.wrapped.contains(&frame)
    }
}

/// Decoded audio by file, so editing the graph or an offset doesn't decode again.
type AudioCache = HashMap<PathBuf, DecodedAudio>;

#[derive(Clone)]
struct DecodedAudio {
    clip: Arc<AudioClip>,
    modulator: Arc<Modulator>,
    waveform: Arc<Waveform>,
}

struct Worker {
    shared: Arc<Shared>,
    built: Option<Built>,
    /// The renderer taps are answered by, apart from the one filling the cache.
    tap_built: Option<Built>,
    /// The key the tap renderer failed to build for, so it isn't tried again until an edit.
    tap_failed: Option<CacheKey>,
    /// Turns the listened connection's signal into sound, for the connection it was made for.
    listen_sink: Option<ListenSink>,
    audio: AudioCache,
    /// The `changes` count when the project last failed; nothing is retried until it moves.
    failed_at: Option<u64>,
}

/// The resampler for one listened connection.
struct ListenSink {
    serial: u64,
    /// The signal's block length and samples per pixel it was made for.
    shape: (usize, u32),
    resampler: SinkResampler,
}

/// What the worker should do next, decided while holding the state lock.
enum Job {
    Build {
        key: CacheKey,
        edits: u64,
        project: Snapshot,
    },
    /// Build the renderer taps are answered by.
    BuildTap {
        key: CacheKey,
        edits: u64,
        project: Snapshot,
    },
    /// Render the sound of frame `frame` of the connection being listened to.
    Listen { frame: usize, serial: u64 },
    /// Answer the tap `request`; `serial` says which request it was.
    Tap { request: TapRequest, serial: u64 },
    /// Render ahead. `seen` is the change count when this was decided: if there turns out to be
    /// nothing to render, the worker sleeps until the count moves past it, so a change made in
    /// between is never missed.
    Render { seen: u64 },
}

impl Worker {
    fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            built: None,
            tap_built: None,
            tap_failed: None,
            listen_sink: None,
            audio: HashMap::new(),
            failed_at: None,
        }
    }

    fn run(mut self) {
        while let Some(job) = self.next_job() {
            match job {
                Job::Build {
                    key,
                    edits,
                    project,
                } => self.build(key, edits, project),
                Job::BuildTap {
                    key,
                    edits,
                    project,
                } => self.build_tap(key, edits, project),
                Job::Tap { request, serial } => self.run_tap(request, serial),
                Job::Listen { frame, serial } => self.run_listen(frame, serial),
                Job::Render { seen } => {
                    if !self.render_next() {
                        self.wait_for_change(seen);
                    }
                }
            }
        }
    }

    /// Waits until there's a project to work on. `None` means shut down.
    fn next_job(&mut self) -> Option<Job> {
        let mut state = lock(&self.shared.state);
        loop {
            if state.shutdown {
                return None;
            }
            let stuck = self.failed_at == Some(state.changes);
            if state.tap.is_none()
                && state.listen.is_none()
                && self.tap_built.as_ref().is_some_and(|b| b.key != state.key)
            {
                // Nothing is asking, and what it would answer with is out of date.
                self.tap_built = None;
            }
            if let (Some(video), Some(graph), false) = (&state.video, &state.graph, stuck) {
                if (state.tap.is_some() || state.listen.is_some())
                    && self.tap_failed != Some(state.key)
                {
                    if self.tap_built.as_ref().is_some_and(|b| b.key == state.key) {
                        if let Some(request) = state.tap.clone() {
                            return Some(Job::Tap {
                                request,
                                serial: self.shared.tap_serial.load(Ordering::SeqCst),
                            });
                        }
                        if let Some(frame) = self.next_listen_frame(&state) {
                            return Some(Job::Listen {
                                frame,
                                serial: self.shared.listen_serial.load(Ordering::SeqCst),
                            });
                        }
                    } else {
                        return Some(Job::BuildTap {
                            key: state.key,
                            edits: self.shared.edits.load(Ordering::SeqCst),
                            project: Self::snapshot(&state, video, graph),
                        });
                    }
                }
                if self.built.as_ref().is_some_and(|b| b.key == state.key) {
                    return Some(Job::Render {
                        seen: state.changes,
                    });
                }
                let job = Job::Build {
                    key: state.key,
                    edits: self.shared.edits.load(Ordering::SeqCst),
                    project: Self::snapshot(&state, video, graph),
                };
                state.status = EngineStatus::Loading;
                return Some(job);
            }
            if state.video.is_none() || state.graph.is_none() {
                state.status = EngineStatus::Idle;
            }
            let seen = state.changes;
            state = self
                .shared
                .changed
                .wait_while(state, |s| s.changes == seen && !s.shutdown)
                .unwrap_or_else(|p| p.into_inner());
        }
    }

    /// The parts of the project a renderer is built from.
    fn snapshot(state: &State, video: &Path, graph: &GraphDesc) -> Snapshot {
        Snapshot {
            video: video.to_owned(),
            tracks: state.tracks.clone(),
            graph: render_form(graph, Registry::shared(), state.bypass_all),
            tempo: state.tempo,
            audio_rate: state.audio_rate,
            max_warmup_frames: state.max_warmup_frames,
        }
    }

    fn wait_for_change(&self, seen: u64) {
        let state = lock(&self.shared.state);
        drop(
            self.shared
                .changed
                .wait_while(state, |s| s.changes == seen && !s.shutdown)
                .unwrap_or_else(|p| p.into_inner()),
        );
    }

    fn notify(&self) {
        let callback = lock(&self.shared.on_update).clone();
        if let Some(callback) = callback {
            callback();
        }
    }

    fn build(&mut self, key: CacheKey, edits: u64, project: Snapshot) {
        self.built = None;
        let tracks = self.load_tracks(&project.tracks);
        let result = tracks.and_then(|(tracks, loaded)| {
            // Publish the audio even if the graph turns out not to compile: playback needs it.
            lock(&self.shared.state).loaded = loaded;
            self.renderer(&tracks, key, &project)
        });

        // A graph that can't render shouldn't hide the video: the timeline and playhead still
        // need its length and frame rate.
        let video_info = match &result {
            Err(_) => self
                .shared
                .backend
                .open_video(&project.video)
                .ok()
                .map(|video| {
                    let info = video.info();
                    let (width, height) = key.scale.output_size().resolve(info.width, info.height);
                    RenderInfo {
                        width,
                        height,
                        frame_rate: info.frame_rate,
                        frames: info.frame_count,
                    }
                }),
            Ok(_) => None,
        };

        let mut state = lock(&self.shared.state);
        if self.shared.edits.load(Ordering::SeqCst) != edits {
            return; // Edited while building; start over.
        }
        match result {
            Ok(renderer) => {
                state.status = EngineStatus::Ready;
                state.info = Some(*renderer.info());
                state.node_stats = renderer.node_stats().to_vec();
                state.compile_options = Some(renderer.compile_options().clone());
                state.audio_sink = renderer.audio_sink();
                self.built = Some(Built { key, renderer });
                self.failed_at = None;
            }
            Err(failure) => {
                tracing::warn!("project can't be rendered: {}", failure.message);
                state.status = EngineStatus::Failed(failure);
                state.info = video_info;
                state.node_stats.clear();
                state.audio_sink = AudioSink::Source;
                self.failed_at = Some(state.changes);
            }
        }
        drop(state);
        self.notify();
    }

    /// A renderer for `project` at the preview scale of `key`.
    fn renderer(
        &self,
        tracks: &[AudioTrack],
        key: CacheKey,
        project: &Snapshot,
    ) -> Result<Renderer, Failure> {
        Renderer::new(
            self.shared.backend.as_ref(),
            &project.video,
            tracks,
            &project.graph,
            project.tempo,
            Registry::shared(),
            key.scale.output_size(),
        )
        .map(|mut renderer| {
            renderer.set_audio_rate(project.audio_rate);
            renderer.set_max_warmup_frames(project.max_warmup_frames);
            renderer
        })
        .map_err(|e| Failure::from_error(&e))
    }

    /// Builds the renderer taps are answered by. A graph that fails to compile here fails the
    /// tap; the main build reports it to the user.
    fn build_tap(&mut self, key: CacheKey, edits: u64, project: Snapshot) {
        self.tap_built = None;
        let renderer = self
            .load_tracks(&project.tracks)
            .and_then(|(tracks, _)| self.renderer(&tracks, key, &project));
        if self.shared.edits.load(Ordering::SeqCst) != edits {
            return; // Edited while building; the tap asks again.
        }
        match renderer {
            Ok(renderer) => self.tap_built = Some(Built { key, renderer }),
            Err(failure) => {
                self.tap_failed = Some(key);
                let serial = self.shared.tap_serial.load(Ordering::SeqCst);
                let request = lock(&self.shared.state).tap.clone();
                if let Some(request) = request {
                    self.finish_tap(request, serial, key, TapOutcome::Failed(failure.detail));
                }
            }
        }
    }

    /// Renders the frame of `request` on the tap renderer and reads the connection.
    fn run_tap(&mut self, request: TapRequest, serial: u64) {
        let edits = self.shared.edits.load(Ordering::SeqCst);
        let Some(built) = self.tap_built.as_mut() else {
            return;
        };
        let key = built.key;
        let info = *built.renderer.info();
        let shared = &self.shared;
        let cancel = || {
            shared.edits.load(Ordering::SeqCst) != edits
                || shared.tap_serial.load(Ordering::SeqCst) != serial
        };
        let outcome = if request.frame >= info.frames {
            TapOutcome::NotRendered
        } else {
            match built.renderer.render(request.frame, &cancel) {
                Ok(Some(_)) => match built.renderer.tap(&request.node, request.output) {
                    Some(signal) => TapOutcome::Ready(Arc::new(tap::read(
                        request.clone(),
                        signal,
                        (info.width, info.height),
                        info.frame_rate.as_f64(),
                    ))),
                    None => TapOutcome::NotRendered,
                },
                // Superseded or edited: whoever did that asks again.
                Ok(None) => return,
                Err(e) => TapOutcome::Failed(e.to_string()),
            }
        };
        self.finish_tap(request, serial, key, outcome);
    }

    /// The first frame of the sound of the connection being listened to that isn't rendered yet,
    /// from the listening position on for [`listen::AHEAD_SECS`].
    fn next_listen_frame(&self, state: &State) -> Option<usize> {
        state.listen.as_ref()?;
        let info = self.tap_built.as_ref()?.renderer.info();
        let from = self.shared.listen_frame.load(Ordering::SeqCst);
        let ahead = (info.frame_rate.as_f64() * listen::AHEAD_SECS).ceil() as usize;
        let listened = lock(&self.shared.listened);
        (from..from.saturating_add(ahead).min(info.frames))
            .find(|f| !listened.blocks.contains_key(f))
    }

    /// Renders frame `frame` on the tap renderer and keeps the sound of the listened connection.
    fn run_listen(&mut self, frame: usize, serial: u64) {
        let edits = self.shared.edits.load(Ordering::SeqCst);
        let Some(target) = lock(&self.shared.state).listen.clone() else {
            return;
        };
        let Some(built) = self.tap_built.as_mut() else {
            return;
        };
        let info = *built.renderer.info();
        let rate = lock(&self.shared.state).audio_rate;
        let shared = &self.shared;
        let cancel = || {
            shared.edits.load(Ordering::SeqCst) != edits
                || shared.listen_serial.load(Ordering::SeqCst) != serial
        };
        let rendered = built.renderer.render(frame, &cancel);
        let block = match rendered {
            Ok(Some(_)) => match built.renderer.tap(&target.node, target.output) {
                Some(signal) => {
                    let shape = (signal.data.len(), signal.layout.samples_per_pixel);
                    if self
                        .listen_sink
                        .as_ref()
                        .is_none_or(|s| s.serial != serial || s.shape != shape)
                    {
                        self.listen_sink = Some(ListenSink {
                            serial,
                            shape,
                            resampler: SinkResampler::new(
                                shape.0,
                                shape.1,
                                info.frame_rate.as_f64(),
                                rate,
                            ),
                        });
                    }
                    let sink = self.listen_sink.as_mut().expect("set above");
                    sink.resampler.push(frame as i64, &signal.data)
                }
                // Not rendered: an empty block, so the frame isn't asked for again.
                None => AudioBlock {
                    start: 0,
                    sample_rate: rate,
                    channels: 1,
                    samples: Vec::new(),
                },
            },
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(frame, "listening failed: {e}");
                AudioBlock {
                    start: 0,
                    sample_rate: rate,
                    channels: 1,
                    samples: Vec::new(),
                }
            }
        };
        if shared.listen_serial.load(Ordering::SeqCst) != serial {
            return;
        }
        let ahead = (info.frame_rate.as_f64() * listen::AHEAD_SECS).ceil() as usize;
        let mut listened = lock(&shared.listened);
        listened.blocks.insert(frame, Arc::new(block));
        listened.trim(shared.listen_frame.load(Ordering::SeqCst), ahead);
    }

    /// Records the answer to a tap, unless a newer request has replaced it.
    fn finish_tap(&self, request: TapRequest, serial: u64, key: CacheKey, outcome: TapOutcome) {
        let mut state = lock(&self.shared.state);
        if self.shared.tap_serial.load(Ordering::SeqCst) != serial {
            return;
        }
        state.tap = None;
        *lock(&self.shared.tap_done) = Some(TapDone {
            request,
            key,
            outcome,
        });
        drop(state);
        self.notify();
    }

    /// Decodes each track (reusing earlier decodes of the same file).
    fn load_tracks(
        &mut self,
        specs: &[AudioTrackSpec],
    ) -> Result<(Vec<AudioTrack>, Vec<LoadedTrack>), Failure> {
        // Forget files no track uses any more.
        self.audio
            .retain(|path, _| specs.iter().any(|s| &s.path == path));
        let mut tracks = Vec::with_capacity(specs.len());
        let mut loaded = Vec::with_capacity(specs.len());
        for spec in specs {
            let decoded = match self.audio.get(&spec.path) {
                Some(cached) => cached.clone(),
                None => {
                    let clip = self
                        .shared
                        .backend
                        .load_audio(&spec.path, AudioOptions::default())
                        .map_err(|e| {
                            Failure::new(tr_args(
                                "error.audio_track",
                                &[("name", &spec.name), ("error", &e.to_string())],
                            ))
                        })?;
                    let entry = DecodedAudio {
                        modulator: Arc::new(Modulator::new(&clip)),
                        waveform: Arc::new(Waveform::new(&clip)),
                        clip: Arc::new(clip),
                    };
                    self.audio.insert(spec.path.clone(), entry.clone());
                    entry
                }
            };
            tracks.push(AudioTrack {
                name: spec.name.clone(),
                modulator: decoded.modulator,
                offset: spec.offset,
            });
            loaded.push(LoadedTrack {
                name: spec.name.clone(),
                clip: decoded.clip,
                waveform: decoded.waveform,
            });
        }
        Ok((tracks, loaded))
    }

    /// Frames ahead of the playhead to keep rendered: the lookahead time, limited so that most of
    /// the cache budget goes to frames ahead.
    fn window(config: &EngineConfig, info: &RenderInfo) -> usize {
        let frame_bytes = (info.width as usize * info.height as usize * 3).max(1);
        let budget_frames = config.cache_bytes / frame_bytes * 3 / 4;
        let lookahead = (config.lookahead_secs * info.frame_rate.as_f64()).ceil() as usize;
        lookahead.min(budget_frames).max(1)
    }

    /// Renders the first missing frame ahead of the playhead. Returns false if there's nothing to do.
    fn render_next(&mut self) -> bool {
        let Some(info) = self.built.as_ref().map(|b| *b.renderer.info()) else {
            return false;
        };
        let window = Self::window(&lock(&self.shared.config), &info);
        let wanted = |shared: &Shared| {
            let playhead = shared.playhead.load(Ordering::SeqCst);
            let looping = lock(&shared.state).looping.clone();
            Wanted::new(playhead, window, looping, info.frames)
        };
        let target = {
            let wanted = wanted(&self.shared);
            let cache = lock(&self.shared.cache);
            wanted.frames().find(|&i| !cache.contains(i))
        };
        let Some(target) = target else {
            return false;
        };

        let edits = self.shared.edits.load(Ordering::SeqCst);
        let built = self.built.as_mut().expect("checked above");
        let shared = &self.shared;
        // Stop between frames if the project changes or the playhead moves so that this frame is
        // no longer wanted.
        let cancel =
            || shared.edits.load(Ordering::SeqCst) != edits || !wanted(shared).contains(target);
        let progress = |step: renderer::Step| {
            *lock(&shared.progress) = Some(RenderProgress {
                frame: target,
                done: step.done,
                total: step.total,
                warming: step.restarted && step.total > 1,
            });
            // Warm-up can take seconds; let the UI show how far along it is.
            if step.restarted && step.total > 1 {
                let callback = lock(&shared.on_update).clone();
                if let Some(callback) = callback {
                    callback();
                }
            }
        };
        let rendered = built.renderer.render_with(target, &cancel, &progress);
        *lock(&self.shared.progress) = None;
        match rendered {
            Ok(Some(rgb)) => {
                let frame = Frame {
                    index: target,
                    width: info.width,
                    height: info.height,
                    rgb: rgb.to_vec(),
                    levels: built.renderer.levels().into(),
                    params: built.renderer.param_levels().into(),
                    costs: built.renderer.costs().into(),
                    meters: built.renderer.meters().into(),
                    audio: built.renderer.audio().cloned().map(Arc::new),
                };
                let playhead = self.shared.playhead.load(Ordering::SeqCst);
                let inserted = lock(&self.shared.cache).insert(built.key, frame, playhead);
                if inserted {
                    self.notify();
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(frame = target, "render failed: {e}");
                let mut state = lock(&self.shared.state);
                state.status = EngineStatus::Failed(Failure::from_error(&e));
                self.failed_at = Some(state.changes);
                self.built = None;
                drop(state);
                self.notify();
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_scales_are_named_by_their_fraction() {
        let labels: Vec<&str> = PreviewScale::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(labels, ["Full", "Half", "Quarter", "Eighth", "Sixteenth"]);
        let divisors: Vec<u32> = PreviewScale::ALL.iter().map(|s| s.divisor()).collect();
        assert_eq!(divisors, [1, 2, 4, 8, 16]);
    }

    #[test]
    fn rendering_ahead_wraps_round_a_loop() {
        // Playing frames 20..40 with the playhead at 35 and room for 10 frames: the last 5 of
        // the loop, then the first 5 from its start.
        let wanted = Wanted::new(35, 10, Some(20..40), 100);
        assert_eq!(
            wanted.frames().collect::<Vec<_>>(),
            [35, 36, 37, 38, 39, 20, 21, 22, 23, 24]
        );
        assert!(!wanted.contains(40), "nothing past the loop");
        // Without a loop, or past its end, it renders straight ahead.
        assert_eq!(Wanted::new(35, 10, None, 100).frames().last(), Some(44));
        assert_eq!(
            Wanted::new(50, 10, Some(20..40), 100).frames().next(),
            Some(50)
        );
        // Never past the video's end.
        assert_eq!(Wanted::new(95, 10, None, 100).frames().count(), 5);
    }
}
