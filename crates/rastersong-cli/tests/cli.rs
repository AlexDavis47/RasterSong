use std::process::Command;

#[test]
fn info_reports_lgpl_ffmpeg() {
    let output = Command::new(env!("CARGO_BIN_EXE_rastersong-cli"))
        .arg("info")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("LGPL"), "{stdout}");
    assert!(stdout.contains("avcodec"), "{stdout}");
}

#[test]
fn a_graph_with_an_audio_output_exports_its_sound() {
    use rastersong_engine::{AudioOptions, FfmpegBackend, MediaBackend};

    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture = |name: &str| workspace.join("fixtures").join(name);
    if !fixture("rgb_pattern.mkv").exists() {
        panic!("missing fixtures; run `cargo xtask fixtures`");
    }
    let tmp = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let graph = tmp.join("silencing.json");
    // The music, turned all the way down: a silent, rendered track at 24 kHz.
    std::fs::write(
        &graph,
        r#"{ "version": 0,
          "nodes": [
            { "id": "video", "type": "video_input" }, { "id": "audio", "type": "audio_input" },
            { "id": "mute", "type": "gain", "params": { "gain": -120 } },
            { "id": "sound", "type": "audio_output" }, { "id": "out", "type": "output" }
          ],
          "connections": [
            { "from": "video", "to": "out" }, { "from": "audio", "to": "mute" }, { "from": "mute", "to": "sound" }
          ] }"#,
    )
    .unwrap();
    let out = tmp.join("rendered_sound.mkv");
    let status = Command::new(env!("CARGO_BIN_EXE_rastersong-cli"))
        .arg("render")
        .arg(fixture("rgb_pattern.mkv"))
        .arg(fixture("music.wav"))
        .arg(&graph)
        .arg(&out)
        .args(["--frames", "12", "--audio-rate", "24000"])
        .status()
        .unwrap();
    assert!(status.success());

    let backend = FfmpegBackend::new().unwrap();
    let fps = backend.open_video(&out).unwrap().info().frame_rate.as_f64();
    let audio = backend.load_audio(&out, AudioOptions::default()).unwrap();
    assert_eq!(audio.sample_rate, 24_000);
    let expected = (12.0 / fps * 24_000.0).round() as usize;
    assert!(
        audio.frames().abs_diff(expected) <= 1,
        "{} vs {expected}",
        audio.frames()
    );
    assert!(
        audio.samples.iter().all(|&s| s.abs() < 1e-3),
        "the sound is the graph's"
    );
}
