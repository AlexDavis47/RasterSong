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

#[test]
fn a_project_with_graph_items_renders_them() {
    use rastersong_engine::{FfmpegBackend, GraphDesc, MediaBackend, Project, TrackKind};

    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let video = workspace.join("fixtures").join("rgb_pattern.mkv");
    if !video.exists() {
        panic!("missing fixtures; run `cargo xtask fixtures`");
    }
    let tmp = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let backend = FfmpegBackend::new().unwrap();
    let fps = backend
        .open_video(&video)
        .unwrap()
        .info()
        .frame_rate
        .as_f64();

    let mut project = Project::new(
        GraphDesc::from_json(
            r#"{ "version": 0,
              "nodes": [ { "id": "v", "type": "video_input" }, { "id": "o", "type": "output" } ],
              "connections": [ { "from": "v", "to": "o" } ] }"#,
        )
        .unwrap(),
    );
    project.add_track(TrackKind::Video, "video", &video);
    let plain = tmp.join("plain.rastersong");
    project.save(&plain).unwrap();

    // The graph, unbound, on frames 2 and 3: black there.
    project.add_layer("Layer");
    let graph = project.graph_id;
    project.place_graph(0, graph, 2.0 / fps, 2.0 / fps).unwrap();
    project.set_graph_binding(0, 0, "v", None);
    let layered = tmp.join("layered.rastersong");
    project.save(&layered).unwrap();

    let render = |project: &std::path::Path, name: &str| {
        let out = tmp.join(name);
        let status = Command::new(env!("CARGO_BIN_EXE_rastersong-cli"))
            .arg("render")
            .arg(project)
            .arg(&out)
            .args(["--frames", "6"])
            .status()
            .unwrap();
        assert!(status.success());
        let mut video = backend.open_video(&out).unwrap();
        (0..6)
            .map(|i| video.frame(i).unwrap().data.to_vec())
            .collect::<Vec<_>>()
    };
    let (plain, layered) = (render(&plain, "plain.mkv"), render(&layered, "layered.mkv"));
    for i in 0..6 {
        if (2..4).contains(&i) {
            assert!(layered[i].iter().all(|&b| b == 0), "frame {i} is black");
            assert!(plain[i].iter().any(|&b| b != 0));
        } else {
            assert_eq!(layered[i], plain[i], "frame {i}");
        }
    }
}
