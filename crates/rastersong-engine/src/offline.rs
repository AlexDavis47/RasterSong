//! Offline rendering: every frame in order from the start, as fast as possible. Used by export
//! ([`crate::export`]) and the CLI.

use std::ops::Range;

use rastersong_graph::{GraphDesc, Registry, Tempo};
use rastersong_media::MediaBackend;

use crate::timeline::{Bus, Timebase};
use crate::{
    AudioBlock, AudioSink, Composition, DEFAULT_AUDIO_RATE, EngineError, MAX_WARMUP_FRAMES_LIMIT,
    OutputSize, RenderInfo, RenderTrack, Renderer,
};

#[derive(Debug, Clone, Default)]
pub struct RenderSettings {
    /// The project's size and frame rate. `None` takes them from the first video track.
    pub timebase: Option<Timebase>,
    /// Processing and output size. `None` uses the project's size.
    pub size: Option<(u32, u32)>,
    /// The frames to write, `start..end`, cut to the project's length; `None` is every frame.
    /// Frames before the start are rendered first only as far as the graph needs history (from
    /// the first frame when its history is unbounded), so the result is exact.
    pub range: Option<Range<usize>>,
    /// The project tempo, for beat and bar units.
    pub tempo: Tempo,
    /// The rate the graph's sound is rendered at; `None` is [`DEFAULT_AUDIO_RATE`].
    pub audio_rate: Option<u32>,
    /// The output bus whose sound is rendered.
    pub bus: Bus,
    /// The project's composition ([`crate::Project::composition`]), with `graph` the open
    /// graph's description. `None` renders `graph` over the whole timeline, its ports named
    /// after tracks reading them.
    pub composition: Option<Composition>,
}

/// One rendered frame, as packed RGB8. `index` is its frame in the project, so a range starts
/// past 0.
#[derive(Debug)]
pub struct RenderedFrame<'a> {
    pub index: usize,
    pub rgb: &'a [u8],
    /// The frame's rendered sound, when the render's audio is [`AudioSink::Rendered`].
    pub audio: Option<&'a AudioBlock>,
}

/// Where rendered frames go.
pub trait FrameSink {
    /// Called once before the first frame, e.g. to create the output file. `audio` says whether
    /// the sound is the audio mix or rendered with each frame.
    fn start(&mut self, info: &RenderInfo, audio: &AudioSink) -> Result<(), EngineError>;

    /// Called for each frame, in order.
    fn frame(&mut self, frame: RenderedFrame) -> Result<(), EngineError>;
}

/// Renders `tracks` through `graph` (or `settings.composition`) into `sink`: every frame, or
/// `settings.range`, exactly. `progress` hears the frames rendered and the total after each
/// frame (warmup before the range included), and cancels the render with
/// [`EngineError::Cancelled`] by returning `false`. The returned info's `frames` is the number
/// of frames written.
pub fn render(
    backend: &dyn MediaBackend,
    tracks: &[RenderTrack],
    graph: &GraphDesc,
    settings: &RenderSettings,
    sink: &mut dyn FrameSink,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<RenderInfo, EngineError> {
    let size = settings
        .size
        .map_or(OutputSize::Native, |(w, h)| OutputSize::Exact(w, h));
    let mut renderer = match &settings.composition {
        Some(composition) => Renderer::with_composition(
            backend,
            settings.timebase,
            tracks,
            composition,
            graph,
            settings.tempo,
            &settings.bus,
            Registry::shared(),
            size,
        ),
        None => Renderer::new(
            backend,
            settings.timebase,
            tracks,
            graph,
            settings.tempo,
            &settings.bus,
            Registry::shared(),
            size,
        ),
    }?;
    renderer.set_audio_rate(settings.audio_rate.unwrap_or(DEFAULT_AUDIO_RATE));
    // Warm up as far as the graph remembers, however far that is.
    renderer.set_max_warmup_frames(MAX_WARMUP_FRAMES_LIMIT);
    let mut info = *renderer.info();
    let range = settings.range.clone().unwrap_or(0..info.frames);
    let range = range.start.min(info.frames)..range.end.min(info.frames);
    // A graph whose history is unbounded (feedback) is exact only rendered from the start.
    let unbounded = renderer.warmup_frames() >= MAX_WARMUP_FRAMES_LIMIT as usize;
    let first = if unbounded { 0 } else { range.start };
    info.frames = range.len();
    sink.start(&info, &renderer.audio_sink())?;
    let total = range.end.saturating_sub(first);
    for index in first..range.end {
        let _frame = tracing::trace_span!("frame", index).entered();
        {
            let _span = tracing::trace_span!("render").entered();
            renderer
                .render(index, &|| false)?
                .expect("offline renders are never cancelled");
        }
        if index >= range.start {
            let _span = tracing::trace_span!("write").entered();
            let (rgb, audio) = renderer.output();
            sink.frame(RenderedFrame { index, rgb, audio })?;
        }
        if !progress(index + 1 - first, total) {
            return Err(EngineError::Cancelled);
        }
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rastersong_media::{AudioClip, FakeBackend, FakeVideo, Rational};

    use crate::sources::Modulator;

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
            samples: vec![0.0; 100].into(),
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
        fn start(&mut self, info: &RenderInfo, _: &AudioSink) -> Result<(), EngineError> {
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

    const PASSTHROUGH: &str = r#"{ "version": 0,
        "nodes": [ { "id": "v", "type": "video_input" }, { "id": "o", "type": "output" } ],
        "connections": [ { "from": "v", "to": "o" } ] }"#;

    fn run(settings: &RenderSettings, sink: &mut Recorder) -> Result<RenderInfo, EngineError> {
        let graph = GraphDesc::from_json(PASSTHROUGH).unwrap();
        let tracks = [
            RenderTrack::video(crate::VIDEO_SOURCE, "clip"),
            RenderTrack::audio(
                crate::DEFAULT_AUDIO_TRACK,
                Arc::new(Modulator::new(&silence())),
                0.0,
            ),
        ];
        render(&backend(), &tracks, &graph, settings, sink, &mut |_, _| {
            true
        })
    }

    #[test]
    fn a_range_writes_only_its_frames() {
        let settings = RenderSettings {
            range: Some(3..6),
            ..Default::default()
        };
        let mut sink = Recorder::default();
        let info = run(&settings, &mut sink).unwrap();
        assert_eq!(info.frames, 3);
        assert_eq!(sink.info.unwrap().frames, 3);
        // The fake video's red channel is the frame index.
        assert_eq!(
            sink.frames,
            [(3, 24, 3), (4, 24, 4), (5, 24, 5)],
            "the frames of the whole render, from 3"
        );
        // A range past the end is cut to the project.
        let settings = RenderSettings {
            range: Some(8..50),
            ..Default::default()
        };
        let mut sink = Recorder::default();
        assert_eq!(run(&settings, &mut sink).unwrap().frames, 2);
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
            range: Some(0..3),
            ..Default::default()
        };
        let mut sink = Recorder::default();
        run(&settings, &mut sink).unwrap();
        assert_eq!(sink.frames.len(), 3);
        assert!(sink.frames.iter().all(|&(_, len, _)| len == 2 * 2 * 3));
    }

    #[test]
    fn progress_hears_every_frame_and_can_cancel() {
        let graph = GraphDesc::from_json(PASSTHROUGH).unwrap();
        let tracks = [RenderTrack::video(crate::VIDEO_SOURCE, "clip")];
        let mut heard = Vec::new();
        let mut sink = Recorder::default();
        render(
            &backend(),
            &tracks,
            &graph,
            &RenderSettings::default(),
            &mut sink,
            &mut |done, total| {
                heard.push((done, total));
                true
            },
        )
        .unwrap();
        assert_eq!(heard, (1..=10).map(|i| (i, 10)).collect::<Vec<_>>());

        let mut sink = Recorder::default();
        let result = render(
            &backend(),
            &tracks,
            &graph,
            &RenderSettings::default(),
            &mut sink,
            &mut |done, _| done < 3,
        );
        assert!(matches!(result, Err(EngineError::Cancelled)));
        assert_eq!(sink.frames.len(), 3);
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
