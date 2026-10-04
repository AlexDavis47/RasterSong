// Shared by several test binaries, each of which uses only some of these helpers.
#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rastersong_engine::sources::Modulator;
use rastersong_engine::{
    AudioClip, AudioTrack, FakeBackend, FakeVideo, GraphDesc, OutputSize, Rational, Registry,
    Renderer,
};

pub const FRAMES: usize = 60;
pub const VIDEO: &str = "clip";
pub const AUDIO: &str = "song";

pub fn audio() -> AudioClip {
    let sample_rate = 9000;
    AudioClip {
        sample_rate,
        channels: 1,
        samples: (0..sample_rate * 3)
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
pub const FINITE: &str = r#"{ "version": 1,
  "nodes": [
    { "id": "video", "type": "video_input" },
    { "id": "audio", "type": "audio_input" },
    { "id": "delay", "type": "delay", "params": { "time": 1.5, "depth": 0.5, "unit": "frames" }, "interpolation": "linear" },
    { "id": "crush", "type": "bitcrush", "params": { "bits": 5, "depth": 1 } },
    { "id": "out", "type": "output" }
  ],
  "connections": [
    { "from": "video", "to": "delay" }, { "from": "audio", "to": "delay.modulation" },
    { "from": "delay", "to": "crush" }, { "from": "audio", "to": "crush.modulation" },
    { "from": "crush", "to": "out" }
  ] }"#;

/// Video straight to the output.
pub const FINITE_PASSTHROUGH: &str = r#"{ "version": 1,
  "nodes": [ { "id": "video", "type": "video_input" }, { "id": "out", "type": "output" } ],
  "connections": [ { "from": "video", "to": "out" } ] }"#;

/// A graph with infinite memory (feedback and an IIR filter): seeking is approximate.
pub const INFINITE: &str = r#"{ "version": 1,
  "nodes": [
    { "id": "video", "type": "video_input" },
    { "id": "audio", "type": "audio_input" },
    { "id": "smooth", "type": "lowpass", "params": { "cutoff": 0.5, "depth": 1 } },
    { "id": "echo", "type": "delay", "params": { "time": 0.5, "unit": "frames", "feedback": 0.5, "mix": 0.5 } },
    { "id": "out", "type": "output" }
  ],
  "connections": [
    { "from": "video", "to": "smooth" }, { "from": "audio", "to": "smooth.modulation" },
    { "from": "smooth", "to": "echo" }, { "from": "echo", "to": "out" }
  ] }"#;

pub fn crush(bits: u32) -> String {
    format!(
        r#"{{ "version": 1,
  "nodes": [ {{ "id": "video", "type": "video_input" }}, {{ "id": "crush", "type": "bitcrush", "params": {{ "bits": {bits} }} }}, {{ "id": "out", "type": "output" }} ],
  "connections": [ {{ "from": "video", "to": "crush" }}, {{ "from": "crush", "to": "out" }} ] }}"#
    )
}

pub fn renderer_with(graph: &str, registry: &Registry, size: OutputSize) -> Renderer {
    Renderer::new(
        &backend(),
        Path::new(VIDEO),
        &[AudioTrack {
            name: "audio".into(),
            modulator: Arc::new(Modulator::new(&audio())),
            offset: 0.0,
        }],
        &GraphDesc::from_json(graph).unwrap(),
        registry,
        size,
    )
    .unwrap()
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
