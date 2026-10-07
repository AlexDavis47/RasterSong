//! Offline rendering: every frame in order from the start, as fast as possible. Used by the CLI
//! now, and by export later.

use std::path::Path;
use std::sync::Arc;

use rastersong_graph::{GraphDesc, Registry, Tempo};
use rastersong_media::{AudioClip, MediaBackend};

use crate::sources::Modulator;
use crate::{
    AudioBlock, AudioSink, AudioTrack, DEFAULT_AUDIO_RATE, DEFAULT_AUDIO_TRACK, EngineError,
    OutputSize, RenderInfo, Renderer,
};

#[derive(Debug, Clone, Default)]
pub struct RenderSettings {
    /// Processing and output size. `None` uses the video's display size.
    pub size: Option<(u32, u32)>,
    /// Render only the first `frames` frames.
    pub frames: Option<usize>,
    /// Seconds the audio starts after the video.
    pub audio_offset: f64,
    /// The project tempo, for beat and bar units.
    pub tempo: Tempo,
    /// The rate the graph's sound is rendered at; `None` is [`DEFAULT_AUDIO_RATE`].
    pub audio_rate: Option<u32>,
}

/// One rendered frame, as packed RGB8.
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
    /// the sound is the source audio, a track passed through, or rendered with each frame.
    fn start(&mut self, info: &RenderInfo, audio: &AudioSink) -> Result<(), EngineError>;

    /// Called for each frame, in order.
    fn frame(&mut self, frame: RenderedFrame) -> Result<(), EngineError>;
}

/// Renders `video` modulated by `audio` through `graph` into `sink`, starting from frame 0 so
/// the result is exact.
pub fn render(
    backend: &dyn MediaBackend,
    video_path: &Path,
    audio: &AudioClip,
    graph: &GraphDesc,
    settings: &RenderSettings,
    sink: &mut dyn FrameSink,
) -> Result<RenderInfo, EngineError> {
    let track = AudioTrack {
        name: DEFAULT_AUDIO_TRACK.to_owned(),
        modulator: Arc::new(Modulator::new(audio)),
        offset: settings.audio_offset,
    };
    let mut renderer = Renderer::new(
        backend,
        video_path,
        &[track],
        graph,
        settings.tempo,
        Registry::shared(),
        settings
            .size
            .map_or(OutputSize::Native, |(w, h)| OutputSize::Exact(w, h)),
    )?;
    renderer.set_audio_rate(settings.audio_rate.unwrap_or(DEFAULT_AUDIO_RATE));
    let mut info = *renderer.info();
    if let Some(limit) = settings.frames {
        info.frames = info.frames.min(limit);
    }
    sink.start(&info, &renderer.audio_sink())?;
    for index in 0..info.frames {
        renderer
            .render(index, &|| false)?
            .expect("offline renders are never cancelled");
        let (rgb, audio) = renderer.output();
        sink.frame(RenderedFrame { index, rgb, audio })?;
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use rastersong_media::{FakeBackend, FakeVideo, Rational};

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
            ..Default::default()
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
