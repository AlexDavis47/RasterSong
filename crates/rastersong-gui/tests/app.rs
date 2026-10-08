//! Interaction tests on the real UI, driven headlessly with the fake media backend and raw
//! pointer and keyboard events.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rastersong_engine::{
    AudioClip, FakeBackend, FakeVideo, GraphDesc, Project, Rational, ResourceKind, StreamInfo,
    StreamKind, TrackKind,
};
use rastersong_gui::{App, AudioOut, STARTER_GRAPH};

mod common;
use common::TrackLists;

fn app() -> App {
    app_with(|_| {})
}

/// The app with the starter graph changed by `edit`.
fn app_with(edit: impl FnOnce(&mut GraphDesc)) -> App {
    app_with_project(|project| edit(&mut project.graph))
}

/// The app with the starter project (the starter graph, the video and the song) changed by `edit`.
fn app_with_project(edit: impl FnOnce(&mut Project)) -> App {
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
    loaded_with(app())
}

fn loaded_with(app: App) -> Harness<'static, App> {
    settled(harness(app))
}

/// [`loaded_with`] with short steps, so two clicks land inside egui's double-click delay.
fn loaded_quick(app: App) -> Harness<'static, App> {
    settled(
        Harness::builder()
            .with_size(egui::vec2(1400.0, 900.0))
            .with_step_dt(0.02)
            .build_ui_state(|ui, app: &mut App| app.ui(ui), app),
    )
}

/// Steps until the first second is rendered.
fn settled(mut harness: Harness<'static, App>) -> Harness<'static, App> {
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

/// A secondary click, and the steps its menu takes to appear.
fn right_click(harness: &mut Harness<'_, App>, pos: Pos2) {
    click(harness, pos, PointerButton::Secondary);
    harness.run_steps(2);
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

/// A point on the header bar of the item at `seconds` in timeline row `row`.
fn item_bar_point(harness: &Harness<'_, App>, seconds: f64, row: usize) -> Pos2 {
    let mut point = timeline_point(harness, seconds, row);
    point.y = harness.state().timeline_area().top() + 22.0 + row as f32 * LANE_HEIGHT + 9.0;
    point
}

#[test]
fn dragging_an_item_by_its_header_bar_moves_it() {
    let mut harness = loaded();
    let from = item_bar_point(&harness, 0.5, 1);
    let to = item_bar_point(&harness, 1.0, 1);
    drag(&mut harness, from, to);
    let position = harness.state().project().audios()[0].items[0].position;
    assert!((position - 0.5).abs() < 0.02, "position {position}");
    // The video isn't linked, so it stays.
    assert_eq!(harness.state().project().videos()[0].items[0].position, 0.0);
    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::Z);
    assert_eq!(harness.state().project().audios()[0].items[0].position, 0.0);
    // Below the bar the item is content: dragging there box-selects instead, and leaves the
    // playhead alone.
    let body = timeline_point(&harness, 0.5, 1);
    let to = timeline_point(&harness, 1.0, 1);
    drag(&mut harness, body, to);
    assert_eq!(harness.state().project().audios()[0].items[0].position, 0.0);
    assert_eq!(harness.state().clock().frame(), 0);
}

#[test]
fn the_selection_box_is_drawn_over_the_items() {
    let mut harness = loaded();
    let from = timeline_point(&harness, 0.2, 1);
    let to = timeline_point(&harness, 1.5, 0);
    harness.event(Event::PointerMoved(from));
    harness.run_steps(1);
    press(&mut harness, from, PointerButton::Primary, true);
    for t in [0.5, 1.0] {
        harness.event(Event::PointerMoved(from + (to - from) * t));
        harness.run_steps(1);
    }
    let selection = egui::Rect::from_two_pos(from, to);
    let rects: Vec<egui::Rect> = harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(r) => Some(r.rect),
            _ => None,
        })
        .collect();
    let at = rects
        .iter()
        .rposition(|r| r.min.distance(selection.min) < 1.0 && r.max.distance(selection.max) < 1.0)
        .expect("the box is drawn while dragging");
    // Shapes come in paint order.
    let later = rects[at + 1..]
        .iter()
        .filter(|r| r.intersects(selection.shrink(1.0)))
        .count();
    assert_eq!(later, 0, "nothing in the lanes is drawn over the box");
    press(&mut harness, to, PointerButton::Primary, false);
}

#[test]
fn dragging_over_the_lanes_box_selects_items() {
    let mut harness = loaded();
    seek_on_ruler(&mut harness, 1.0);
    harness.event(Event::PointerMoved(timeline_point(&harness, 1.5, 1)));
    shortcut(&mut harness, Modifiers::NONE, egui::Key::S);
    let selected = |harness: &Harness<'_, App>| -> Vec<(String, usize)> {
        let mut refs: Vec<_> = harness
            .state()
            .selected_items()
            .iter()
            .map(|r| (r.track.clone(), r.item))
            .collect();
        refs.sort();
        refs
    };
    // A box over the second halves of both tracks, from below the last track.
    let mut from = timeline_point(&harness, 1.9, 1);
    from.y += LANE_HEIGHT;
    let to = timeline_point(&harness, 1.2, 0);
    drag(&mut harness, from, to);
    assert_eq!(
        selected(&harness),
        [("audio".to_owned(), 1), ("video".to_owned(), 1)]
    );
    assert_eq!(harness.state().clock().frame(), 30, "the playhead stays");

    // A plain box replaces the selection; Ctrl or Shift adds to it.
    let from = timeline_point(&harness, 0.2, 1);
    let to = timeline_point(&harness, 0.4, 1);
    drag(&mut harness, from, to);
    assert_eq!(selected(&harness), [("audio".to_owned(), 0)]);
    let from = timeline_point(&harness, 0.2, 0);
    let to = timeline_point(&harness, 0.4, 0);
    modifier_drag(&mut harness, Modifiers::SHIFT, from, to);
    assert_eq!(
        selected(&harness),
        [("audio".to_owned(), 0), ("video".to_owned(), 0)]
    );

    // The selected items edit together: Delete removes both.
    shortcut(&mut harness, Modifiers::NONE, egui::Key::Delete);
    let project = harness.state().project();
    assert_eq!(project.audios()[0].items.len(), 1);
    assert_eq!(project.videos()[0].items.len(), 1);

    // A click on empty lane space selects nothing.
    let empty = timeline_point(&harness, 0.3, 1);
    click(&mut harness, empty, PointerButton::Primary);
    assert!(selected(&harness).is_empty());
}

#[test]
fn linked_tracks_move_their_overlapping_items_together() {
    let mut harness = harness(app_with_project(|project| {
        project.link_tracks("video", "audio");
    }));
    step_until(&mut harness, "rendered frames", |app| {
        app.engine().buffered_from(0) >= 30
    });
    step_until(&mut harness, "the video's length", |app| {
        app.thumbnail_count() > 0
    });
    harness.run_steps(2);
    // Both headers show the link.
    assert_eq!(harness.get_all_by_label("linked").count(), 2);
    let from = item_bar_point(&harness, 0.5, 1);
    let to = item_bar_point(&harness, 1.0, 1);
    drag(&mut harness, from, to);
    let project = harness.state().project();
    let audio = project.audios()[0].items[0].position;
    assert!((audio - 0.5).abs() < 0.02, "audio at {audio}");
    assert_eq!(project.videos()[0].items[0].position, audio);
}

/// Clicks the ruler at `seconds`, moving the playhead there.
fn seek_on_ruler(harness: &mut Harness<'_, App>, seconds: f64) {
    let mut spot = timeline_point(harness, seconds, 0);
    spot.y = harness.state().timeline_area().top() + 11.0;
    click(harness, spot, PointerButton::Primary);
}

#[test]
fn s_splits_under_the_playhead_and_delete_removes_the_selected_items() {
    let mut harness = loaded();
    seek_on_ruler(&mut harness, 1.0);
    harness.event(Event::PointerMoved(timeline_point(&harness, 1.5, 1)));
    // Nothing selected: every item under the playhead is split.
    shortcut(&mut harness, Modifiers::NONE, egui::Key::S);
    let project = harness.state().project();
    assert_eq!(project.audios()[0].items.len(), 2);
    assert_eq!(project.videos()[0].items.len(), 2);
    let second = item_bar_point(&harness, 1.5, 1);
    click(&mut harness, second, PointerButton::Primary);
    shortcut(&mut harness, Modifiers::NONE, egui::Key::Delete);
    let project = harness.state().project();
    assert_eq!(project.audios()[0].items.len(), 1);
    assert_eq!(project.audios()[0].items[0].end, Some(1.0));
    // The video isn't linked, so it keeps both halves.
    assert_eq!(project.videos()[0].items.len(), 2);
}

/// The system clipboard as the platform layer (egui-winit) handles it: Ctrl+C and Ctrl+X send
/// copy and cut events, and whatever the app copies lands on the clipboard; Ctrl+V sends a paste
/// event only when the clipboard holds text.
#[derive(Default)]
struct PlatformClipboard(String);

impl PlatformClipboard {
    fn send(&mut self, harness: &mut Harness<'_, App>, event: Event) {
        harness.event(event);
        harness.run_steps(1);
        for command in &harness.output().platform_output.commands {
            if let egui::OutputCommand::CopyText(text) = command {
                self.0.clone_from(text);
            }
        }
        harness.run_steps(1);
    }

    fn copy(&mut self, harness: &mut Harness<'_, App>) {
        self.send(harness, Event::Copy);
    }

    fn cut(&mut self, harness: &mut Harness<'_, App>) {
        self.send(harness, Event::Cut);
    }

    fn paste(&mut self, harness: &mut Harness<'_, App>) {
        if !self.0.is_empty() {
            let text = self.0.clone();
            self.send(harness, Event::Paste(text));
        }
    }
}

#[test]
fn copied_items_paste_at_the_playhead() {
    let mut harness = loaded();
    let mut clipboard = PlatformClipboard::default();
    let nodes = harness.state().project().graph.nodes.len();
    let bar = item_bar_point(&harness, 0.5, 1);
    click(&mut harness, bar, PointerButton::Primary);
    clipboard.copy(&mut harness);
    seek_on_ruler(&mut harness, 1.0);
    clipboard.paste(&mut harness);
    let positions = |harness: &Harness<'_, App>| -> Vec<f64> {
        harness.state().project().audios()[0]
            .items
            .iter()
            .map(|i| i.position)
            .collect()
    };
    assert_eq!(positions(&harness), [0.0, 1.0]);
    // Over the timeline the clipboard is the timeline's, not the graph's.
    assert_eq!(harness.state().project().graph.nodes.len(), nodes);

    // Cut removes the selected (pasted) item and pastes it back.
    clipboard.cut(&mut harness);
    assert_eq!(positions(&harness), [0.0]);
    seek_on_ruler(&mut harness, 1.5);
    clipboard.paste(&mut harness);
    assert_eq!(positions(&harness), [0.0, 1.5]);

    // Text copied elsewhere since then isn't the items.
    clipboard.0 = "some other text".into();
    clipboard.paste(&mut harness);
    assert_eq!(positions(&harness).len(), 2);
}

#[test]
fn dragging_an_items_edge_trims_it() {
    let mut harness = loaded();
    // The song is 2 s long; its end edge runs the item's whole height.
    let from = timeline_point(&harness, 2.0, 1);
    let to = timeline_point(&harness, 1.5, 1);
    drag(&mut harness, from, to);
    let item = harness.state().project().audios()[0].items[0].clone();
    let end = item.end.unwrap();
    assert!((end - 1.5).abs() < 0.02, "end {end}");
    assert_eq!((item.position, item.start, item.rate), (0.0, 0.0, 1.0));
}

#[test]
fn items_mute_from_their_header_bar_and_tracks_solo_from_theirs() {
    let mut harness = loaded();
    // The song is 2 s long: its mute button is at the right end of its bar.
    let mut mute = item_bar_point(&harness, 2.0, 1);
    mute.x -= 8.0;
    click(&mut harness, mute, PointerButton::Primary);
    assert!(harness.state().project().audios()[0].items[0].muted);
    // Each header has a solo button, the video's first.
    harness.get_all_by_label("S").nth(1).unwrap().click();
    harness.run_steps(2);
    assert!(harness.state().project().audios()[0].solo);
    assert!(!harness.state().project().videos()[0].solo);
}

#[test]
fn dragging_a_headers_bottom_edge_changes_the_track_height() {
    let mut harness = loaded();
    let area = harness.state().timeline_area();
    // The video's header, the first row.
    let edge = pos2(area.left() + 60.0, area.top() + 22.0 + LANE_HEIGHT);
    drag(&mut harness, edge, edge + vec2(0.0, 40.0));
    let height = harness.state().project().videos()[0].height.unwrap();
    assert!(
        (height - (LANE_HEIGHT + 40.0)).abs() < 2.0,
        "height {height}"
    );
}

/// Two primary clicks at `pos`; a double-click on a harness from [`loaded_quick`].
fn double_click(harness: &mut Harness<'_, App>, pos: Pos2) {
    harness.event(Event::PointerMoved(pos));
    harness.run_steps(1);
    for _ in 0..2 {
        for pressed in [true, false] {
            press(harness, pos, PointerButton::Primary, pressed);
        }
    }
    harness.run_steps(2);
}

#[test]
fn double_clicking_a_headers_bottom_edge_resets_the_track_height() {
    let mut harness = loaded_quick(app());
    let area = harness.state().timeline_area();
    let edge = pos2(area.left() + 60.0, area.top() + 22.0 + LANE_HEIGHT);
    drag(&mut harness, edge, edge + vec2(0.0, 40.0));
    assert!(harness.state().project().videos()[0].height.is_some());
    // The edge moved down with the track.
    double_click(&mut harness, edge + vec2(0.0, 40.0));
    let height = harness.state().project().videos()[0].height;
    assert!(
        height.is_none_or(|h| (h - LANE_HEIGHT).abs() < 0.5),
        "height {height:?}"
    );
}

#[test]
fn thumbnails_of_the_source_video_arrive() {
    let mut harness = loaded();
    step_until(&mut harness, "thumbnails", |app| app.thumbnail_count() > 0);
}

#[test]
fn the_same_video_on_two_tracks_loads_both() {
    let mut harness = loaded();
    let resource = harness.state().project().videos()[0].resource.unwrap();
    harness.state_mut().add_resource_track(resource, 1.0);
    harness.state_mut().add_resource_track(resource, 2.0);
    harness.run_steps(2);
    assert_eq!(harness.state().project().videos().len(), 3);
    // Neither new track is left on "loading", and each gets its thumbnails.
    step_until(&mut harness, "both lengths", |app| {
        app.video_durations().iter().all(Option::is_some)
    });
    harness.run_steps(2);
}

#[test]
fn empty_tracks_take_resources_dropped_on_them() {
    let mut app = import_app();
    app.import_files([PathBuf::from("concert.mkv")]);
    app.finish_import(Some(&[true, true, false]));
    let ids: Vec<_> = app.project().resources.iter().map(|r| r.id).collect();
    let (video, band) = (ids[0], ids[1]);
    let name = app.add_empty_track();
    assert_eq!(app.project().track(&name).unwrap().resource, None);
    let i = app.project().track_index(&name).unwrap();
    let row = app.project().shown_tracks().iter().position(|&j| j == i);
    // Dropped on the empty track, the video fills it; the same again adds an item.
    assert_eq!(app.drop_resource(video, row, 2.0), Some(name.clone()));
    assert_eq!(app.drop_resource(video, row, 9.0), Some(name.clone()));
    assert_eq!(app.project().videos().len(), 1);
    assert_eq!(app.project().videos()[0].items.len(), 2);
    // Audio on a video track starts a track of its own.
    let other = app.drop_resource(band, row, 0.0).unwrap();
    assert_ne!(other, name);
    assert_eq!(app.project().audios().len(), 1);
    // Off the tracks, a resource makes a new track.
    app.drop_resource(video, None, 0.0);
    assert_eq!(app.project().videos().len(), 2);
}

#[test]
fn a_missing_file_leaves_its_tracks_as_gaps_until_relocated() {
    let mut app = import_app();
    app.import_files([PathBuf::from("concert.mkv")]);
    app.finish_import(Some(&[true, true, false]));
    let ids: Vec<_> = app.project().resources.iter().map(|r| r.id).collect();
    app.add_resource_track(ids[0], 0.0);
    assert!(app.missing_resources().is_empty());
    // The file moves: the resource is missing until it is pointed at the new place.
    app.relocate_resource(ids[0], PathBuf::from("gone.mkv"));
    assert!(app.missing_resources().contains(&ids[0]));
    assert!(app.missing_resources().contains(&ids[1]));
    app.relocate_resource(ids[0], PathBuf::from("concert.mkv"));
    assert!(app.missing_resources().is_empty());
    assert!(
        app.project()
            .resources
            .iter()
            .all(|r| r.path.as_path() == Path::new("concert.mkv"))
    );
}

#[test]
fn a_new_graph_opens_as_a_passthrough_and_the_old_one_is_kept() {
    let mut harness = loaded();
    let before = harness.state().project().graph.clone();
    let id = harness.state_mut().new_graph();
    harness.run_steps(3);
    let app = harness.state();
    assert_eq!(app.project().graph_id, id);
    assert_eq!(app.project().graphs.len(), 1);
    assert_eq!(
        app.project().graphs[0].graph.nodes.len(),
        before.nodes.len()
    );
    // The editor shows the passthrough: the input nodes, Video Output and Audio Output.
    assert!(
        app.editor().node_count() <= 6,
        "{}",
        app.editor().node_count()
    );
    let old = app.project().graphs[0].id;
    harness.state_mut().open_graph(old);
    harness.run_steps(3);
    assert_eq!(harness.state().project().graph_id, old);
}

#[test]
fn adding_tracks_names_them_after_their_files_and_leaves_the_graph_alone() {
    let mut harness = loaded();
    let graph = harness.state().project().graph.clone();
    harness
        .state_mut()
        .add_audio_tracks([PathBuf::from("song"), PathBuf::from("song")]);
    harness.run_steps(2);
    let names: Vec<String> = harness
        .state()
        .project()
        .audios()
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(names, ["audio", "song", "song_2"]);
    // A graph's inputs are ports, filled where the graph is used, so tracks add no nodes.
    assert_eq!(harness.state().project().graph, graph);
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
    add_node(&mut harness, "filter");
    let filter = key(&harness, "filter");
    let audio = key(&harness, "audio");
    let from = harness
        .state()
        .editor()
        .pin_screen_pos(audio, false, 0)
        .unwrap();
    let to = harness
        .state()
        .editor()
        .pin_screen_pos(filter, true, rastersong_gui::editor::param_port(2))
        .unwrap();
    drag(&mut harness, from, to);
    let before = harness.state().inspector_rect().width();
    assert!(
        before < 400.0,
        "the inspector is {before} wide; it starts at 340"
    );
    select(&mut harness, "filter");
    for _ in 0..6 {
        harness.run_steps(10);
        let width = harness.state().inspector_rect().width();
        assert!(width <= before + 1.0, "grew from {before} to {width}");
    }
}

#[test]
fn typing_a_very_long_number_keeps_the_inspector_width() {
    let mut harness = loaded();
    add_node(&mut harness, "filter");
    select(&mut harness, "filter");
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
    modifier_drag(harness, Modifiers::COMMAND, from, to);
}

/// A primary drag with `modifiers` held throughout.
fn modifier_drag(harness: &mut Harness<'_, App>, modifiers: Modifiers, from: Pos2, to: Pos2) {
    harness.event(Event::ModifiersChanged(modifiers));
    harness.run_steps(1);
    harness.event(Event::PointerMoved(from));
    harness.run_steps(1);
    harness.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers,
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
        modifiers,
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
fn tracks_are_renamed_without_touching_the_graph() {
    let mut harness = loaded();
    let graph = harness.state().project().graph.clone();
    assert!(harness.state_mut().rename_track("audio", "drums"));
    harness.run_steps(2);
    assert_eq!(harness.state().project().audios()[0].name, "drums");
    assert_eq!(harness.state().project().graph, graph);

    // A name another track has, or an empty one, is refused.
    harness
        .state_mut()
        .add_audio_tracks([PathBuf::from("song")]);
    assert!(!harness.state_mut().rename_track("song", "drums"));
    assert!(!harness.state_mut().rename_track("song", "  "));

    // The video's track is renamed the same way.
    let video = harness.state().project().video_display_name().unwrap();
    assert!(harness.state_mut().rename_track(&video, "Intro shot"));
    assert_eq!(
        harness.state().project().video_display_name().as_deref(),
        Some("Intro shot")
    );
    assert!(!harness.state_mut().rename_track("Intro shot", "drums"));
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
                samples: vec![0.0; 8000].into(),
            },
        );
    let mut app = App::new(
        Arc::new(backend),
        Project::new(GraphDesc::from_json(STARTER_GRAPH).unwrap()),
        None,
        AudioOut::silent(None),
    );
    app.open_video(PathBuf::from("movie.mp4"));
    let tracks: Vec<(String, Option<&Path>)> = app
        .project()
        .audios()
        .iter()
        .map(|t| (t.name.clone(), app.project().track_path(t)))
        .collect();
    assert_eq!(tracks, [("movie".to_owned(), Some(Path::new("movie.mp4")))]);
    // Opening it again doesn't add the track twice.
    app.open_video(PathBuf::from("movie.mp4"));
    assert_eq!(app.project().audios().len(), 1);
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
fn tracks_offer_the_buses_and_project_settings_add_them() {
    let mut harness = harness(app_with_project(|project| {
        project.buses.push(rastersong_engine::Bus {
            name: "Stems".into(),
            channels: 1,
        });
        project.track_of_mut(TrackKind::Audio, 0).bus = "Stems".into();
    }));
    step_until(&mut harness, "the project to load", |app| {
        app.engine().info().is_some()
    });
    harness.run_steps(2);
    // With two buses, the track's header shows which it is routed to.
    harness.get_by_value("Stems");
    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::Comma);
    harness.get_by_label("Project").click();
    harness.run_steps(3);
    harness.get_by_label("Output buses");
    harness.get_by_label("+ Add bus").click();
    harness.run_steps(2);
    let names: Vec<&str> = harness
        .state()
        .project()
        .buses
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    assert_eq!(names, ["Main", "Stems", "Bus 2"]);
}

#[test]
fn the_picture_follows_the_video_until_a_frame_rate_is_picked() {
    let mut harness = loaded();
    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::Comma);
    harness.get_by_label("Project").click();
    harness.run_steps(3);
    // No timebase of its own: the fields show the video's, and say so.
    harness.get_by_label("Following the first video");
    harness.get_by_value("30 fps").click();
    harness.run_steps(2);
    harness.get_by_label("29.97 fps").click();
    harness.run_steps(2);
    let picked = harness.state().project().timebase;
    assert_eq!(
        picked,
        Some(rastersong_engine::Timebase {
            width: 64,
            height: 36,
            frame_rate: Rational::new(30000, 1001),
        })
    );
    step_until(&mut harness, "the new frame rate", |app| {
        app.engine()
            .info()
            .is_some_and(|info| info.frame_rate == Rational::new(30000, 1001))
    });
    // The reset button goes back to following the video.
    harness.get_by_label("\u{21ba}").click();
    harness.run_steps(2);
    assert_eq!(harness.state().project().timebase, None);
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

#[test]
fn whole_number_parameters_round_what_is_typed() {
    use rastersong_engine::ParamValue;
    let mut harness = loaded();
    add_node(&mut harness, "beat");
    select(&mut harness, "beat");
    harness.run_steps(3);
    let panel = harness.state().inspector_rect();
    // The first number in a Beat's settings is its division, which only makes sense whole.
    let center = harness
        .query_all_by_role(egui::accesskit::Role::SpinButton)
        .map(|n| n.rect())
        .find(|r| r.min.x >= panel.left())
        .expect("a numeric value box in the inspector")
        .center();
    press(&mut harness, center, PointerButton::Primary, true);
    press(&mut harness, center, PointerButton::Primary, false);
    harness.event(Event::Text("3.4".into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(3);
    let node = key(&harness, "beat");
    let value = harness.state().editor().node(node).unwrap().params["division"].clone();
    assert_eq!(value, ParamValue::Number(3.0));
}

#[test]
fn generators_have_a_layout_setting_next_to_the_shared_ones() {
    let mut harness = loaded();
    add_node(&mut harness, "beat");
    select(&mut harness, "beat");
    harness.run_steps(3);
    harness.get_by_label("Layout");
}

fn beat_with(extra: &str) -> impl FnOnce(&mut GraphDesc) {
    let node = format!(r#"{{ "id": "beat", "type": "beat" {extra} }}"#);
    move |graph| graph.nodes.push(serde_json::from_str(&node).unwrap())
}

#[test]
fn parameters_a_setting_leaves_unused_are_hidden() {
    // A Beat defaults to the decay shape, which uses neither width nor steps.
    let mut harness = harness(app_with(beat_with("")));
    harness.run_steps(3);
    select(&mut harness, "beat");
    harness.run_steps(3);
    harness.get_by_label("Division");
    assert!(harness.query_by_label("Width").is_none());
    assert!(harness.query_by_label("Steps").is_none());
}

#[test]
fn a_parameter_appears_when_its_setting_makes_it_matter() {
    let mut harness = harness(app_with(beat_with(r#", "params": { "shape": "pulse" }"#)));
    harness.run_steps(3);
    select(&mut harness, "beat");
    harness.run_steps(3);
    harness.get_by_label("Width");
    assert!(harness.query_by_label("Steps").is_none());
    assert!(harness.query_by_label_contains("Unused").is_none());
}

#[test]
fn an_unused_parameter_with_a_wire_stays_visible_and_says_why() {
    let graph = |graph: &mut GraphDesc| {
        beat_with("")(graph);
        graph
            .connections
            .push(serde_json::from_str(r#"{ "from": "audio", "to": "beat.@width" }"#).unwrap());
    };
    let mut harness = harness(app_with(graph));
    harness.run_steps(3);
    select(&mut harness, "beat");
    harness.run_steps(3);
    harness.get_by_label("Width");
    harness.get_by_label_contains("Unused: only applies when Shape is pulse");
}

fn stream(index: usize, kind: StreamKind, title: Option<&str>) -> StreamInfo {
    StreamInfo {
        index,
        kind,
        codec: "fake".into(),
        title: title.map(str::to_owned),
        language: None,
        default: index == 0,
    }
}

/// An app whose backend knows `concert.mkv` (a video and two audio streams) and `kick.wav`.
fn import_app() -> App {
    let audio = || AudioClip {
        sample_rate: 8000,
        channels: 1,
        samples: vec![0.0; 8000].into(),
    };
    let backend = FakeBackend::new()
        .with_video(
            "concert.mkv",
            FakeVideo {
                width: 64,
                height: 36,
                frame_count: 30,
                frame_rate: Rational::new(30, 1),
            },
        )
        .with_audio("concert.mkv", audio())
        .with_streams(
            "concert.mkv",
            vec![
                stream(
                    0,
                    StreamKind::Video {
                        width: 64,
                        height: 36,
                        frame_rate: 30.0,
                    },
                    None,
                ),
                stream(
                    1,
                    StreamKind::Audio {
                        sample_rate: 8000,
                        channels: 1,
                    },
                    Some("Band"),
                ),
                stream(
                    2,
                    StreamKind::Audio {
                        sample_rate: 8000,
                        channels: 1,
                    },
                    Some("Crowd"),
                ),
            ],
        )
        .with_audio("kick.wav", audio());
    App::new(
        Arc::new(backend),
        Project::new(GraphDesc::from_json(STARTER_GRAPH).unwrap()),
        None,
        AudioOut::silent(None),
    )
}

#[test]
fn importing_a_file_with_several_streams_asks_which_to_import() {
    let mut app = import_app();
    app.import_files([PathBuf::from("kick.wav"), PathBuf::from("concert.mkv")]);
    // One stream: imported at once.
    let names = |app: &App| -> Vec<(String, ResourceKind, Option<usize>)> {
        app.project()
            .resources
            .iter()
            .map(|r| (r.name.clone(), r.kind, r.stream))
            .collect()
    };
    assert_eq!(names(&app), [("kick".into(), ResourceKind::Audio, Some(0))]);
    assert_eq!(app.pending_import().unwrap().streams.len(), 3);

    let mut harness = harness(app);
    harness.run_steps(2);
    harness.get_by_label("Found multiple tracks in this media");
    // Leave out the crowd.
    harness
        .get_by_label("Audio 2: Crowd · fake · 8000 Hz · Mono")
        .click();
    harness.run_steps(1);
    harness.get_by_label("Import").click();
    harness.run_steps(2);
    let app = harness.state();
    assert!(app.pending_import().is_none());
    assert_eq!(
        names(app),
        [
            ("kick".into(), ResourceKind::Audio, Some(0)),
            ("concert.mkv".into(), ResourceKind::Video, Some(0)),
            ("concert Band".into(), ResourceKind::Audio, Some(1)),
        ]
    );
    // Nothing is on the timeline until a resource is placed there.
    assert!(app.project().audios().is_empty());
}

#[test]
fn resources_become_tracks_and_take_their_tracks_with_them() {
    let mut app = import_app();
    app.import_files([PathBuf::from("concert.mkv")]);
    app.finish_import(Some(&[true, true, false]));
    let ids: Vec<_> = app.project().resources.iter().map(|r| r.id).collect();
    assert_eq!(
        app.add_resource_track(ids[1], 1.5).as_deref(),
        Some("concert Band")
    );
    app.add_resource_track(ids[0], 0.0);
    let project = app.project();
    assert_eq!(project.audios()[0].items[0].position, 1.5);
    assert_eq!(project.videos().len(), 1);
    let band = project.timeline();
    let band = band
        .tracks
        .iter()
        .find(|t| t.name == "concert Band")
        .unwrap();
    assert_eq!((band.kind, band.stream), (TrackKind::Audio, Some(1)));

    // A cancelled import adds nothing.
    app.import_files([PathBuf::from("concert.mkv")]);
    app.finish_import(None);
    assert_eq!(app.project().resources.len(), 2);

    app.remove_resource(ids[1]);
    assert!(app.project().audios().is_empty());
    assert_eq!(app.project().resources.len(), 1);
}

// --- FX ---

use rastersong_engine::{Fx, FxTarget};
use rastersong_gui::timeline::FxDrop;

/// A graph reading the main picture and a port `Side`, wired to the output.
const SIDE_GRAPH: &str = r#"{ "version": 0, "nodes": [
    { "id": "v", "type": "video_input" },
    { "id": "s", "type": "audio_input", "params": { "port": "Side" } },
    { "id": "o", "type": "output" } ],
    "connections": [ { "from": "v", "to": "o" } ] }"#;

/// The starter project with a second graph (reading a port `Side`); `edit` also gets the ids
/// of the open graph and the second one.
fn fx_app(edit: impl FnOnce(&mut Project, u32, u32)) -> App {
    app_with_project(|project| {
        let second = project.add_graph(
            "Second graph",
            Some(GraphDesc::from_json(SIDE_GRAPH).unwrap()),
        );
        let first = project.graph_id;
        edit(project, first, second);
    })
}

/// Switches the Resources panel to its graphs and returns the centre of the card named `name`.
fn graph_card(harness: &mut Harness<'_, App>, name: &str) -> Pos2 {
    harness.get_by_label("Graphs").click();
    harness.run_steps(2);
    // The Resources panel's card, not the FX window's line naming the graph.
    harness
        .get_all_by_label(name)
        .map(|n| n.rect().center())
        .min_by(|a, b| a.x.total_cmp(&b.x))
        .unwrap()
}

/// The centre of the right end of track row `row`'s header, clear of its widgets.
fn header_point(harness: &Harness<'_, App>, row: usize) -> Pos2 {
    let area = harness.state().timeline_area();
    pos2(
        area.left() + HEADER_WIDTH - 8.0,
        area.top() + 22.0 + (row as f32 + 0.5) * LANE_HEIGHT,
    )
}

#[test]
fn dropping_a_graph_on_a_header_adds_it_to_the_tracks_fx_and_undo_removes_it() {
    let mut harness = loaded_with(fx_app(|_, _, _| {}));
    let second = harness.state().project().graph_entries()[1].id;
    let card = graph_card(&mut harness, "Second graph");
    let to = header_point(&harness, 0);
    drag(&mut harness, card, to);
    harness.run_steps(2);
    assert_eq!(harness.state().project().tracks[0].fx, [Fx::new(second)]);
    // The chain opens, to wire up the new FX.
    assert_eq!(
        harness.state().fx_window(),
        Some(&FxTarget::Track("video".into()))
    );
    assert!(harness.query_by_label("FX: video").is_some());

    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::Z);
    assert!(harness.state().project().tracks[0].fx.is_empty());
}

#[test]
fn dropping_a_graph_on_an_item_or_below_the_tracks_picks_that_chain() {
    let mut harness = loaded_with(fx_app(|_, _, _| {}));
    let card = graph_card(&mut harness, "Second graph");
    let item = timeline_point(&harness, 0.5, 1);
    drag(&mut harness, card, item);
    harness.run_steps(2);
    assert_eq!(harness.state().project().audios()[0].items[0].fx.len(), 1);
    assert!(harness.state().project().audios()[0].fx.is_empty());

    let card = graph_card(&mut harness, "Second graph");
    let below = timeline_point(&harness, 0.5, 2);
    drag(&mut harness, card, below);
    harness.run_steps(2);
    assert_eq!(harness.state().project().master_fx.len(), 1);
    assert_eq!(harness.state().fx_window(), Some(&FxTarget::Master));
}

#[test]
fn header_fx_buttons_open_their_chains() {
    let mut harness = loaded_with(fx_app(|project, first, _| {
        project.tracks[1].fx.push(Fx::new(first));
    }));
    let area = harness.state().timeline_area();
    let buttons: Vec<Pos2> = harness
        .get_all_by_label("FX")
        .map(|b| b.rect().center())
        .filter(|c| area.contains(*c))
        .collect();
    assert_eq!(buttons.len(), 2, "one per track");
    click(&mut harness, buttons[1], PointerButton::Primary);
    harness.run_steps(2);
    assert_eq!(
        harness.state().fx_window(),
        Some(&FxTarget::Track("audio".into()))
    );
    harness.get_by_label("Master FX").click();
    harness.run_steps(2);
    assert_eq!(harness.state().fx_window(), Some(&FxTarget::Master));
    assert!(harness.query_by_label_contains("No FX yet").is_some());
}

#[test]
fn the_fx_window_bypasses_reorders_and_removes() {
    let mut harness = loaded_with(fx_app(|project, first, second| {
        project.master_fx = vec![Fx::new(first), Fx::new(second)];
    }));
    harness.get_by_label("Master FX").click();
    harness.run_steps(2);
    let order = |harness: &Harness<'_, App>| -> Vec<u32> {
        harness
            .state()
            .project()
            .master_fx
            .iter()
            .map(|f| f.graph)
            .collect()
    };
    let (first, second) = (order(&harness)[0], order(&harness)[1]);
    harness
        .get_all_by_role(egui::accesskit::Role::CheckBox)
        .next()
        .unwrap()
        .click();
    harness.run_steps(2);
    assert!(harness.state().project().master_fx[0].bypass);

    // The second FX's up arrow (the first one's is disabled).
    harness.get_all_by_label("⏶").nth(1).unwrap().click();
    harness.run_steps(2);
    assert_eq!(order(&harness), [second, first]);

    let area = harness.state().timeline_area();
    let remove = harness
        .get_all_by_label("×")
        .map(|b| b.rect().center())
        .find(|c| !area.contains(*c))
        .unwrap();
    click(&mut harness, remove, PointerButton::Primary);
    harness.run_steps(2);
    assert_eq!(order(&harness), [first]);
    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::Z);
    assert_eq!(order(&harness), [second, first]);
}

#[test]
fn the_fx_window_fills_ports_from_tracks_and_adds_graphs() {
    let mut harness = loaded_with(fx_app(|project, _, second| {
        project.master_fx = vec![Fx::new(second)];
    }));
    harness.get_by_label("Master FX").click();
    harness.run_steps(2);
    // One combo box: the Side port; the main ports are filled by what the FX is on.
    // The combo box on the Side port's line.
    let side = harness.get_by_label("Side").rect().center();
    let combo = harness
        .get_all_by_role(egui::accesskit::Role::ComboBox)
        .map(|c| c.rect().center())
        .min_by(|a, b| (a.y - side.y).abs().total_cmp(&(b.y - side.y).abs()))
        .unwrap();
    click(&mut harness, combo, PointerButton::Primary);
    harness.run_steps(2);
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "audio")
        .click();
    harness.run_steps(2);
    let receive = |harness: &Harness<'_, App>| {
        harness.state().project().master_fx[0]
            .receives
            .get("Side")
            .cloned()
    };
    assert_eq!(receive(&harness).as_deref(), Some("audio"));
    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::Z);
    assert_eq!(receive(&harness), None);

    harness.get_by_label("+ Add FX").click();
    harness.run_steps(2);
    let open = harness.state().project().graph_name.clone();
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, &open)
        .click();
    harness.run_steps(2);
    assert_eq!(harness.state().project().master_fx.len(), 2);
    // Neither graph has an Audio Output, so both pass the sound through.
    assert_eq!(
        harness.query_all_by_label_contains("sound through").count(),
        2
    );
}

#[test]
fn a_receive_from_a_missing_track_shows_a_note() {
    let mut harness = loaded_with(fx_app(|project, _, second| {
        let mut fx = Fx::new(second);
        fx.receives.insert("Side".into(), "vanished".into());
        project.master_fx = vec![fx];
    }));
    harness.get_by_label("Master FX").click();
    harness.run_steps(2);
    assert!(
        harness
            .query_by_label_contains("\"vanished\" no longer exists")
            .is_some()
    );
}

#[test]
fn double_clicking_an_fx_opens_its_graph() {
    let app = fx_app(|project, _, second| {
        project.master_fx = vec![Fx::new(second)];
    });
    let mut harness = loaded_quick(app);
    harness.get_by_label("Master FX").click();
    harness.run_steps(2);
    let second = harness.state().project().master_fx[0].graph;
    assert_ne!(harness.state().project().graph_id, second);
    let name = harness.get_by_label("Second graph").rect().center();
    // Long enough after the button's click that the next two aren't taken for a triple click.
    harness.run_steps(40);
    double_click(&mut harness, name);
    assert_eq!(harness.state().project().graph_id, second);
}

#[test]
fn an_items_menu_opens_its_fx_and_turns_pre_roll_off() {
    let mut harness = loaded_with(fx_app(|_, _, _| {}));
    let mut bar = timeline_point(&harness, 0.5, 1);
    bar.y = harness.state().timeline_area().top() + 22.0 + LANE_HEIGHT + 8.0;
    right_click(&mut harness, bar);
    harness.get_by_label("Pre-roll FX").click();
    harness.run_steps(2);
    assert!(!harness.state().project().audios()[0].items[0].pre_roll);
    right_click(&mut harness, bar);
    harness.get_by_label("FX…").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().fx_window(),
        Some(&FxTarget::Item(rastersong_engine::ItemRef::new("audio", 0)))
    );
    // The item's chain has its pre-roll switch too.
    assert!(harness.query_by_label("Pre-roll FX").is_some());
}

#[test]
fn without_fx_the_preview_plays_the_track_mix() {
    // An open graph that renders black: nothing feeds its output.
    let black = |project: &mut Project| project.graph.connections.clear();
    let mut harness = loaded_with(app_with_project(black));
    let frame = harness.state().engine().frame(0).unwrap();
    assert!(
        frame.rgb.iter().any(|&b| b != 0),
        "no FX, so the video shows"
    );

    // On the master, the graph applies.
    let graph = harness.state().project().graph_id;
    harness.state_mut().drop_graph(graph, FxDrop::Master);
    step_until(&mut harness, "the graph rendered", |app| {
        app.engine()
            .frame(0)
            .is_some_and(|f| f.rgb.iter().all(|&b| b == 0))
    });
}

// --- Track headers ---

fn track_names(tracks: Vec<&rastersong_engine::ProjectTrack>) -> Vec<String> {
    tracks.iter().map(|t| t.name.clone()).collect()
}

/// The starter project with a second video track and a second audio track.
fn four_track_app() -> App {
    app_with_project(|project| {
        project.add_track(TrackKind::Video, "video 2", "clip");
        project.add_track(TrackKind::Audio, "audio 2", "song");
    })
}

#[test]
fn dragging_a_header_moves_the_track_among_the_others() {
    let mut harness = loaded_with(four_track_app());
    // Rows: video, audio, video 2, audio 2.
    assert_eq!(
        track_names(harness.state().project().videos()),
        ["video", "video 2"]
    );
    // Dragged by the kind icon, which is a label over the header's background.
    let second_video = harness
        .get_all_by_label("▣")
        .nth(1)
        .unwrap()
        .rect()
        .center();
    let to = second_video - vec2(0.0, LANE_HEIGHT * 2.0);
    drag_to(&mut harness, second_video, to);
    // The drag bubble follows the pointer meanwhile.
    assert!(matches!(
        bubble_phase(&harness.ctx),
        Some(Phase::Dragging { .. })
    ));
    press(&mut harness, to, PointerButton::Primary, false);
    assert_eq!(
        track_names(harness.state().project().tracks().collect()),
        ["video 2", "video", "audio", "audio 2"]
    );

    // Audio and video tracks mix freely: the first audio goes to the bottom.
    harness.run_steps(2);
    let first_audio = harness
        .get_all_by_label("♪")
        .next()
        .unwrap()
        .rect()
        .center();
    drag(
        &mut harness,
        first_audio,
        first_audio + vec2(0.0, LANE_HEIGHT * 2.0),
    );
    assert_eq!(
        track_names(harness.state().project().tracks().collect()),
        ["video 2", "video", "audio 2", "audio"]
    );
}

/// The starter project's video and audio tracks under a folder `F` at the top.
fn folder_app() -> App {
    app_with_project(|project| {
        project
            .tracks
            .insert(0, rastersong_engine::ProjectTrack::new_folder("F".into()));
    })
}

#[test]
fn dragging_a_header_right_puts_the_track_in_the_folder_above() {
    let mut harness = loaded_with(folder_app());
    assert_eq!(harness.state().project().parent_of(1), None);
    let video = harness
        .get_all_by_label("▣")
        .next()
        .unwrap()
        .rect()
        .center();
    drag(&mut harness, video, video + vec2(30.0, 0.0));
    let project = harness.state().project();
    assert_eq!(project.tracks[1].name, "video");
    assert_eq!(project.tracks[1].depth, 1);
    assert_eq!(project.parent_of(1), Some(0));
    assert_eq!(project.parent_of(2), None, "the audio stays at the top");

    // Collapsing the folder hides what is in it.
    harness.get_by_label("⏷").click();
    harness.run_steps(2);
    assert!(harness.state().project().tracks[0].collapsed);
    assert_eq!(harness.query_all_by_label("▣").count(), 0);
    harness.get_by_label("⏵").click();
    harness.run_steps(2);
    assert_eq!(harness.get_all_by_label("▣").count(), 1);
}

#[test]
fn tracks_are_removed_from_their_header() {
    let mut harness = loaded_with(four_track_app());
    // The third row's × button: video 2.
    harness.get_all_by_label("×").nth(2).unwrap().click();
    harness.run_steps(2);
    assert_eq!(track_names(harness.state().project().videos()), ["video"]);

    // And from the header's right-click menu.
    let icon = harness
        .get_all_by_label("▣")
        .next()
        .unwrap()
        .rect()
        .center();
    right_click(&mut harness, icon);
    harness.get_by_label("Remove track").click();
    harness.run_steps(2);
    assert!(harness.state().project().videos().is_empty());
}

// --- Every interaction a hint promises (CONTRIBUTING rule 14) ---

use rastersong_engine::{LoopRegion, TimelineMode};

/// A point on the ruler at `seconds`.
fn ruler_point(harness: &Harness<'_, App>, seconds: f64) -> Pos2 {
    let mut point = timeline_point(harness, seconds, 0);
    point.y = harness.state().timeline_area().top() + 11.0;
    point
}

fn with_loop(project: &mut Project) {
    project.loop_region = Some(LoopRegion {
        start: 0.5,
        end: 1.0,
        enabled: true,
    });
}

#[test]
fn ctrl_dragging_a_loop_edge_moves_it() {
    let mut harness = loaded_with(app_with_project(with_loop));
    let from = ruler_point(&harness, 1.0);
    let to = ruler_point(&harness, 1.5);
    ctrl_drag(&mut harness, from, to);
    let region = harness.state().project().loop_region.unwrap();
    assert!((region.start - 0.5).abs() < 1e-6, "{region:?}");
    assert!((region.end - 1.5).abs() < 0.05, "{region:?}");
}

#[test]
fn right_clicking_the_ruler_offers_looping_options() {
    let mut harness = loaded_with(app_with_project(with_loop));
    let spot = ruler_point(&harness, 1.5);
    right_click(&mut harness, spot);
    harness.get_by_label_contains("Loop playback (R)").click();
    harness.run_steps(2);
    assert!(!harness.state().project().loop_region.unwrap().enabled);
    right_click(&mut harness, spot);
    harness.get_by_label("Remove loop region").click();
    harness.run_steps(2);
    assert!(harness.state().project().loop_region.is_none());
}

#[test]
fn clicking_the_ruler_mode_switches_between_time_and_tempo() {
    let mut harness = loaded();
    assert_eq!(harness.state().project().timeline_mode, TimelineMode::Time);
    harness.get_by_label("⏱ Time").click();
    harness.run_steps(2);
    assert_eq!(harness.state().project().timeline_mode, TimelineMode::Tempo);
    harness.get_by_label("♪ Tempo").click();
    harness.run_steps(2);
    assert_eq!(harness.state().project().timeline_mode, TimelineMode::Time);
}

#[test]
fn the_header_menu_links_and_unlinks_tracks() {
    let mut harness = loaded();
    let icon = harness.get_by_label("▣").rect().center();
    right_click(&mut harness, icon);
    harness.get_by_label_contains("Link with").click();
    harness.run_steps(2);
    harness.get_by_label("audio").click();
    harness.run_steps(2);
    assert_eq!(harness.state().project().linked_to("video"), ["audio"]);

    right_click(&mut harness, icon);
    harness.get_by_label("Unlink").click();
    harness.run_steps(2);
    assert!(harness.state().project().linked_to("video").is_empty());
}

/// A primary click with `modifiers` held.
fn modifier_click(harness: &mut Harness<'_, App>, modifiers: Modifiers, pos: Pos2) {
    harness.event(Event::ModifiersChanged(modifiers));
    harness.event(Event::PointerMoved(pos));
    harness.run_steps(1);
    for pressed in [true, false] {
        harness.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        });
        harness.run_steps(1);
    }
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
    harness.run_steps(1);
}

#[test]
fn ctrl_clicking_items_adds_them_and_their_menu_acts_on_the_selection() {
    let mut harness = loaded();
    let video = item_bar_point(&harness, 0.5, 0);
    let audio = item_bar_point(&harness, 0.5, 1);
    click(&mut harness, video, PointerButton::Primary);
    assert_eq!(harness.state().selected_items().len(), 1);
    modifier_click(&mut harness, Modifiers::COMMAND, audio);
    assert_eq!(harness.state().selected_items().len(), 2);

    right_click(&mut harness, audio);
    harness.get_by_label_contains("Delete").click();
    harness.run_steps(2);
    let project = harness.state().project();
    assert!(project.videos()[0].items.is_empty());
    assert!(project.audios()[0].items.is_empty());
}

#[test]
fn alt_dragging_an_items_edge_changes_its_rate() {
    let mut harness = loaded();
    let from = timeline_point(&harness, 2.0, 1);
    let to = timeline_point(&harness, 1.0, 1);
    modifier_drag(&mut harness, Modifiers::ALT, from, to);
    let item = harness.state().project().audios()[0].items[0].clone();
    assert!((item.rate - 2.0).abs() < 0.05, "rate {}", item.rate);
}

#[test]
fn shift_drags_an_edge_without_snapping() {
    let mut harness = loaded();
    seek_on_ruler(&mut harness, 1.0);
    // A few pixels past the playhead: close enough to snap to it.
    let near = 1.0 + 4.0 / harness.state().timeline_view().px_per_sec;
    let from = timeline_point(&harness, 2.0, 1);
    let to = timeline_point(&harness, near, 1);
    drag(&mut harness, from, to);
    let end = harness.state().project().audios()[0].items[0].end.unwrap();
    assert!((end - 1.0).abs() < 1e-6, "snapped to the playhead: {end}");

    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::Z);
    modifier_drag(&mut harness, Modifiers::SHIFT, from, to);
    let end = harness.state().project().audios()[0].items[0].end.unwrap();
    assert!((end - near).abs() < 0.01, "free: {end}");
}

#[test]
fn dragging_a_media_card_onto_the_timeline_makes_a_track() {
    let mut harness = loaded();
    let name = harness.state().project().resources[0].name.clone();
    let card = harness
        .get_all_by_label(&name)
        .next()
        .unwrap()
        .rect()
        .center();
    // Below the tracks, off every lane.
    let mut to = timeline_point(&harness, 0.5, 1);
    to.y += LANE_HEIGHT * 1.5;
    drag(&mut harness, card, to);
    harness.run_steps(2);
    assert_eq!(harness.state().project().videos().len(), 2);
}

/// The middle of the first wire of the graph, which hovering inspects.
fn wire_middle(harness: &Harness<'_, App>) -> Pos2 {
    let editor = harness.state().editor();
    let wire = editor.wires()[0];
    let from = editor
        .pin_screen_pos(wire.from.0, false, wire.from.1)
        .unwrap();
    let to = editor.pin_screen_pos(wire.to.0, true, wire.to.1).unwrap();
    // Wires leave and enter their pins level, so the curve passes through the middle.
    from + (to - from) * 0.5
}

/// Rests the pointer on the first wire until the editor reports it hovered.
fn hover_wire(harness: &mut Harness<'_, App>) {
    let at = wire_middle(harness);
    harness.event(Event::PointerMoved(at));
    step_until(harness, "the wire hovered", |app| {
        app.editor().hovered_output().is_some()
    });
}

#[test]
fn holding_alt_and_scrolling_over_a_connection_changes_its_view() {
    let mut harness = loaded();
    hover_wire(&mut harness);
    let before = harness.state().settings().views;
    harness.event(Event::ModifiersChanged(Modifiers::ALT));
    harness.run_steps(1);
    harness.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, -1.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::ALT,
    });
    harness.run_steps(2);
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
    harness.run_steps(1);
    assert_ne!(harness.state().settings().views, before);
}

#[test]
fn holding_shift_over_a_connection_listens_to_it() {
    let mut harness = loaded();
    hover_wire(&mut harness);
    assert!(!harness.state().is_listening());
    harness.event(Event::ModifiersChanged(Modifiers::SHIFT));
    harness.run_steps(2);
    assert!(harness.state().is_listening());
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
    harness.run_steps(2);
    assert!(!harness.state().is_listening());
}

#[test]
fn copies_keep_input_connections_and_shift_does_the_opposite_once() {
    let mut harness = loaded();
    assert!(harness.state().settings().keep_connections);
    select(&mut harness, "split");
    let split = key(&harness, "split");
    let wires = |harness: &Harness<'_, App>| harness.state().editor().wires().len();
    let feeding = harness
        .state()
        .editor()
        .wires()
        .iter()
        .filter(|w| w.to.0 == split)
        .count();
    assert!(feeding > 0);

    // Ctrl+D: the copy is fed like the original.
    let before = wires(&harness);
    shortcut(&mut harness, Modifiers::COMMAND, egui::Key::D);
    assert_eq!(wires(&harness), before + feeding);
    // Ctrl+Shift+D (the copy is selected now, fed the same): its inputs are left unconnected.
    let before = wires(&harness);
    shortcut(
        &mut harness,
        Modifiers::COMMAND | Modifiers::SHIFT,
        egui::Key::D,
    );
    assert_eq!(wires(&harness), before);

    // The same for pasting, with Shift held for Ctrl+Shift+V.
    select(&mut harness, "split");
    harness.event(Event::Copy);
    harness.run_steps(2);
    let text = harness.state().editor().clipboard().unwrap().to_owned();
    let canvas = harness.state().editor().canvas_rect();
    harness.event(Event::PointerMoved(pos2(
        canvas.left() + 40.0,
        canvas.bottom() - 90.0,
    )));
    let before = wires(&harness);
    harness.event(Event::Paste(text.clone()));
    harness.run_steps(2);
    assert_eq!(wires(&harness), before + feeding);
    let before = wires(&harness);
    harness.event(Event::ModifiersChanged(
        Modifiers::COMMAND | Modifiers::SHIFT,
    ));
    harness.event(Event::Paste(text));
    harness.run_steps(2);
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
    harness.run_steps(1);
    assert_eq!(wires(&harness), before);
}

#[test]
fn the_split_divider_drags_and_unprocessed_swaps_the_sides() {
    use rastersong_gui::preview::Feed;
    let mut harness = loaded();
    harness.get_by_label("Split").click();
    harness.run_steps(2);
    let (position, sides) = harness.state().preview_split().expect("split on");
    assert_eq!(sides, (Feed::Unprocessed, Feed::Processed));

    let rect = harness.state().preview_rect();
    let divider = pos2(rect.left() + rect.width() * position, rect.center().y);
    drag(
        &mut harness,
        divider,
        divider + vec2(rect.width() * 0.2, 0.0),
    );
    let (moved, _) = harness.state().preview_split().unwrap();
    assert!(
        (moved - (position + 0.2)).abs() < 0.02,
        "{position} -> {moved}"
    );

    harness.get_by_label("Unprocessed").click();
    harness.run_steps(2);
    let (_, sides) = harness.state().preview_split().unwrap();
    assert_eq!(sides, (Feed::Processed, Feed::Unprocessed));
}

#[test]
fn clicking_the_error_bar_shows_the_node_at_fault() {
    let app = app_with_project(|project| {
        project.graph = GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [ { "id": "v", "type": "video_input" },
                { "id": "c", "type": "pack", "params": { "channels": 7 } },
                { "id": "o", "type": "output" } ],
              "connections": [ { "from": "v", "to": "c" }, { "from": "c", "to": "o" } ] }"#,
        )
        .unwrap();
        let graph = project.graph_id;
        project.master_fx.push(rastersong_engine::Fx::new(graph));
    });
    let mut harness = harness(app);
    step_until(&mut harness, "the failure", |app| {
        matches!(
            app.engine().status(),
            rastersong_engine::EngineStatus::Failed(_)
        )
    });
    harness.run_steps(2);
    assert!(harness.state().editor().active().is_none());
    // On the message itself, which runs along the bar.
    let canvas = harness.state().editor().canvas_rect();
    let bar = pos2(canvas.left() + 60.0, canvas.bottom() - 15.0);
    click(&mut harness, bar, PointerButton::Primary);
    assert_eq!(
        harness.state().editor().active().map(|n| n.id.as_str()),
        Some("c")
    );
}

#[test]
fn the_add_track_menu_adds_empty_tracks_and_folders() {
    let mut harness = loaded();
    let before = harness.state().project().tracks.len();
    harness.get_by_label("+ Track").click();
    harness.run_steps(2);
    harness.get_by_label("Empty track").click();
    harness.run_steps(2);
    harness.get_by_label("+ Track").click();
    harness.run_steps(2);
    harness.get_by_label("Folder").click();
    harness.run_steps(2);
    let project = harness.state().project();
    assert_eq!(project.tracks.len(), before + 2);
    let empty = &project.tracks[before];
    assert!(empty.resource.is_none() && !empty.folder);
    assert!(project.tracks[before + 1].folder);
}

// --- The drag bubble ---

use rastersong_gui::widgets::{Phase, bubble_phase};

/// Presses on `from` and moves to `to`, keeping the button down.
fn drag_to(harness: &mut Harness<'_, App>, from: Pos2, to: Pos2) {
    harness.event(Event::PointerMoved(from));
    harness.run_steps(1);
    press(harness, from, PointerButton::Primary, true);
    for t in [0.25, 0.5, 0.75, 1.0] {
        harness.event(Event::PointerMoved(from + (to - from) * t));
        harness.run_steps(1);
    }
}

#[test]
fn a_dragged_card_shows_the_bubble_which_blooms_where_it_is_dropped() {
    let mut harness = loaded();
    let name = harness.state().project().resources[0].name.clone();
    let card = harness
        .get_all_by_label(&name)
        .next()
        .unwrap()
        .rect()
        .center();
    let mut to = timeline_point(&harness, 0.5, 1);
    to.y += LANE_HEIGHT * 1.5;
    drag_to(&mut harness, card, to);
    assert!(matches!(
        bubble_phase(&harness.ctx),
        Some(Phase::Dragging { .. })
    ));
    press(&mut harness, to, PointerButton::Primary, false);
    assert!(
        matches!(bubble_phase(&harness.ctx), Some(Phase::Dropped { .. })),
        "{:?}",
        bubble_phase(&harness.ctx)
    );
    assert_eq!(harness.state().project().videos().len(), 2);
    // The bloom fades and the bubble goes.
    harness.run_steps(30);
    assert!(bubble_phase(&harness.ctx).is_none());
}

#[test]
fn a_card_dropped_where_nothing_takes_it_sends_the_bubble_back() {
    let mut harness = loaded();
    let name = harness.state().project().resources[0].name.clone();
    let card = harness
        .get_all_by_label(&name)
        .next()
        .unwrap()
        .rect()
        .center();
    // The preview takes no resources.
    let to = harness.state().preview_rect().center();
    drag_to(&mut harness, card, to);
    press(&mut harness, to, PointerButton::Primary, false);
    assert!(
        matches!(bubble_phase(&harness.ctx), Some(Phase::Cancelled { .. })),
        "{:?}",
        bubble_phase(&harness.ctx)
    );
    assert_eq!(harness.state().project().videos().len(), 1);
}

// --- The drop preview ---

/// Where text reading `text` was painted over the timeline's lanes this frame.
fn painted_text(harness: &Harness<'_, App>, text: &str) -> Vec<Pos2> {
    let lanes_left = harness.state().timeline_area().left() + HEADER_WIDTH;
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(t) if t.galley.text() == text && t.pos.x > lanes_left => Some(t.pos),
            _ => None,
        })
        .collect()
}

#[test]
fn a_dragged_resource_shows_its_ghost_and_lands_where_it_was_drawn() {
    let mut harness = loaded();
    seek_on_ruler(&mut harness, 1.0);
    let song = harness.state().project().audios()[0].resource.unwrap();
    let name = harness
        .state()
        .project()
        .resource(song)
        .unwrap()
        .name
        .clone();
    let card = harness
        .get_all_by_label(&name)
        .next()
        .unwrap()
        .rect()
        .center();
    // Over the song's own track, a few pixels after the playhead: it snaps to it.
    let near = 1.0 + 3.0 / harness.state().timeline_view().px_per_sec;
    let to = timeline_point(&harness, near, 1);
    drag_to(&mut harness, card, to);
    harness.run_steps(1);
    let playhead_x = timeline_point(&harness, 1.0, 1).x;
    let ghost = painted_text(&harness, &name);
    assert!(
        ghost.iter().any(|p| (p.x - (playhead_x + 6.0)).abs() < 1.0),
        "the ghost is drawn at the playhead: {ghost:?} vs {playhead_x}"
    );
    press(&mut harness, to, PointerButton::Primary, false);
    let items = &harness.state().project().audios()[0].items;
    assert_eq!(items.len(), 2, "it landed on the song's track");
    assert!((items[1].position - 1.0).abs() < 1e-9, "{items:?}");
}

#[test]
fn a_resource_no_track_takes_shows_a_ghost_row_and_makes_a_track() {
    let mut harness = loaded();
    let video = harness.state().project().videos()[0].resource.unwrap();
    let name = harness
        .state()
        .project()
        .resource(video)
        .unwrap()
        .name
        .clone();
    let card = harness
        .get_all_by_label(&name)
        .next()
        .unwrap()
        .rect()
        .center();
    // The video over the audio track, which can't take it.
    let to = timeline_point(&harness, 0.5, 1);
    drag_to(&mut harness, card, to);
    harness.run_steps(1);
    // The ghost row sits where the new video track will go: with no folder taking videos,
    // at the bottom, under the audio.
    let ghost = painted_text(&harness, &name);
    let boundary = harness.state().timeline_area().top() + 22.0 + 2.0 * LANE_HEIGHT;
    assert!(
        ghost.iter().any(|p| (p.y - boundary).abs() < 20.0),
        "{ghost:?} vs {boundary}"
    );
    press(&mut harness, to, PointerButton::Primary, false);
    assert_eq!(harness.state().project().videos().len(), 2);
}

#[test]
fn a_dragged_graph_highlights_the_chain_it_would_join() {
    let mut harness = loaded_with(fx_app(|_, _, _| {}));
    let card = graph_card(&mut harness, "Second graph");
    let to = timeline_point(&harness, 0.5, 1);
    drag_to(&mut harness, card, to);
    harness.run_steps(1);
    let lane_top = harness.state().timeline_area().top() + 22.0 + LANE_HEIGHT;
    let ghost = painted_text(&harness, "Add Second graph as an FX here");
    assert!(
        ghost
            .iter()
            .any(|p| p.y > lane_top && p.y < lane_top + LANE_HEIGHT),
        "{ghost:?}"
    );
    press(&mut harness, to, PointerButton::Primary, false);
    assert_eq!(harness.state().project().audios()[0].items[0].fx.len(), 1);
}
