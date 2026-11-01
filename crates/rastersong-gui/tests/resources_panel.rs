//! The Resources panel on its own: the two tabs, the card grid, the filter, the cards' details,
//! the Relocate button, renaming in place, and the clicks and menus the cards' hints promise.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Event, PointerButton, vec2};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{By, Queryable};
use rastersong_engine::{GraphDesc, Project, ResourceId, StreamInfo, StreamKind, TrackKind};
use rastersong_gui::STARTER_GRAPH;
use rastersong_gui::resources::{ResourceAction, resources_panel};

struct State {
    project: Project,
    missing: HashSet<ResourceId>,
    streams: HashMap<std::path::PathBuf, Vec<StreamInfo>>,
    actions: Vec<ResourceAction>,
    /// The "open graphs with a single click" setting.
    open_on_click: bool,
}

/// A narrow panel: two cards to a row.
fn harness(missing_first: bool) -> Harness<'static, State> {
    harness_with(
        missing_first,
        Harness::builder().with_size(vec2(260.0, 500.0)),
    )
}

/// The panel with short steps, so two clicks land inside egui's double-click delay.
fn quick_harness() -> Harness<'static, State> {
    harness_with(
        false,
        Harness::builder()
            .with_step_dt(0.02)
            .with_size(vec2(260.0, 500.0)),
    )
}

fn harness_with(
    missing_first: bool,
    builder: egui_kittest::HarnessBuilder<State>,
) -> Harness<'static, State> {
    let mut project = Project::new(GraphDesc::from_json(STARTER_GRAPH).unwrap());
    project.add_track(TrackKind::Video, "video", "clip");
    project.add_track(TrackKind::Audio, "audio", "song");
    project.add_graph("Second graph", None);
    let mut missing = HashSet::new();
    if missing_first {
        missing.insert(project.resources[0].id);
    }
    // What the app would have read from the files: the video's shape and the song's.
    let stream = |kind| StreamInfo {
        index: 0,
        kind,
        codec: "fake".into(),
        title: None,
        language: None,
        default: true,
    };
    let mut streams = HashMap::new();
    streams.insert(
        project.resources[0].path.clone(),
        vec![stream(StreamKind::Video {
            width: 320,
            height: 180,
            frame_rate: 30.0,
        })],
    );
    streams.insert(
        project.resources[1].path.clone(),
        vec![stream(StreamKind::Audio {
            sample_rate: 44_100,
            channels: 2,
        })],
    );
    let state = State {
        project,
        missing,
        streams,
        actions: Vec::new(),
        open_on_click: false,
    };
    builder.build_ui_state(
        |ui, state: &mut State| {
            // The app's fonts are only there from the frame after they're set.
            if ui.ctx().cumulative_frame_nr() == 0 {
                rastersong_gui::theme::apply_style(ui.ctx());
                return;
            }
            let actions = resources_panel(
                ui,
                &state.project,
                &state.missing,
                &state.streams,
                state.open_on_click,
            );
            state.actions.extend(actions);
        },
        state,
    )
}

fn click_at(harness: &mut Harness<'_, State>, pos: egui::Pos2, button: PointerButton) {
    harness.event(Event::PointerMoved(pos));
    harness.run_steps(1);
    for pressed in [true, false] {
        harness.event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Default::default(),
        });
        harness.run_steps(1);
    }
    harness.run_steps(1);
}

fn double_click_at(harness: &mut Harness<'_, State>, pos: egui::Pos2) {
    harness.event(Event::PointerMoved(pos));
    harness.run_steps(1);
    for _ in 0..2 {
        for pressed in [true, false] {
            harness.event(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
            harness.run_steps(1);
        }
    }
    harness.run_steps(1);
}

/// Types `text` into the focused field (its text all selected) and presses `key`.
fn type_then(harness: &mut Harness<'_, State>, text: &str, key: egui::Key) {
    harness.event(Event::Text(text.into()));
    harness.run_steps(1);
    harness.key_press(key);
    harness.run_steps(2);
}

/// The rename field: the text field that isn't the filter.
fn rename_field<'h>(harness: &'h Harness<'_, State>) -> Option<egui_kittest::Node<'h>> {
    harness.query(
        By::new()
            .role(Role::TextInput)
            .predicate(|n| n.placeholder() != Some("Filter")),
    )
}

/// The card whose detail line reads `detail`.
fn card_detailed<'h>(
    harness: &'h Harness<'_, State>,
    detail: &'h str,
) -> Option<egui_kittest::Node<'h>> {
    harness.query(By::new().predicate(move |n| n.description().as_deref() == Some(detail)))
}

/// Where the card named `name` is.
fn card(harness: &Harness<'_, State>, name: &str) -> egui::Rect {
    harness.get_by_label(name).rect()
}

/// A point on the name line along the bottom of the card named `name`.
fn name_of(harness: &Harness<'_, State>, name: &str) -> egui::Pos2 {
    let rect = card(harness, name);
    egui::pos2(rect.center().x, rect.bottom() - 30.0)
}

/// Opens the Graphs tab, then waits out egui's double-click delay so the next click doesn't
/// pair up with this one.
fn graphs_tab(harness: &mut Harness<'_, State>) {
    harness.get_by_label_contains("Graphs ·").click();
    harness.run_steps(30);
}

/// The graph that isn't open: its id and name.
fn graph_name(harness: &Harness<'_, State>) -> (u32, String) {
    let entry = harness.state().project.graph_entries()[1].clone();
    (entry.id, entry.name)
}

fn has(harness: &Harness<'_, State>, action: &ResourceAction) -> bool {
    harness.state().actions.contains(action)
}

#[test]
fn media_and_graphs_are_tabs_with_counts() {
    let mut harness = harness(false);
    harness.run_steps(2);
    let media = harness.state().project.resources[0].name.clone();
    let (_, graph) = graph_name(&harness);
    harness.get_by_label("Media · 2");
    harness.get_by_label("Graphs · 2");
    // The media tab is open: its cards show, the graphs' don't.
    assert!(harness.query_by_label(&media).is_some());
    assert!(harness.query_by_label(&graph).is_none());
    graphs_tab(&mut harness);
    assert!(harness.query_by_label(&media).is_none());
    assert!(harness.query_by_label(&graph).is_some());
}

#[test]
fn cards_are_wider_than_high_and_wrap_into_a_grid() {
    let mut harness = harness_with(false, Harness::builder().with_size(vec2(420.0, 400.0)));
    harness.run_steps(2);
    let names: Vec<String> = harness
        .state()
        .project
        .resources
        .iter()
        .map(|r| r.name.clone())
        .collect();
    let (video, audio) = (card(&harness, &names[0]), card(&harness, &names[1]));
    let ratio = video.height() / video.width();
    assert!((ratio - 0.75).abs() < 0.02, "{video:?}");
    // Side by side on one row, inside the panel.
    assert!(audio.left() > video.right(), "{video:?} {audio:?}");
    assert!((audio.top() - video.top()).abs() < 1.0);
    assert!(audio.right() <= 420.0);

    // Too narrow for two, they wrap: one above the other.
    let mut narrow = harness_with(false, Harness::builder().with_size(vec2(150.0, 500.0)));
    narrow.run_steps(2);
    let (video, audio) = (card(&narrow, &names[0]), card(&narrow, &names[1]));
    assert!(audio.top() > video.bottom(), "{video:?} {audio:?}");
}

#[test]
fn cards_say_what_each_resource_is() {
    let mut harness = harness(false);
    harness.run_steps(2);
    for detail in ["video · 320×180 · 30 fps", "audio · stereo · 44.1 kHz"] {
        assert!(card_detailed(&harness, detail).is_some(), "{detail}");
    }
    graphs_tab(&mut harness);
    // The open graph says so; neither graph is placed.
    for detail in ["open · not placed", "not placed"] {
        assert!(card_detailed(&harness, detail).is_some(), "{detail}");
    }
}

#[test]
fn a_missing_files_card_says_so() {
    let mut harness = harness(true);
    harness.run_steps(2);
    assert!(card_detailed(&harness, "missing").is_some());
    assert!(card_detailed(&harness, "video · 320×180 · 30 fps").is_none());
}

#[test]
fn the_filter_narrows_both_tabs() {
    let mut harness = harness(false);
    harness.run_steps(2);
    let media = harness.state().project.resources[0].name.clone();
    let (_, graph) = graph_name(&harness);
    harness
        .get(
            By::new()
                .role(Role::TextInput)
                .predicate(|n| n.placeholder() == Some("Filter")),
        )
        .focus();
    harness.run_steps(1);
    harness.event(Event::Text("SEC".into()));
    harness.run_steps(2);
    // Case doesn't matter; only "Second graph" contains it.
    assert!(harness.query_by_label(&media).is_none());
    harness.get_by_label("Media · 0");
    harness.get_by_label("Graphs · 1");
    harness.get_by_label("Nothing here matches the filter.");
    graphs_tab(&mut harness);
    assert!(harness.query_by_label(&graph).is_some());
}

#[test]
fn import_and_new_graph_make_their_actions() {
    let mut harness = harness(false);
    harness.run_steps(2);
    harness.get_by_label("Import…").click();
    graphs_tab(&mut harness);
    harness.get_by_label("+ New").click();
    harness.run_steps(2);
    assert!(has(&harness, &ResourceAction::Import));
    assert!(has(&harness, &ResourceAction::NewGraph));
}

#[test]
fn the_relocate_button_of_a_missing_file_is_clickable() {
    let mut harness = harness(true);
    harness.run_steps(2);
    let id = harness.state().project.resources[0].id;
    // A real pointer click, which the card's drag area under the button must not swallow.
    let at = harness.get_by_label("Relocate...").rect().center();
    click_at(&mut harness, at, PointerButton::Primary);
    assert!(
        has(&harness, &ResourceAction::Relocate(id)),
        "{:?}",
        harness.state().actions
    );
}

#[test]
fn double_clicking_a_media_card_adds_it_to_the_timeline() {
    let mut harness = quick_harness();
    harness.run_steps(2);
    let resource = &harness.state().project.resources[1];
    let (id, name) = (resource.id, resource.name.clone());
    // On the picture, away from the name.
    let picture = card(&harness, &name).center();
    double_click_at(&mut harness, picture);
    assert!(has(&harness, &ResourceAction::AddToTimeline(id)));
    assert!(rename_field(&harness).is_none());
}

#[test]
fn double_clicking_a_cards_name_renames_it_in_place() {
    for graphs in [false, true] {
        let mut harness = quick_harness();
        harness.run_steps(2);
        let (name, rename) = if graphs {
            graphs_tab(&mut harness);
            let (id, name) = graph_name(&harness);
            (name, ResourceAction::RenameGraph(id, "Renamed".into()))
        } else {
            let resource = &harness.state().project.resources[0];
            let id = resource.id;
            (
                resource.name.clone(),
                ResourceAction::Rename(id, "Renamed".into()),
            )
        };
        assert!(rename_field(&harness).is_none());
        let at = name_of(&harness, &name);
        double_click_at(&mut harness, at);
        harness.run_steps(1);
        assert!(rename_field(&harness).is_some(), "graphs: {graphs}");
        // Opening a graph or adding media is left to a double-click elsewhere on the card.
        assert!(
            !harness.state().actions.iter().any(|a| matches!(
                a,
                ResourceAction::OpenGraph(_) | ResourceAction::AddToTimeline(_)
            )),
            "{:?}",
            harness.state().actions
        );
        type_then(&mut harness, "Renamed", egui::Key::Enter);
        assert!(has(&harness, &rename), "{:?}", harness.state().actions);
        assert!(rename_field(&harness).is_none());
    }
}

#[test]
fn f2_renames_the_selected_card_and_escape_cancels() {
    for graphs in [false, true] {
        let mut harness = harness(false);
        harness.run_steps(2);
        let (name, rename) = if graphs {
            graphs_tab(&mut harness);
            let (id, name) = graph_name(&harness);
            (name, ResourceAction::RenameGraph(id, "F2 name".into()))
        } else {
            let resource = &harness.state().project.resources[1];
            let id = resource.id;
            (
                resource.name.clone(),
                ResourceAction::Rename(id, "F2 name".into()),
            )
        };
        let picture = card(&harness, &name).center();
        click_at(&mut harness, picture, PointerButton::Primary);
        harness.key_press(egui::Key::F2);
        harness.run_steps(2);
        assert!(rename_field(&harness).is_some(), "graphs: {graphs}");
        type_then(&mut harness, "nope", egui::Key::Escape);
        assert!(rename_field(&harness).is_none());
        assert!(
            harness.state().actions.is_empty(),
            "{:?}",
            harness.state().actions
        );

        harness.key_press(egui::Key::F2);
        harness.run_steps(2);
        type_then(&mut harness, "F2 name", egui::Key::Enter);
        assert!(has(&harness, &rename), "{:?}", harness.state().actions);
    }
}

#[test]
fn a_media_cards_menu_renames_and_removes_it() {
    let mut harness = harness(false);
    harness.run_steps(2);
    let resource = &harness.state().project.resources[0];
    let (id, name) = (resource.id, resource.name.clone());
    // The menu has no field of its own: Rename turns the name into one, as F2 does.
    harness.get_by_label(&name).click_secondary();
    harness.run_steps(2);
    assert!(rename_field(&harness).is_none());
    harness.get_by_label_contains("Rename").click();
    harness.run_steps(3);
    assert!(rename_field(&harness).is_some());
    type_then(&mut harness, "Renamed", egui::Key::Enter);
    assert!(has(&harness, &ResourceAction::Rename(id, "Renamed".into())));

    harness.get_by_label(&name).click_secondary();
    harness.run_steps(2);
    harness.get_by_label("Remove").click();
    harness.run_steps(2);
    assert!(has(&harness, &ResourceAction::Remove(id)));
}

#[test]
fn double_clicking_a_graph_card_opens_it() {
    let mut harness = quick_harness();
    harness.run_steps(2);
    graphs_tab(&mut harness);
    let (id, name) = graph_name(&harness);
    let picture = card(&harness, &name).center();
    // One click only selects it while the setting is off.
    click_at(&mut harness, picture, PointerButton::Primary);
    harness.run_steps(30);
    assert!(!has(&harness, &ResourceAction::OpenGraph(id)));
    double_click_at(&mut harness, picture);
    assert!(has(&harness, &ResourceAction::OpenGraph(id)));
}

#[test]
fn with_the_setting_one_click_opens_a_graph() {
    let mut harness = harness(false);
    harness.state_mut().open_on_click = true;
    harness.run_steps(2);
    graphs_tab(&mut harness);
    let (id, name) = graph_name(&harness);
    let picture = card(&harness, &name).center();
    click_at(&mut harness, picture, PointerButton::Primary);
    assert!(has(&harness, &ResourceAction::OpenGraph(id)));
}

#[test]
fn a_graph_cards_menu_duplicates_and_removes_it() {
    let mut harness = harness(false);
    harness.run_steps(2);
    graphs_tab(&mut harness);
    let (id, name) = graph_name(&harness);
    harness.get_by_label(&name).click_secondary();
    harness.run_steps(2);
    harness.get_by_label("Duplicate").click();
    harness.run_steps(2);
    harness.get_by_label(&name).click_secondary();
    harness.run_steps(2);
    harness.get_by_label("Remove").click();
    harness.run_steps(2);
    assert!(has(&harness, &ResourceAction::DuplicateGraph(id)));
    assert!(has(&harness, &ResourceAction::RemoveGraph(id)));
}
