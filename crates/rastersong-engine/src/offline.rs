//! Offline rendering: every frame in order from the start, as fast as possible. Used by the CLI
//! now, and by export later.

use std::collections::HashMap;
use std::path::Path;

use rastersong_graph::{CompileOptions, Graph, GraphDesc, GraphError, Layout, Registry, Signal};
use rastersong_media::{AudioClip, MediaBackend, MediaError, Rational};

use crate::sources::{Modulator, fill_video, to_rgb8};

/// Source names the renderer supplies to the graph.
pub const VIDEO_SOURCE: &str = "video";
pub const AUDIO_SOURCE: &str = "audio";

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Graph(#[from] GraphError),
    /// Raised by the frame callback, e.g. when writing the output fails.
    #[error("{0}")]
    Output(String),
}

#[derive(Debug, Clone, Default)]
pub struct RenderSettings {
    /// Processing and output size. `None` uses the video's display size.
    pub size: Option<(u32, u32)>,
    /// Render only the first `frames` frames.
    pub frames: Option<usize>,
}

/// What a render will produce, known before the first frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderInfo {
    pub width: u32,
    pub height: u32,
    pub frame_rate: Rational,
    pub frames: usize,
}

/// One rendered frame, as packed RGB8.
#[derive(Debug)]
pub struct RenderedFrame<'a> {
    pub index: usize,
    pub rgb: &'a [u8],
}

/// Where rendered frames go.
pub trait FrameSink {
    /// Called once before the first frame, e.g. to create the output file.
    fn start(&mut self, info: &RenderInfo) -> Result<(), EngineError>;

    /// Called for each frame, in order.
    fn frame(&mut self, frame: RenderedFrame) -> Result<(), EngineError>;
}

/// Renders `video` modulated by `audio` through `graph` into `sink`.
pub fn render(
    backend: &dyn MediaBackend,
    video_path: &Path,
    audio: &AudioClip,
    graph: &GraphDesc,
    settings: &RenderSettings,
    sink: &mut dyn FrameSink,
) -> Result<RenderInfo, EngineError> {
    let mut video = backend.open_video(video_path)?;
    video.set_output_size(settings.size);
    let source_info = video.info().clone();
    let (width, height) = settings
        .size
        .unwrap_or((source_info.width, source_info.height));
    let fps = source_info.frame_rate.as_f64();
    let frames = settings
        .frames
        .map_or(source_info.frame_count, |n| n.min(source_info.frame_count));

    let modulator = Modulator::new(audio);
    let video_layout = Layout::rgb(width, height);
    let audio_layout = Layout::audio(modulator.block_len(fps));
    let mut graph = Graph::compile(
        graph,
        &Registry::default(),
        &CompileOptions {
            frame_rate: fps,
            sources: HashMap::from([
                (VIDEO_SOURCE.to_owned(), video_layout),
                (AUDIO_SOURCE.to_owned(), audio_layout),
            ]),
            output: video_layout,
        },
    )?;

    let info = RenderInfo {
        width,
        height,
        frame_rate: source_info.frame_rate,
        frames,
    };
    sink.start(&info)?;
    if frames == 0 {
        return Ok(info);
    }

    // Frame start times; past the end (latency pre-roll), frames continue at the nominal rate.
    let last = frames - 1;
    let time = |n: usize| {
        if n <= last {
            video.frame_time(n)
        } else {
            video.frame_time(last) + (n - last) as f64 / fps
        }
    };
    let times: Vec<f64> = (0..=frames + graph.latency_frames() as usize)
        .map(time)
        .collect();

    let latency = graph.latency_frames() as usize;
    let mut sources = HashMap::from([
        (VIDEO_SOURCE.to_owned(), Signal::zeros(video_layout)),
        (AUDIO_SOURCE.to_owned(), Signal::zeros(audio_layout)),
    ]);
    let mut rgb = Vec::with_capacity(video_layout.len());
    let mut have_video = false;

    // Render `latency` extra frames (repeating the last source frame) so every output frame
    // comes out; the first `latency` outputs are pre-roll.
    for n in 0..frames + latency {
        match video.frame(n.min(last)) {
            Ok(frame) => {
                fill_video(&frame, sources.get_mut(VIDEO_SOURCE).unwrap());
                have_video = true;
            }
            // A damaged frame repeats the previous one rather than failing the whole render.
            Err(MediaError::FrameUnavailable(i)) if have_video => {
                tracing::warn!(
                    frame = i,
                    "frame could not be decoded; repeating the previous frame"
                );
            }
            Err(e) => return Err(e.into()),
        }
        modulator.fill_block(
            times[n],
            times[n + 1],
            &mut sources.get_mut(AUDIO_SOURCE).unwrap().data,
        );

        let output = graph.process(n as u64, &sources)?;
        if n >= latency {
            to_rgb8(output, &mut rgb);
            sink.frame(RenderedFrame {
                index: n - latency,
                rgb: &rgb,
            })?;
        }
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use rastersong_media::{FakeBackend, FakeVideo};

    use super::*;

    fn backend() -> FakeBackend {
        FakeBackend::new().with_video(
            "clip",
            FakeVideo {
                width: 4,
                height: 2,
                frame_count: 10,
                frame_rate: Rational::new(10, 1),
            },
        )
    }

    fn silence() -> AudioClip {
        AudioClip {
            sample_rate: 100,
            channels: 1,
            samples: vec![0.0; 100],
        }
    }

    /// Records the first byte of every frame.
    #[derive(Default)]
    struct Recorder {
        info: Option<RenderInfo>,
        frames: Vec<(usize, usize, u8)>,
        fail: bool,
    }

    impl FrameSink for Recorder {
        fn start(&mut self, info: &RenderInfo) -> Result<(), EngineError> {
            self.info = Some(*info);
            Ok(())
        }

        fn frame(&mut self, frame: RenderedFrame) -> Result<(), EngineError> {
            if self.fail {
                return Err(EngineError::Output("disk full".into()));
            }
            self.frames
                .push((frame.index, frame.rgb.len(), frame.rgb[0]));
            Ok(())
        }
    }

    const PASSTHROUGH: &str = r#"{ "version": 1,
        "nodes": [ { "id": "v", "type": "video_input" }, { "id": "o", "type": "output" } ],
        "connections": [ { "from": "v", "to": "o" } ] }"#;

    fn run(settings: &RenderSettings, sink: &mut Recorder) -> Result<RenderInfo, EngineError> {
        let graph = GraphDesc::from_json(PASSTHROUGH).unwrap();
        render(
            &backend(),
            Path::new("clip"),
            &silence(),
            &graph,
            settings,
            sink,
        )
    }

    #[test]
    fn renders_every_frame_in_order() {
        let mut sink = Recorder::default();
        let info = run(&RenderSettings::default(), &mut sink).unwrap();
        assert_eq!((info.width, info.height, info.frames), (4, 2, 10));
        assert_eq!(sink.info, Some(info));
        // The fake video's red channel is the frame index.
        let expected: Vec<_> = (0..10).map(|i| (i, 4 * 2 * 3, i as u8)).collect();
        assert_eq!(sink.frames, expected);
    }

    #[test]
    fn honors_size_and_frame_limit() {
        let settings = RenderSettings {
            size: Some((2, 2)),
            frames: Some(3),
        };
        let mut sink = Recorder::default();
        run(&settings, &mut sink).unwrap();
        assert_eq!(sink.frames.len(), 3);
        assert!(sink.frames.iter().all(|&(_, len, _)| len == 2 * 2 * 3));
    }

    #[test]
    fn sink_errors_stop_the_render() {
        let mut sink = Recorder {
            fail: true,
            ..Default::default()
        };
        let result = run(&RenderSettings::default(), &mut sink);
        assert!(matches!(result, Err(EngineError::Output(_))));
    }
}
