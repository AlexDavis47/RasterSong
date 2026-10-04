//! Interaction tests on the real UI, driven headlessly with the fake media backend and raw
//! pointer and keyboard events.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rastersong_engine::{
    AudioClip, FakeBackend, FakeVideo, GraphDesc, Project, ProjectTrack, Rational,
};
use rastersong_gui::{App, AudioOut, STARTER_GRAPH};

fn app() -> App {
    let backend = FakeBackend::new()
        .with_video(
            "clip",
            FakeVideo {
                width: 64,
                height: 36,
                frame_count: 60,
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
    project.video = Some(PathBuf::from("clip"));
    project
        .audio_tracks
        .push(ProjectTrack::new("audio".into(), PathBuf::from("song")));
    App::new(Arc::new(backend), project, None, AudioOut::silent(None))
}

fn harness(app: App) -> Harness<'static, App> {
    Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app)
}

/// Steps the UI until `condition` holds; rendering happens on the engine's thread.
fn step_until(harness: &mut Harness<'_, App>, what: &str, condition: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !condition(harness.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        harness.step();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn loaded() -> Harness<'static, App> {
    let mut harness = harness(app());
    step_until(&mut harness, "rendered frames", |app| {
        app.engine().buffered_from(0) >= 30
    });
    harness.run_steps(2);
    harness
}

fn key(harness: &Harness<'_, App>, id: &str) -> u64 {
    harness.state().editor().key_of(id).unwrap()
}

fn press(harness: &mut Harness<'_, App>, pos: Pos2, button: PointerButton, pressed: bool) {
    harness.event(Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(1);
}

fn click(harness: &mut Harness<'_, App>, pos: Pos2, button: PointerButton) {
    harness.event(Event::PointerMoved(pos));
    harness.run_steps(1);
    press(harness, pos, button, true);
    press(harness, pos, button, false);
    harness.run_steps(1);
}

fn drag(harness: &mut Harness<'_, App>, from: Pos2, to: Pos2) {
    harness.event(Event::PointerMoved(from));
    harness.run_steps(1);
    press(harness, from, PointerButton::Primary, true);
    for t in [0.25, 0.5, 0.75, 1.0] {
        harness.event(Event::PointerMoved(from + (to - from) * t));
        harness.run_steps(1);
    }
    press(harness, to, PointerButton::Primary, false);
    harness.run_steps(1);
}

#[test]
fn shows_the_graph_and_renders() {
    let harness = loaded();
    assert_eq!(harness.state().editor().node_count(), 9);
    assert!(harness.state().editor().canvas_rect().width() > 300.0);
}

#[test]
fn play_button_and_space_bar_control_playback() {
    let mut harness = harness(app());
    step_until(&mut harness, "the project to load", |app| {
        app.engine().info().is_some()
    });

    harness.get_by_label("▶").click();
    harness.step();
    assert!(harness.state().clock().is_playing());

    harness.key_press(egui::Key::Space);
    harness.step();
    assert!(!harness.state().clock().is_playing());
}

#[test]
fn clicking_a_node_makes_it_active() {
    let mut harness = loaded();
    let split = key(&harness, "split");
    let rect = harness.state().editor().node_screen_rect(split).unwrap();
    click(
        &mut harness,
        rect.center_top() + vec2(0.0, 8.0),
        PointerButton::Primary,
    );
    assert_eq!(
        harness.state().editor().active().map(|n| n.id.as_str()),
        Some("split")
    );
}

#[test]
fn dragging_empty_space_box_selects() {
    let mut harness = loaded();
    let canvas = harness.state().editor().canvas_rect();
    drag(
        &mut harness,
        canvas.min + vec2(4.0, 4.0),
        canvas.max - vec2(4.0, 40.0),
    );
    assert_eq!(harness.state().editor().selected().len(), 9);
}

#[test]
fn scroll_wheel_zooms_around_the_pointer() {
    let mut harness = loaded();
    let video = key(&harness, "video");
    let anchor = harness
        .state()
        .editor()
        .node_screen_rect(video)
        .unwrap()
        .center();
    let zoom = harness.state().editor().view().zoom;

    harness.event(Event::PointerMoved(anchor));
    harness.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 120.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(20);

    let editor = harness.state().editor();
    assert!(
        editor.view().zoom > zoom * 1.05,
        "zoomed in from {zoom} to {}",
        editor.view().zoom
    );
    let moved = editor
        .node_screen_rect(video)
        .unwrap()
        .center()
        .distance(anchor);
    assert!(
        moved < 2.0,
        "the point under the pointer stays put (moved {moved} px)"
    );
}

#[test]
fn right_click_search_adds_a_node_at_the_cursor() {
    let mut harness = loaded();
    let canvas = harness.state().editor().canvas_rect();
    let spot = pos2(canvas.left() + 30.0, canvas.bottom() - 80.0);
    click(&mut harness, spot, PointerButton::Secondary);
    assert!(harness.state().editor().search_open());

    // The search box has focus straight away: type and press Enter.
    harness.event(Event::Text("bit".into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);

    let editor = harness.state().editor();
    assert!(!editor.search_open());
    let added = editor
        .key_of("bitcrush")
        .expect("a bit crush node was added");
    assert_eq!(editor.active().map(|n| n.key), Some(added));
    let placed = editor.node_screen_rect(added).unwrap().min;
    assert!(
        placed.distance(spot) < 1.5,
        "placed at {placed:?}, clicked at {spot:?}"
    );
}

#[test]
fn dragging_a_connected_input_picks_its_wire_up() {
    let mut harness = loaded();
    let combine = key(&harness, "combine");
    let wires = harness.state().editor().wires().len();
    let pin = harness
        .state()
        .editor()
        .pin_screen_pos(combine, true, 0)
        .unwrap();
    // Drag the wire off `combine.r` and drop it on empty canvas: it's disconnected, and the
    // search opens to connect it to something new.
    let canvas = harness.state().editor().canvas_rect();
    drag(
        &mut harness,
        pin,
        pos2(canvas.left() + 20.0, canvas.top() + 20.0),
    );
    assert_eq!(harness.state().editor().wires().len(), wires - 1);
    assert!(harness.state().editor().search_open());
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(!harness.state().editor().search_open());
}

#[test]
fn idle_frames_do_not_rerender() {
    let mut harness = loaded();
    let before = harness.state().engine().frame(0).unwrap();
    harness.run_steps(5);
    assert!(Arc::ptr_eq(
        &before,
        &harness.state().engine().frame(0).unwrap()
    ));
}
