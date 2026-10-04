//! The preview render service: a background thread that keeps rendering ahead of the playhead
//! into the frame cache, whether or not playback is running.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use rastersong_graph::{GraphDesc, Registry};
use rastersong_media::{AudioClip, AudioOptions, MediaBackend};

use crate::cache::{CacheKey, Frame, FrameCache};
use crate::sources::Modulator;
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
}

impl PreviewScale {
    pub fn output_size(self) -> OutputSize {
        match self {
            Self::Full => OutputSize::Native,
            Self::Half => OutputSize::Scaled(0.5),
            Self::Quarter => OutputSize::Scaled(0.25),
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
}

/// Why the project can't be rendered as it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub message: String,
    /// The node at fault, when the problem is with one node of the graph.
    pub node: Option<String>,
}

impl Failure {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            node: None,
        }
    }

    fn from_error(error: &EngineError) -> Self {
        Self {
            message: error.to_string(),
            node: match error {
                EngineError::Graph(e) => e.node().map(str::to_owned),
                _ => None,
            },
        }
    }
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
    config: EngineConfig,
    state: Mutex<State>,
    /// Signalled on every change to `state`.
    changed: Condvar,
    cache: Mutex<FrameCache>,
    /// Copies of state the worker checks between frames, without locking.
    edits: AtomicU64,
    playhead: AtomicUsize,
    on_update: Mutex<Option<Callback>>,
}

struct State {
    video: Option<PathBuf>,
    tracks: Vec<AudioTrackSpec>,
    graph: Option<GraphDesc>,
    key: CacheKey,
    playhead: usize,
    status: EngineStatus,
    info: Option<RenderInfo>,
    loaded: Vec<LoadedTrack>,
    /// Bumped on every change, so the worker knows when to look again.
    changes: u64,
    shutdown: bool,
}

/// The parts of the project a renderer is built from.
struct Snapshot {
    video: PathBuf,
    tracks: Vec<AudioTrackSpec>,
    graph: GraphDesc,
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
            config,
            state: Mutex::new(State {
                video: None,
                tracks: Vec::new(),
                graph: None,
                key,
                playhead: 0,
                status: EngineStatus::Idle,
                info: None,
                loaded: Vec::new(),
                changes: 0,
                shutdown: false,
            }),
            changed: Condvar::new(),
            edits: AtomicU64::new(0),
            playhead: AtomicUsize::new(0),
            on_update: Mutex::new(None),
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

    pub fn set_graph(&self, graph: GraphDesc) {
        self.edit(|state| state.graph = Some(graph));
    }

    pub fn set_preview_scale(&self, scale: PreviewScale) {
        self.edit(|state| state.key.scale = scale);
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

    pub fn status(&self) -> EngineStatus {
        lock(&self.shared.state).status.clone()
    }

    /// Size, frame rate and length of the current preview, once loaded.
    pub fn info(&self) -> Option<RenderInfo> {
        lock(&self.shared.state).info
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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic on the render thread must not take the UI down with it.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Decoded audio by file, so editing the graph or an offset doesn't decode again.
type AudioCache = HashMap<PathBuf, (Arc<AudioClip>, Arc<Modulator>)>;

struct Worker {
    shared: Arc<Shared>,
    built: Option<Built>,
    audio: AudioCache,
    /// The `changes` count when the project last failed; nothing is retried until it moves.
    failed_at: Option<u64>,
}

/// What the worker should do next, decided while holding the state lock.
enum Job {
    Build {
        key: CacheKey,
        edits: u64,
        project: Snapshot,
    },
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
            if let (Some(video), Some(graph), false) = (&state.video, &state.graph, stuck) {
                if self.built.as_ref().is_some_and(|b| b.key == state.key) {
                    return Some(Job::Render {
                        seen: state.changes,
                    });
                }
                let job = Job::Build {
                    key: state.key,
                    edits: self.shared.edits.load(Ordering::SeqCst),
                    project: Snapshot {
                        video: video.clone(),
                        tracks: state.tracks.clone(),
                        graph: graph.clone(),
                    },
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
            Renderer::new(
                self.shared.backend.as_ref(),
                &project.video,
                &tracks,
                &project.graph,
                &Registry::default(),
                key.scale.output_size(),
            )
            .map_err(|e| Failure::from_error(&e))
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
                self.built = Some(Built { key, renderer });
                self.failed_at = None;
            }
            Err(failure) => {
                tracing::warn!("project can't be rendered: {}", failure.message);
                state.status = EngineStatus::Failed(failure);
                state.info = video_info;
                self.failed_at = Some(state.changes);
            }
        }
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
            let (clip, modulator) = match self.audio.get(&spec.path) {
                Some(cached) => cached.clone(),
                None => {
                    let clip = self
                        .shared
                        .backend
                        .load_audio(&spec.path, AudioOptions::default())
                        .map_err(|e| Failure::new(format!("audio track `{}`: {e}", spec.name)))?;
                    let modulator = Arc::new(Modulator::new(&clip));
                    let entry = (Arc::new(clip), modulator);
                    self.audio.insert(spec.path.clone(), entry.clone());
                    entry
                }
            };
            tracks.push(AudioTrack {
                name: spec.name.clone(),
                modulator,
                offset: spec.offset,
            });
            loaded.push(LoadedTrack {
                name: spec.name.clone(),
                clip,
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
        let window = Self::window(&self.shared.config, &info);
        let playhead = self.shared.playhead.load(Ordering::SeqCst);
        let end = playhead.saturating_add(window).min(info.frames);
        let target = {
            let cache = lock(&self.shared.cache);
            (playhead..end).find(|&i| !cache.contains(i))
        };
        let Some(target) = target else {
            return false;
        };

        let edits = self.shared.edits.load(Ordering::SeqCst);
        let built = self.built.as_mut().expect("checked above");
        let shared = &self.shared;
        // Stop between frames if the project changes or the playhead moves so that this frame is
        // no longer wanted.
        let cancel = || {
            let playhead = shared.playhead.load(Ordering::SeqCst);
            shared.edits.load(Ordering::SeqCst) != edits
                || target < playhead
                || target >= playhead.saturating_add(window)
        };
        match built.renderer.render(target, &cancel) {
            Ok(Some(rgb)) => {
                let frame = Frame {
                    index: target,
                    width: info.width,
                    height: info.height,
                    rgb: rgb.to_vec(),
                    levels: built.renderer.levels().into(),
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
