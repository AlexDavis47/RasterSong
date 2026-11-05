// Shared by several test binaries, each of which uses only some of these helpers.
#![allow(dead_code)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use rastersong_engine::sources::Modulator;
use rastersong_engine::{
    AudioClip, AudioResources, Bus, Composition, EngineError, FakeBackend, FakeVideo, GraphDesc,
    OutputSize, Project, Rational, Registry, Renderer, TrackKind,
};

pub const FRAMES: usize = 60;
pub const VIDEO: &str = "clip";
pub const AUDIO: &str = "song";

/// The song: as long as the video, so the project is the video's length.
pub fn audio() -> AudioClip {
    let sample_rate = 9000;
    AudioClip {
        sample_rate,
        channels: 1,
        samples: (0..sample_rate * 2)
            .map(|i| (i as f32 * 0.013).sin() * 0.8)
            .collect(),
    }
}

pub fn backend() -> FakeBackend {
    FakeBackend::new()
        .with_video(
            VIDEO,
            FakeVideo {
                width: 16,
                height: 8,
                frame_count: FRAMES,
                frame_rate: Rational::new(30, 1),
            },
        )
        .with_audio(AUDIO, audio())
}

/// A graph whose nodes all have finite memory, so seeking with warmup is exact.
pub const FINITE: &str = r#"{ "version": 0,
  "nodes": [
    { "id": "video", "type": "video_input" },
    { "id": "audio", "type": "audio_input" },
    { "id": "delay", "type": "delay", "params": { "time": 1.5, "time.unit": "frame" }, "modulation": { "time": { "amount": 5, "mode": "bipolar" } }, "interpolation": "linear" },
    { "id": "crush", "type": "bitcrush", "params": { "bits": 5 }, "modulation": { "bits": { "amount": 20, "mode": "bipolar" } } },
    { "id": "out", "type": "output" }
  ],
  "connections": [
    { "from": "video", "to": "delay" }, { "from": "audio", "to": "delay.@time" },
    { "from": "delay", "to": "crush" }, { "from": "audio", "to": "crush.@bits" },
    { "from": "crush", "to": "out" }
  ] }"#;

/// Video straight to the output.
pub const FINITE_PASSTHROUGH: &str = r#"{ "version": 0,
  "nodes": [ { "id": "video", "type": "video_input" }, { "id": "out", "type": "output" } ],
  "connections": [ { "from": "video", "to": "out" } ] }"#;

/// A graph with infinite memory (feedback and an IIR filter): seeking is approximate.
pub const INFINITE: &str = r#"{ "version": 0,
  "nodes": [
    { "id": "video", "type": "video_input" },
    { "id": "audio", "type": "audio_input" },
    { "id": "smooth", "type": "filter", "params": { "cutoff": 0.5, "slope": "6" }, "modulation": { "cutoff": { "amount": 20, "mode": "bipolar" } } },
    { "id": "echo", "type": "delay", "params": { "time": 0.5, "time.unit": "frame", "feedback": 0.5, "mix": 0.5 } },
    { "id": "out", "type": "output" }
  ],
  "connections": [
    { "from": "video", "to": "smooth" }, { "from": "audio", "to": "smooth.@cutoff" },
    { "from": "smooth", "to": "echo" }, { "from": "echo", "to": "out" }
  ] }"#;

pub fn crush(bits: u32) -> String {
    format!(
        r#"{{ "version": 0,
  "nodes": [ {{ "id": "video", "type": "video_input" }}, {{ "id": "crush", "type": "bitcrush", "params": {{ "bits": {bits} }} }}, {{ "id": "out", "type": "output" }} ],
  "connections": [ {{ "from": "video", "to": "crush" }}, {{ "from": "crush", "to": "out" }} ] }}"#
    )
}

/// How long the clip lasts, in seconds.
pub const SECONDS: f64 = FRAMES as f64 / 30.0;

/// What `rastersong render clip song <graph>` renders: the clip on the track `Video`, the song
/// on the track `Audio`, and `graph` over the whole clip.
pub fn project(graph: &str) -> Project {
    Project::from_files(
        VIDEO,
        AUDIO,
        GraphDesc::from_json(graph).unwrap(),
        0.0,
        SECONDS,
    )
}

/// The sound of every audio resource of `composition`: the clip of `clips` its file is named
/// in, or the song.
pub fn decoded(composition: &Composition, clips: &[(&str, &AudioClip)]) -> AudioResources {
    composition
        .resources
        .iter()
        .filter(|r| r.kind() == TrackKind::Audio)
        .map(|r| {
            let path = r.file().unwrap().path.to_string_lossy().into_owned();
            let song = audio();
            let clip = clips
                .iter()
                .find(|(p, _)| *p == path)
                .map_or(&song, |(_, c)| *c);
            (r.id, Arc::new(Modulator::new(clip)))
        })
        .collect()
}

/// A renderer of `project`, its audio files' sound from `clips` (the song otherwise), with the
/// sound of `bus`.
pub fn renderer_of(
    project: &Project,
    clips: &[(&str, &AudioClip)],
    bus: &Bus,
    registry: &Registry,
    size: OutputSize,
) -> Result<Renderer, EngineError> {
    let composition = project.composition();
    Renderer::new(
        &backend(),
        &composition,
        &decoded(&composition, clips),
        None,
        Default::default(),
        bus,
        registry,
        size,
    )
}

pub fn renderer_with(graph: &str, registry: &Registry, size: OutputSize) -> Renderer {
    renderer_of(&project(graph), &[], &Bus::main(), registry, size).unwrap()
}

pub fn renderer(graph: &str) -> Renderer {
    renderer_with(graph, &Registry::default(), OutputSize::Native)
}

/// Every frame rendered in order from the start: the exact reference.
pub fn sequential(graph: &str, size: OutputSize) -> Vec<Vec<u8>> {
    let mut r = renderer_with(graph, &Registry::default(), size);
    (0..FRAMES)
        .map(|i| r.render(i, &|| false).unwrap().unwrap().to_vec())
        .collect()
}

/// Polls `condition` until it holds, failing the test after a generous timeout.
pub fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}
