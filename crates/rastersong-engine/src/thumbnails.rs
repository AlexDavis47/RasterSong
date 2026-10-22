//! Small frames of the source video for the timeline, decoded on their own thread.
//!
//! Thumbnails show the video as it is in the file, not the rendered output, so graph edits never
//! invalidate them, and their decoder is separate from the renderer's, so they never slow
//! rendering down. Requests are latest-wins: the timeline asks for the frames it can see, and
//! frames it has scrolled away from are skipped.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use rastersong_media::{MediaBackend, VideoFrame, VideoInfo};

/// Thumbnail height in pixels; the width follows the video's aspect ratio.
pub const THUMBNAIL_HEIGHT: u32 = 64;
/// Thumbnails kept in memory (about 1 MB per 60 at 16:9).
const CAPACITY: usize = 600;

/// A video stream of a file, as thumbnails are made of it: its path and stream index (the file's
/// best video stream for `None`).
pub type VideoKey = (PathBuf, Option<usize>);

type Callback = Arc<dyn Fn() + Send + Sync>;

pub struct Thumbnails {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Thumbnails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Thumbnails").finish_non_exhaustive()
    }
}

struct Shared {
    backend: Arc<dyn MediaBackend>,
    state: Mutex<State>,
    changed: Condvar,
    on_update: Mutex<Option<Callback>>,
}

#[derive(Default)]
struct State {
    video: Option<VideoKey>,
    /// Bumped when the video changes, so the worker drops what it was doing.
    generation: u64,
    info: Option<VideoInfo>,
    /// Frames the timeline wants, in the order it asked.
    wanted: Vec<usize>,
    frames: BTreeMap<usize, Arc<VideoFrame>>,
    /// Decoded frames, oldest first, for eviction.
    order: VecDeque<usize>,
    shutdown: bool,
}

impl Thumbnails {
    pub fn new(backend: Arc<dyn MediaBackend>) -> Self {
        let shared = Arc::new(Shared {
            backend,
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
            on_update: Mutex::new(None),
        });
        let worker = std::thread::Builder::new()
            .name("rastersong-thumbnails".into())
            .spawn({
                let shared = shared.clone();
                move || run(&shared)
            })
            .expect("failed to start the thumbnail thread");
        Self {
            shared,
            worker: Some(worker),
        }
    }

    /// Called from the thumbnail thread when new thumbnails are ready (e.g. to repaint).
    pub fn on_update(&self, callback: impl Fn() + Send + Sync + 'static) {
        *lock(&self.shared.on_update) = Some(Arc::new(callback));
    }

    /// The video stream to make thumbnails of. Changing it forgets every thumbnail.
    pub fn set_video(&self, video: Option<VideoKey>) {
        let mut state = lock(&self.shared.state);
        if state.video == video {
            return;
        }
        state.video = video;
        state.generation += 1;
        state.info = None;
        state.wanted.clear();
        state.frames.clear();
        state.order.clear();
        self.shared.changed.notify_all();
    }

    /// The frames the timeline would like, replacing any earlier request.
    pub fn request(&self, frames: &[usize]) {
        let mut state = lock(&self.shared.state);
        let missing: Vec<usize> = frames
            .iter()
            .copied()
            .filter(|f| !state.frames.contains_key(f))
            .collect();
        if missing != state.wanted {
            state.wanted = missing;
            self.shared.changed.notify_all();
        }
    }

    /// Every thumbnail decoded so far, by frame index.
    pub fn frames(&self) -> BTreeMap<usize, Arc<VideoFrame>> {
        lock(&self.shared.state).frames.clone()
    }

    /// The source video's own size, frame rate and length, once opened.
    pub fn info(&self) -> Option<VideoInfo> {
        lock(&self.shared.state).info.clone()
    }
}

impl Drop for Thumbnails {
    fn drop(&mut self) {
        lock(&self.shared.state).shutdown = true;
        self.shared.changed.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The worker: opens the video when it changes, then decodes wanted frames one at a time,
/// lowest first (the decoder's fast path is forward).
fn run(shared: &Shared) {
    let mut source = None;
    let mut opened: Option<(u64, VideoKey)> = None;
    loop {
        let (generation, video, next) = {
            let mut state = lock(&shared.state);
            loop {
                if state.shutdown {
                    return;
                }
                // A new video (or none) since the source was opened.
                let needs_open = opened.as_ref().map(|(g, _)| *g) != Some(state.generation);
                if needs_open || (source.is_some() && !state.wanted.is_empty()) {
                    break;
                }
                state = shared
                    .changed
                    .wait(state)
                    .unwrap_or_else(|p| p.into_inner());
            }
            let next = state.wanted.iter().copied().min();
            (state.generation, state.video.clone(), next)
        };

        if opened.as_ref().map(|(g, _)| *g) != Some(generation) {
            source = None;
            opened = Some((generation, video.clone().unwrap_or_default()));
            let Some((path, stream)) = video else {
                continue;
            };
            match shared.backend.open_video_stream(&path, stream) {
                Ok(mut video) => {
                    let info = video.info().clone();
                    let height = THUMBNAIL_HEIGHT.min(info.height.max(1));
                    let width = ((u64::from(info.width) * u64::from(height))
                        / u64::from(info.height.max(1)))
                    .max(1) as u32;
                    video.set_output_size(Some((width, height)));
                    let mut state = lock(&shared.state);
                    if state.generation == generation {
                        state.info = Some(info);
                    }
                    drop(state);
                    source = Some(video);
                    notify(shared);
                }
                Err(e) => tracing::warn!("thumbnails unavailable for {}: {e}", path.display()),
            }
            continue;
        }

        let (Some(index), Some(video)) = (next, source.as_mut()) else {
            continue;
        };
        let decoded = video.frame(index);
        let mut state = lock(&shared.state);
        state.wanted.retain(|&f| f != index);
        if state.generation != generation {
            continue;
        }
        match decoded {
            Ok(frame) => {
                state.frames.insert(index, frame);
                state.order.push_back(index);
                evict(&mut state);
                drop(state);
                notify(shared);
            }
            Err(e) => tracing::debug!(frame = index, "thumbnail failed: {e}"),
        }
    }
}

/// Drops the oldest thumbnails the timeline isn't asking for, down to [`CAPACITY`].
fn evict(state: &mut State) {
    let mut kept = 0;
    while state.frames.len() > CAPACITY && kept < state.order.len() {
        let oldest = state.order.pop_front().expect("checked length");
        if state.wanted.contains(&oldest) {
            state.order.push_back(oldest);
            kept += 1;
        } else {
            state.frames.remove(&oldest);
        }
    }
}

fn notify(shared: &Shared) {
    let callback = lock(&shared.on_update).clone();
    if let Some(callback) = callback {
        callback();
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use rastersong_media::{FakeBackend, FakeVideo, Rational};

    use super::*;

    fn thumbnails() -> Thumbnails {
        let backend = FakeBackend::new().with_video(
            "clip",
            FakeVideo {
                width: 320,
                height: 180,
                frame_count: 100,
                frame_rate: Rational::new(25, 1),
            },
        );
        let thumbnails = Thumbnails::new(Arc::new(backend));
        thumbnails.set_video(Some((PathBuf::from("clip"), None)));
        thumbnails
    }

    fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn decodes_requested_frames_at_thumbnail_size() {
        let thumbnails = thumbnails();
        thumbnails.request(&[40, 0, 20]);
        wait_for("thumbnails", || thumbnails.frames().len() == 3);
        let frames = thumbnails.frames();
        assert_eq!(frames.keys().copied().collect::<Vec<_>>(), [0, 20, 40]);
        let frame = &frames[&20];
        assert_eq!((frame.width, frame.height), (113, THUMBNAIL_HEIGHT));
        assert_eq!(thumbnails.info().unwrap().width, 320);
    }

    #[test]
    fn a_new_video_forgets_the_old_thumbnails() {
        let thumbnails = thumbnails();
        thumbnails.request(&[1]);
        wait_for("a thumbnail", || !thumbnails.frames().is_empty());
        thumbnails.set_video(None);
        assert!(thumbnails.frames().is_empty());
        assert!(thumbnails.info().is_none());
    }
}
