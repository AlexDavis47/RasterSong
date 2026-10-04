//! Renders the UI offscreen in a few states and saves PNGs to `target/tmp/screenshots/`, for
//! looking at layout changes without opening the app. Needs a GPU, so it doesn't run by default:
//!
//! ```sh
//! cargo test -p rastersong-gui --test screenshots -- --ignored
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rastersong_engine::{
    AudioClip, FakeBackend, FakeVideo, GraphDesc, Project, ProjectTrack, Rational,
};
use rastersong_gui::{App, AudioOut, STARTER_GRAPH, ThemeChoice};

fn app(theme: ThemeChoice) -> App {
    let backend = FakeBackend::new()
        .with_video(
            "clip",
            FakeVideo {
                width: 320,
                height: 180,
                frame_count: 120,
                frame_rate: Rational::new(30, 1),
            },
        )
        .with_audio(
            "song",
            AudioClip {
                sample_rate: 8000,
                channels: 1,
                samples: (0..24_000).map(|i| (i as f32 * 0.05).sin() * 0.8).collect(),
            },
        );
    let mut project = Project::new(GraphDesc::from_json(STARTER_GRAPH).unwrap());
    project.video = Some(PathBuf::from("clip"));
    project
        .audio_tracks
        .push(ProjectTrack::new("audio".into(), PathBuf::from("song")));
    let mut app = App::new(Arc::new(backend), project, None, AudioOut::silent(None));
    with_theme(&mut app, theme);
    app
}

fn with_theme(app: &mut App, theme: ThemeChoice) {
    let mut settings = app.settings().clone();
    settings.theme = theme;
    app.set_settings(settings);
}

fn gpu_harness(app: App) -> Harness<'static, App> {
    Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app)
}

fn save(harness: &mut Harness<'_, App>, name: &str) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("screenshots");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.png"));
    harness.render().unwrap().save(&path).unwrap();
    println!("saved {}", path.display());
}

fn click_at(harness: &mut Harness<'_, App>, pos: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(pos));
    harness.run_steps(1);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(1);
    }
    harness.run_steps(2);
}

#[test]
#[ignore = "needs a GPU; run explicitly to look at the UI"]
fn screenshots() {
    for (theme, name) in [(ThemeChoice::Dark, "dark"), (ThemeChoice::Light, "light")] {
        capture(theme, name);
    }
}

fn capture(theme: ThemeChoice, theme_name: &str) {
    // A new project with nothing loaded yet.
    let mut empty =
        App::with_starter_project(Arc::new(FakeBackend::new()), None, AudioOut::silent(None));
    with_theme(&mut empty, theme);
    let mut harness = gpu_harness(empty);
    harness.run_steps(4);
    save(&mut harness, &format!("{theme_name}-0-empty"));

    let mut harness = gpu_harness(app(theme));

    let deadline = Instant::now() + Duration::from_secs(20);
    while harness.state().engine().buffered_from(0) < 60 {
        assert!(Instant::now() < deadline, "timed out waiting for frames");
        harness.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    harness.run_steps(3);
    save(&mut harness, &format!("{theme_name}-1-loaded"));

    let split = harness
        .state()
        .editor()
        .key_of("three_band")
        .or(harness.state().editor().key_of("bands"))
        .unwrap();
    let header = harness
        .state()
        .editor()
        .node_screen_rect(split)
        .unwrap()
        .center_top()
        + egui::vec2(0.0, 8.0);
    click_at(&mut harness, header);
    harness.run_steps(4);
    save(&mut harness, &format!("{theme_name}-2-node-selected"));

    // Right-click empty canvas: the search menu.
    let canvas = harness.state().editor().canvas_rect();
    harness.event(egui::Event::PointerMoved(
        canvas.left_bottom() + egui::vec2(40.0, -120.0),
    ));
    harness.run_steps(1);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: canvas.left_bottom() + egui::vec2(40.0, -120.0),
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(1);
    }
    harness.event(egui::Event::Text("de".into()));
    harness.run_steps(3);
    save(&mut harness, &format!("{theme_name}-3-search"));
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);

    // Break the graph: pull the wire off the output and drop it on empty space.
    let out = harness.state().editor().key_of("out").unwrap();
    let pin = harness
        .state()
        .editor()
        .pin_screen_pos(out, true, 0)
        .unwrap();
    let canvas = harness.state().editor().canvas_rect();
    let drop = canvas.left_top() + egui::vec2(30.0, 30.0);
    harness.event(egui::Event::PointerMoved(pin));
    harness.run_steps(1);
    harness.event(egui::Event::PointerButton {
        pos: pin,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(1);
    for t in [0.3, 0.6, 1.0] {
        harness.event(egui::Event::PointerMoved(pin + (drop - pin) * t));
        harness.run_steps(1);
    }
    harness.event(egui::Event::PointerButton {
        pos: drop,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(2);
    harness.key_press(egui::Key::Escape);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !matches!(
        harness.state().engine().status(),
        rastersong_engine::EngineStatus::Failed(_)
    ) {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the failure"
        );
        harness.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    harness.run_steps(3);
    save(&mut harness, &format!("{theme_name}-4-error"));

    harness.get_by_label("Help").click();
    harness.run_steps(2);
    harness.get_by_label("About RasterSong").click();
    harness.run_steps(3);
    save(&mut harness, &format!("{theme_name}-5-about"));
}
