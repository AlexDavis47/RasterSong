//! The sequential renderer shared by preview and offline rendering.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use rastersong_graph::{CompileOptions, Graph, GraphDesc, Layout, Registry, Signal};
use rastersong_media::{MediaBackend, MediaError, Rational, VideoSource};

use crate::EngineError;
use crate::sources::{Modulator, fill_video, to_rgb8};

/// Source names the renderer supplies to the graph.
pub const VIDEO_SOURCE: &str = "video";
pub const AUDIO_SOURCE: &str = "audio";

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
                let scale = |v: u32| ((v as f32 * f).round() as u32).max(1);
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

/// Renders output frames of one video through one compiled graph.
///
/// The graph is stateful, so frames are produced by processing source frames in order. Asking for
/// the next frame is the fast path. Asking for any other frame resets the graph and first renders
/// (and discards) [`Graph::warmup_frames`] frames so stateful nodes have history. That makes the
/// result exact for nodes with finite memory and a close approximation for infinite-memory ones
/// (feedback, IIR filters). Rendering from frame 0 is always exact.
pub struct Renderer {
    video: Box<dyn VideoSource>,
    modulator: Arc<Modulator>,
    graph: Graph,
    info: RenderInfo,
    fps: f64,
    latency: usize,
    warmup: usize,
    sources: HashMap<String, Signal>,
    have_video: bool,
    /// The next source frame to process, if the graph's state is positioned somewhere.
    next_source: Option<usize>,
    rgb: Vec<u8>,
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
    /// Opens `video_path` at `size` and compiles `graph` for it.
    pub fn new(
        backend: &dyn MediaBackend,
        video_path: &Path,
        modulator: Arc<Modulator>,
        graph: &GraphDesc,
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

        let video_layout = Layout::rgb(width, height);
        let audio_layout = Layout::audio(modulator.block_len(fps));
        let graph = Graph::compile(
            graph,
            registry,
            &CompileOptions {
                frame_rate: fps,
                sources: HashMap::from([
                    (VIDEO_SOURCE.to_owned(), video_layout),
                    (AUDIO_SOURCE.to_owned(), audio_layout),
                ]),
                output: video_layout,
            },
        )?;

        Ok(Self {
            video,
            modulator,
            info: RenderInfo {
                width,
                height,
                frame_rate: source.frame_rate,
                frames: source.frame_count,
            },
            fps,
            latency: graph.latency_frames() as usize,
            warmup: graph.warmup_frames() as usize,
            graph,
            sources: HashMap::from([
                (VIDEO_SOURCE.to_owned(), Signal::zeros(video_layout)),
                (AUDIO_SOURCE.to_owned(), Signal::zeros(audio_layout)),
            ]),
            have_video: false,
            next_source: None,
            rgb: Vec::with_capacity(video_layout.len()),
        })
    }

    pub fn info(&self) -> &RenderInfo {
        &self.info
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
        let count = self.info.frames;
        if index >= count {
            return Err(MediaError::FrameOutOfRange { index, count }.into());
        }

        // Processing source frame `m` produces output frame `m - latency`.
        let next_output = self.next_source.and_then(|s| s.checked_sub(self.latency));
        let continue_forward =
            next_output.is_some_and(|next| next <= index && index - next <= self.warmup);
        if !continue_forward {
            self.graph.reset();
            self.next_source = Some(index.saturating_sub(self.warmup));
        }

        let target_source = index + self.latency;
        while let Some(source) = self.next_source.filter(|&s| s <= target_source) {
            if cancel() {
                return Ok(None);
            }
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
        self.modulator.fill_block(
            start,
            end,
            &mut self.sources.get_mut(AUDIO_SOURCE).unwrap().data,
        );

        match self.graph.process(m as u64, &self.sources) {
            Ok(output) => {
                to_rgb8(output, &mut self.rgb);
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
