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

#[test]
fn moving_a_node_keeps_rendered_frames_but_is_an_unsaved_change() {
    let mut harness = loaded();
    let before = harness.state().engine().frame(0).unwrap();
    assert!(!harness.state().is_dirty());
    // Pick a node that isn't drawn on top, so clicking it also raises it.
    let video = key(&harness, "video");
    let rect = harness.state().editor().node_screen_rect(video).unwrap();
    let grab = rect.center_top() + vec2(0.0, 8.0);
    drag(&mut harness, grab, grab + vec2(40.0, 25.0));
    harness.run_steps(3);
    assert!(
        harness.state().is_dirty(),
        "positions are saved with the project"
    );
    assert!(
        Arc::ptr_eq(&before, &harness.state().engine().frame(0).unwrap()),
        "moving a node must not re-render"
    );
}

#[test]
fn the_theme_is_dark_unless_chosen_otherwise() {
    let mut harness = harness(app());
    harness.run_steps(2);
    assert_eq!(harness.ctx.theme(), egui::Theme::Dark);

    let mut settings = harness.state().settings().clone();
    settings.theme = rastersong_gui::ThemeChoice::Light;
    harness.state_mut().set_settings(settings);
    harness.run_steps(2);
    assert_eq!(harness.ctx.theme(), egui::Theme::Light);
}

/// Clicks a node's header, then leaves the pointer over the canvas so canvas keys apply.
fn select(harness: &mut Harness<'_, App>, id: &str) {
    let node = key(harness, id);
    let rect = harness.state().editor().node_screen_rect(node).unwrap();
    click(
        harness,
        rect.center_top() + vec2(0.0, 8.0),
        PointerButton::Primary,
    );
}

fn shortcut(harness: &mut Harness<'_, App>, modifiers: Modifiers, key: egui::Key) {
    harness.key_press_modifiers(modifiers, key);
    harness.run_steps(2);
}

#[test]
fn undo_and_redo_step_through_edits() {
    let mut harness = loaded();
    assert!(!harness.state().can_undo());
    select(&mut harness, "split");
    shortcut(&mut harness, Modifiers::NONE, egui::Key::Delete);
    assert_eq!(harness.state().editor().node_count(), 8);

    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::Z);
    assert_eq!(harness.state().editor().node_count(), 9);
    assert!(!harness.state().is_dirty(), "back to the saved state");

    shortcut(
        &mut harness,
        Modifiers::COMMAND | Modifiers::SHIFT,
        egui::Key::Z,
    );
    assert_eq!(harness.state().editor().node_count(), 8);
}

#[test]
fn a_drag_is_one_undo_step() {
    let mut harness = loaded();
    let video = key(&harness, "video");
    let before = harness.state().editor().node(video).unwrap().pos;
    let rect = harness.state().editor().node_screen_rect(video).unwrap();
    let grab = rect.center_top() + vec2(0.0, 8.0);
    drag(&mut harness, grab, grab + vec2(60.0, 30.0));
    harness.run_steps(2);
    assert_ne!(harness.state().editor().node(video).unwrap().pos, before);

    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::Z);
    let video = key(&harness, "video");
    assert_eq!(harness.state().editor().node(video).unwrap().pos, before);
    assert!(!harness.state().can_undo());
}

#[test]
fn copy_and_paste_adds_the_nodes_at_the_pointer() {
    let mut harness = loaded();
    select(&mut harness, "split");
    harness.event(Event::Copy);
    harness.run_steps(2);
    let text = harness.state().editor().clipboard().unwrap().to_owned();

    let canvas = harness.state().editor().canvas_rect();
    let spot = pos2(canvas.left() + 40.0, canvas.bottom() - 90.0);
    harness.event(Event::PointerMoved(spot));
    harness.event(Event::Paste(text));
    harness.run_steps(2);

    let editor = harness.state().editor();
    assert_eq!(editor.node_count(), 10);
    let pasted = editor.active().expect("the pasted node is selected");
    assert_eq!(pasted.kind, "split");
    assert_ne!(pasted.id, "split");
    let placed = editor.node_screen_rect(pasted.key).unwrap().min;
    assert!(placed.distance(spot) < 1.5, "placed at {placed:?}");
}

#[test]
fn new_project_asks_before_dropping_changes() {
    let mut harness = loaded();
    select(&mut harness, "split");
    shortcut(&mut harness, Modifiers::NONE, egui::Key::Delete);
    assert!(harness.state().is_dirty());

    harness.get_by_label("File").click();
    harness.run_steps(2);
    harness.get_by_label("New Project").click();
    harness.run_steps(2);
    assert!(harness.state().is_confirming());
    assert_eq!(
        harness.state().editor().node_count(),
        8,
        "nothing happens yet"
    );

    harness.get_by_label("Don't Save").click();
    harness.run_steps(2);
    assert!(!harness.state().is_confirming());
    assert!(!harness.state().is_dirty());
    assert_eq!(harness.state().editor().node_count(), 9);
    assert!(
        !harness.state().can_undo(),
        "a new project starts a new history"
    );
}

use rastersong_gui::timeline::{HEADER_WIDTH, LANE_HEIGHT};

/// Screen x of `seconds` in the timeline's lanes, and the vertical middle of lane `row` (0 is
/// the video).
fn timeline_point(harness: &Harness<'_, App>, seconds: f64, row: usize) -> Pos2 {
    let area = harness.state().timeline_area();
    let lanes_left = area.left() + HEADER_WIDTH + 6.0;
    let x = harness.state().timeline_view().x(lanes_left, seconds);
    pos2(x, area.top() + 22.0 + (row as f32 + 0.5) * LANE_HEIGHT)
}

#[test]
fn the_wheel_zooms_the_timeline_and_f_fits_it_again() {
    let mut harness = loaded();
    let fitted = harness.state().timeline_view().px_per_sec;
    assert!(fitted > 0.0);
    let anchor = timeline_point(&harness, 1.0, 0);
    harness.event(Event::PointerMoved(anchor));
    harness.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 200.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(20);
    let view = harness.state().timeline_view();
    assert!(view.px_per_sec > fitted * 1.2, "zoomed in");
    let moved = timeline_point(&harness, 1.0, 0).x - anchor.x;
    assert!(
        moved.abs() < 1.5,
        "1 s stays under the pointer (moved {moved})"
    );

    shortcut(&mut harness, Modifiers::NONE, egui::Key::F);
    assert_eq!(harness.state().timeline_view().px_per_sec, fitted);
}

#[test]
fn clicking_the_ruler_seeks() {
    let mut harness = loaded();
    let mut spot = timeline_point(&harness, 1.0, 0);
    spot.y = harness.state().timeline_area().top() + 11.0;
    click(&mut harness, spot, PointerButton::Primary);
    assert_eq!(harness.state().clock().frame(), 30);
}

#[test]
fn dragging_an_audio_block_moves_its_offset() {
    let mut harness = loaded();
    let from = timeline_point(&harness, 0.5, 1);
    let to = timeline_point(&harness, 1.0, 1);
    drag(&mut harness, from, to);
    let offset = harness.state().project().audio_tracks[0].offset;
    assert!((offset - 0.5).abs() < 0.02, "offset {offset}");
    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::Z);
    assert_eq!(harness.state().project().audio_tracks[0].offset, 0.0);
}

#[test]
fn thumbnails_of_the_source_video_arrive() {
    let mut harness = loaded();
    step_until(&mut harness, "thumbnails", |app| app.thumbnail_count() > 0);
}

#[test]
fn adding_tracks_names_them_after_their_files_and_links_a_node_to_each() {
    let mut harness = loaded();
    let nodes = harness.state().editor().node_count();
    harness
        .state_mut()
        .add_audio_tracks([PathBuf::from("song"), PathBuf::from("song")]);
    harness.run_steps(2);
    let names: Vec<String> = harness
        .state()
        .project()
        .audio_tracks
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(names, ["audio", "song", "song_2"]);
    assert_eq!(harness.state().editor().node_count(), nodes + 2);

    // The linked nodes can't be deleted from the graph...
    let graph = harness.state().project().graph.clone();
    let source = |n: &rastersong_engine::NodeDesc| {
        n.params
            .get("source")
            .map(|v| format!("{v:?}"))
            .unwrap_or_default()
    };
    let song = graph
        .nodes
        .iter()
        .find(|n| source(n).contains("\"song\""))
        .expect("a node reads track song");
    select(&mut harness, &song.id.clone());
    shortcut(&mut harness, Modifiers::NONE, egui::Key::Delete);
    assert_eq!(harness.state().editor().node_count(), nodes + 2);
    assert!(harness.state().editor().is_linked(key(&harness, &song.id)));
}

#[test]
fn dragging_a_wire_onto_a_parameter_pin_modulates_it() {
    let mut harness = loaded();
    // Add a delay below the graph.
    let canvas = harness.state().editor().canvas_rect();
    click(
        &mut harness,
        pos2(canvas.center().x, canvas.bottom() - 120.0),
        PointerButton::Secondary,
    );
    harness.event(Event::Text("delay".into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    let delay = key(&harness, "delay");
    let audio = key(&harness, "audio");

    // Delay shows its time and feedback pins by default; wire the audio into time.
    let from = harness
        .state()
        .editor()
        .pin_screen_pos(audio, false, 0)
        .unwrap();
    let to = harness
        .state()
        .editor()
        .pin_screen_pos(delay, true, rastersong_gui::editor::param_port(0))
        .expect("the time pin is shown");
    drag(&mut harness, from, to);
    harness.run_steps(2);
    let graph = &harness.state().project().graph;
    assert!(
        graph
            .connections
            .iter()
            .any(|c| c.from == "audio.out" && c.to == "delay.@time"),
        "{:?}",
        graph.connections
    );
}

/// Adds a node of `kind` through the right-click search, near the bottom of the canvas.
fn add_node(harness: &mut Harness<'_, App>, search: &str) {
    let canvas = harness.state().editor().canvas_rect();
    click(
        harness,
        pos2(canvas.center().x, canvas.bottom() - 120.0),
        PointerButton::Secondary,
    );
    harness.event(Event::Text(search.into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
}

#[test]
fn the_inspector_keeps_its_width() {
    // Long units and modulated values used to widen the inspector a little every frame.
    let mut harness = loaded();
    add_node(&mut harness, "low pass");
    let lowpass = key(&harness, "lowpass");
    let audio = key(&harness, "audio");
    let from = harness
        .state()
        .editor()
        .pin_screen_pos(audio, false, 0)
        .unwrap();
    let to = harness
        .state()
        .editor()
        .pin_screen_pos(lowpass, true, rastersong_gui::editor::param_port(0))
        .unwrap();
    drag(&mut harness, from, to);
    let before = harness.state().inspector_rect().width();
    assert!(
        before < 400.0,
        "the inspector is {before} wide; it starts at 340"
    );
    select(&mut harness, "lowpass");
    for _ in 0..6 {
        harness.run_steps(10);
        let width = harness.state().inspector_rect().width();
        assert!(width <= before + 1.0, "grew from {before} to {width}");
    }
}

#[test]
fn typing_a_very_long_number_keeps_the_inspector_width() {
    let mut harness = loaded();
    add_node(&mut harness, "low pass");
    select(&mut harness, "lowpass");
    harness.run_steps(3);
    let panel = harness.state().inspector_rect();
    let before = panel.width();
    let center = harness
        .query_all_by_role(egui::accesskit::Role::SpinButton)
        .map(|n| n.rect())
        .find(|r| r.min.x >= panel.left())
        .expect("a numeric value box in the inspector")
        .center();
    // A click turns the value into a text field.
    press(&mut harness, center, PointerButton::Primary, true);
    press(&mut harness, center, PointerButton::Primary, false);
    harness.event(Event::Text(
        "1234567890123456789012345678901234567890".into(),
    ));
    harness.run_steps(3);
    let field = harness
        .query_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| n.value().is_some_and(|v| v.contains("1234567890")))
        .expect("the value box is being edited");
    let (chars, width) = (field.value().unwrap().chars().count(), field.rect().width());
    assert!(
        chars <= rastersong_gui::value_box::MAX_CHARS,
        "{chars} characters"
    );
    assert!(width <= 60.0, "the box is {width} wide while typing");
    for _ in 0..4 {
        harness.run_steps(5);
        let width = harness.state().inspector_rect().width();
        assert!(width <= before + 1.0, "grew from {before} to {width}");
    }
}

#[test]
fn the_wheel_zooms_over_an_audio_block_too() {
    let mut harness = loaded();
    let fitted = harness.state().timeline_view().px_per_sec;
    // The audio track's block (row 1) covers this point.
    harness.event(Event::PointerMoved(timeline_point(&harness, 0.5, 1)));
    harness.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 200.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(20);
    assert!(harness.state().timeline_view().px_per_sec > fitted * 1.2);
}

/// Drags with the Ctrl key held.
fn ctrl_drag(harness: &mut Harness<'_, App>, from: Pos2, to: Pos2) {
    harness.event(Event::ModifiersChanged(Modifiers::COMMAND));
    harness.run_steps(1);
    harness.event(Event::PointerMoved(from));
    harness.run_steps(1);
    harness.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::COMMAND,
    });
    harness.run_steps(1);
    for t in [0.25, 0.5, 0.75, 1.0] {
        harness.event(Event::PointerMoved(from + (to - from) * t));
        harness.run_steps(1);
    }
    harness.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::COMMAND,
    });
    harness.run_steps(1);
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
    harness.run_steps(1);
}

#[test]
fn dragging_along_the_ruler_scrubs_the_playhead_without_making_a_loop() {
    let mut harness = loaded();
    let ruler_y = harness.state().timeline_area().top() + 11.0;
    let mut from = timeline_point(&harness, 0.5, 0);
    let mut to = timeline_point(&harness, 1.0, 0);
    from.y = ruler_y;
    to.y = ruler_y;
    drag(&mut harness, from, to);
    assert_eq!(
        harness.state().clock().frame(),
        30,
        "the playhead follows the drag"
    );
    assert!(harness.state().project().loop_region.is_none());
}

#[test]
fn ctrl_dragging_along_the_ruler_makes_a_loop_region() {
    let mut harness = loaded();
    let ruler_y = harness.state().timeline_area().top() + 11.0;
    let mut from = timeline_point(&harness, 0.5, 0);
    let mut to = timeline_point(&harness, 1.0, 0);
    from.y = ruler_y;
    to.y = ruler_y;
    let playhead = harness.state().clock().frame();
    ctrl_drag(&mut harness, from, to);
    assert_eq!(harness.state().clock().frame(), playhead, "it doesn't seek");
    let region = harness
        .state()
        .project()
        .loop_region
        .expect("a loop region");
    assert!((region.start - 0.5).abs() < 0.05 && (region.end - 1.0).abs() < 0.05);
    assert!(region.enabled);
    // Whole frames.
    assert!((region.start * 30.0 - (region.start * 30.0).round()).abs() < 1e-9);

    shortcut(&mut harness, Modifiers::NONE, egui::Key::R);
    assert!(!harness.state().project().loop_region.unwrap().enabled);
    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::Z);
    assert!(
        harness.state().project().loop_region.unwrap().enabled,
        "undoable"
    );
}

#[test]
fn track_and_video_names_are_shared_with_the_graph() {
    let mut harness = loaded();
    let audio = key(&harness, "audio");
    assert!(harness.state().editor().is_linked(audio));

    assert!(harness.state_mut().rename_track("audio", "drums"));
    harness.run_steps(2);
    assert_eq!(harness.state().project().audio_tracks[0].name, "drums");
    let node = harness.state().editor().node(audio).unwrap();
    assert_eq!(
        node.params.get("source"),
        Some(&rastersong_engine::ParamValue::Text("drums".into()))
    );
    assert!(harness.state().editor().is_linked(audio), "still linked");

    // A name another track has, or an empty one, is refused.
    harness
        .state_mut()
        .add_audio_tracks([PathBuf::from("song")]);
    assert!(!harness.state_mut().rename_track("song", "drums"));
    assert!(!harness.state_mut().rename_track("song", "  "));

    // The video's name is the project's too, and isn't its file's.
    let file_name = harness.state().project().video_display_name();
    harness.state_mut().rename_video("Intro shot");
    assert_eq!(
        harness.state().project().video_name.as_deref(),
        Some("Intro shot")
    );
    harness
        .state_mut()
        .rename_video(&file_name.clone().unwrap());
    assert_eq!(
        harness.state().project().video_name,
        None,
        "back to the file's name"
    );
    assert_eq!(harness.state().project().video_display_name(), file_name);
}

#[test]
fn opening_a_video_with_sound_adds_its_audio_track() {
    let backend = FakeBackend::new()
        .with_video(
            "movie.mp4",
            FakeVideo {
                width: 64,
                height: 36,
                frame_count: 30,
                frame_rate: Rational::new(30, 1),
            },
        )
        .with_audio(
            "movie.mp4",
            AudioClip {
                sample_rate: 8000,
                channels: 1,
                samples: vec![0.0; 8000],
            },
        );
    let mut app = App::new(
        Arc::new(backend),
        Project::new(GraphDesc::from_json(STARTER_GRAPH).unwrap()),
        None,
        AudioOut::silent(None),
    );
    app.open_video(PathBuf::from("movie.mp4"));
    let tracks: Vec<(String, PathBuf)> = app
        .project()
        .audio_tracks
        .iter()
        .map(|t| (t.name.clone(), t.path.clone()))
        .collect();
    assert_eq!(tracks, [("movie".to_owned(), PathBuf::from("movie.mp4"))]);
    // Opening it again doesn't add the track twice.
    app.open_video(PathBuf::from("movie.mp4"));
    assert_eq!(app.project().audio_tracks.len(), 1);
}

#[test]
fn ctrl_comma_opens_the_settings_window_with_both_pages() {
    let mut harness = loaded();
    assert!(harness.query_by_label("Application").is_none());
    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::Comma);
    harness.run_steps(2);
    harness.get_by_label("Application");
    harness.get_by_label("Keep input connections when duplicating and pasting");
    harness.get_by_label("Project").click();
    harness.run_steps(3);
    harness.get_by_label("Audio output rate");
}

#[test]
fn the_warmup_limit_is_a_project_setting_that_reaches_the_project() {
    let mut harness = loaded();
    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::Comma);
    harness.get_by_label("Project").click();
    harness.run_steps(3);
    harness.get_by_label("Max warmup frames");
    assert_eq!(harness.state().project().max_warmup_frames, 120);
}
