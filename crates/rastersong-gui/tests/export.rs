//! The Export window, driven headlessly: the fake media backend renders, the real FFmpeg writes.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rastersong_engine::{
    AudioClip, AudioOptions, FakeBackend, FakeVideo, FfmpegBackend, GraphDesc, LoopRegion,
    MediaBackend, Project, Rational, Timebase, TrackKind,
};
use rastersong_gui::{App, AudioOut, ExportState, STARTER_GRAPH};

/// The starter project over a fake video of `frames` frames at 64×36, 30 fps, and a song.
fn app(frames: usize) -> App {
    app_with(frames, |_| {})
}

/// [`app`] with the project changed by `edit`.
fn app_with(frames: usize, edit: impl FnOnce(&mut Project)) -> App {
    let backend = FakeBackend::new()
        .with_video(
            "clip",
            FakeVideo {
                width: 64,
                height: 36,
                frame_count: frames,
                frame_rate: Rational::new(30, 1),
            },
        )
        .with_audio(
            "song",
            AudioClip {
                sample_rate: 8000,
                channels: 1,
                samples: (0..16_000).map(|i| (i as f32 * 0.05).sin()).collect(),
            },
        );
    let mut project = Project::new(GraphDesc::from_json(STARTER_GRAPH).unwrap());
    project.add_track(TrackKind::Video, "video", "clip");
    project.add_track(TrackKind::Audio, "audio", "song");
    edit(&mut project);
    App::new(Arc::new(backend), project, None, AudioOut::silent(None))
}

fn harness(app: App) -> Harness<'static, App> {
    Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app)
}

/// Steps the UI until the export is no longer running, and returns how it ended.
fn finish(harness: &mut Harness<'_, App>) -> ExportState {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        harness.step();
        let state = harness.state().export_state();
        if !matches!(state, ExportState::Running { .. }) {
            return state;
        }
        assert!(Instant::now() < deadline, "the export never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn temp_file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}

#[test]
fn the_file_menu_opens_the_export_window() {
    let mut harness = harness(app(60));
    harness.run_steps(2);
    harness.get_by_label("File").click();
    harness.run_steps(2);
    // The menu's item, not the top bar's button beside it.
    harness.get_all_by_label("Export…").last().unwrap().click();
    harness.run_steps(2);
    assert!(harness.query_by_label("FORMAT").is_some());
    assert!(harness.query_by_label("SOUND").is_some());
    // The project's size, in the Size row and the summary.
    assert_eq!(harness.get_all_by_label("64×36").count(), 2);
    // Nothing rendered yet, and the summary of what will be.
    assert!(harness.query_by_label("Not started yet.").is_some());
    harness.get_by_label("0:00.00 – 0:02.00 · 60 frames");
    harness.get_by_label("30 fps");
    harness.get_by_label("none");
    // Only formats that work here are offered; ProRes always does.
    assert!(
        rastersong_engine::export_formats()
            .iter()
            .any(|f| f.kind == rastersong_engine::FormatKind::ProRes)
    );
}

#[test]
fn the_top_bar_export_button_opens_the_export_window() {
    let mut harness = harness(app(60));
    harness.run_steps(2);
    harness.get_by_label("Export…").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("FORMAT").is_some());
    // Close hides the window.
    harness.get_by_label("Close").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("FORMAT").is_none());
}

#[test]
fn an_export_writes_the_whole_project() {
    // 45 frames of video under 2 s of song: the project lasts as long as the song.
    let mut harness = harness(app(45));
    harness.state_mut().open_export();
    harness.run_steps(2);
    // The default format is H.264, or the first that works here when no H.264 encoder does.
    let kind = rastersong_engine::export_formats()
        .iter()
        .find(|f| f.kind == rastersong_engine::FormatKind::H264)
        .unwrap_or(&rastersong_engine::export_formats()[0])
        .kind;
    let path = temp_file(&format!("whole_project.{}", kind.extension()));
    let _ = std::fs::remove_file(&path);
    harness.state_mut().start_export(path.clone());
    assert_eq!(finish(&mut harness), ExportState::Done(path.clone()));
    harness.run_steps(2);
    assert!(
        harness.query_by_label_contains("Exported").is_some(),
        "the window says where the file went"
    );
    // The latest frame written stays in the preview, labelled with its frame and time.
    let frame = harness.state().export_frame().expect("a frame was shown");
    assert!(frame < 60);
    harness.get_by_label_contains(&format!("frame {frame} · 0:"));
    harness.get_by_label(&format!("whole_project.{}", kind.extension()));

    let backend = FfmpegBackend::new().unwrap();
    let mut video = backend.open_video(&path).unwrap();
    assert_eq!(
        (
            video.info().width,
            video.info().height,
            video.info().frame_count
        ),
        (64, 36, 60)
    );
    video.frame(59).unwrap();
    assert!(backend.has_audio(&path), "the master bus's sound");
}

#[test]
fn cancel_stops_the_export_and_removes_the_file() {
    // Long enough that Cancel comes long before the end.
    let mut harness = harness(app(100_000));
    harness.state_mut().open_export();
    harness.run_steps(2);
    let path = temp_file("cancelled.mkv");
    harness.state_mut().start_export(path.clone());
    harness.run_steps(2);
    assert!(matches!(
        harness.state().export_state(),
        ExportState::Running { .. }
    ));
    harness.get_by_label("Cancel").click();
    assert_eq!(finish(&mut harness), ExportState::Cancelled);
    assert!(!path.exists());
    harness.run_steps(2);
    assert!(
        harness
            .query_by_label("The export was cancelled.")
            .is_some()
    );
}

#[test]
fn the_loop_region_can_be_exported_alone() {
    let mut harness = harness(app_with(60, |project| {
        project.loop_region = Some(LoopRegion {
            start: 0.5,
            end: 1.0,
            enabled: false,
        });
    }));
    harness.state_mut().open_export();
    harness.run_steps(2);
    harness.get_by_label("0:00.00 – 0:02.00 · 60 frames");
    harness.get_by_label("Loop region").click();
    harness.run_steps(2);
    harness.get_by_label("0:00.50 – 0:01.00 · 15 frames");
    harness.get_by_label("0:00.50");

    let path = temp_file("region.mkv");
    harness.state_mut().start_export(path.clone());
    assert_eq!(finish(&mut harness), ExportState::Done(path.clone()));

    let backend = FfmpegBackend::new().unwrap();
    // 0.5 s to 1 s at 30 fps.
    assert_eq!(backend.open_video(&path).unwrap().info().frame_count, 15);
    let audio = backend.load_audio(&path, AudioOptions::default()).unwrap();
    let seconds = audio.frames() as f64 / f64::from(audio.sample_rate);
    assert!((seconds - 0.5).abs() < 0.01, "{seconds} s of sound");
}

#[test]
fn an_odd_size_says_which_side_and_offers_the_nearest_even_size() {
    let mut harness = harness(app_with(60, |project| {
        project.timebase = Some(Timebase {
            width: 63,
            height: 36,
            frame_rate: Rational::new(30, 1),
        });
    }));
    harness.state_mut().open_export();
    harness.run_steps(2);
    harness.get_by_label("H.264 needs an even width; 63 is odd.");
    assert!(harness.query_by_label("Export… (fix size first)").is_some());
    harness.get_by_label("Use 64×36").click();
    harness.run_steps(2);
    assert!(harness.query_by_label_contains("needs an even").is_none());
    assert!(harness.query_by_label("Use 64×36").is_none());
}
