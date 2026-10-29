//! The Resources panel on its own: the two groups, the filter, the rows' details, the Relocate
//! button, renaming, and the clicks and menus the rows' hints promise.

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
}

/// A panel too narrow for two columns: Media above Graphs.
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
    };
    builder.build_ui_state(
        |ui, state: &mut State| {
            // The app's fonts are only there from the frame after they're set.
            if ui.ctx().cumulative_frame_nr() == 0 {
                rastersong_gui::theme::apply_style(ui.ctx());
                return;
            }
            let actions = resources_panel(ui, &state.project, &state.missing, &state.streams);
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

/// The rename field of the open menu: the text field that isn't the filter.
fn rename_field<'h>(harness: &'h Harness<'_, State>) -> Option<egui_kittest::Node<'h>> {
    harness.query(
        By::new()
            .role(Role::TextInput)
            .predicate(|n| n.placeholder() != Some("Filter")),
    )
}

/// The row whose detail line reads `detail`.
fn row_detailed<'h>(
    harness: &'h Harness<'_, State>,
    detail: &'h str,
) -> Option<egui_kittest::Node<'h>> {
    harness.query(By::new().predicate(move |n| n.description().as_deref() == Some(detail)))
}

/// Where the row named `name` is.
fn row(harness: &Harness<'_, State>, name: &str) -> egui::Rect {
    harness.get_by_label(name).rect()
}

#[test]
fn media_and_graphs_show_together_with_counts() {
    let mut harness = harness(false);
    harness.run_steps(2);
    let media = harness.state().project.resources[0].name.clone();
    let graph = harness.state().project.graph_entries()[1].name.clone();
    harness.get_by_label("MEDIA · 2");
    harness.get_by_label("GRAPHS · 2");
    // Narrow, the groups stack: the graph's row is under the media's, as wide.
    let (m, g) = (row(&harness, &media), row(&harness, &graph));
    assert!(g.top() > m.bottom(), "{m:?} {g:?}");
    assert!((m.width() - g.width()).abs() < 1.0, "{m:?} {g:?}");
}

#[test]
fn a_wide_panel_puts_media_and_graphs_side_by_side() {
    let mut harness = harness_with(false, Harness::builder().with_size(vec2(420.0, 400.0)));
    harness.run_steps(2);
    let media = harness.state().project.resources[0].name.clone();
    let graph = harness.state().project.graph_entries()[1].name.clone();
    let (m, g) = (row(&harness, &media), row(&harness, &graph));
    assert!(g.left() > m.right(), "{m:?} {g:?}");
    assert!(m.right() <= 420.0 && g.right() <= 420.0);
}

#[test]
fn rows_say_what_each_resource_is() {
    let mut harness = harness(false);
    harness.run_steps(2);
    for detail in [
        "video · 320×180 · 30 fps",
        "audio · stereo · 44.1 kHz",
        // The open graph says so; neither graph is placed.
        "open · not placed",
        "not placed",
    ] {
        assert!(row_detailed(&harness, detail).is_some(), "{detail}");
    }
}

#[test]
fn a_missing_files_row_says_so() {
    let mut harness = harness(true);
    harness.run_steps(2);
    assert!(row_detailed(&harness, "missing").is_some());
    assert!(row_detailed(&harness, "video · 320×180 · 30 fps").is_none());
}

#[test]
fn the_filter_narrows_both_groups() {
    let mut harness = harness(false);
    harness.run_steps(2);
    let media = harness.state().project.resources[0].name.clone();
    let song = harness.state().project.resources[1].name.clone();
    let graph = harness.state().project.graph_entries()[1].name.clone();
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
    assert!(harness.query_by_label(&song).is_none());
    assert!(harness.query_by_label(&graph).is_some());
    harness.get_by_label("MEDIA · 0");
    harness.get_by_label("GRAPHS · 1");
    harness.get_by_label("Nothing here matches the filter.");
}

#[test]
fn the_headers_buttons_import_and_make_a_graph() {
    let mut harness = harness(false);
    harness.run_steps(2);
    harness.get_by_label("Import…").click();
    harness.get_by_label("+ New").click();
    harness.run_steps(2);
    let actions = &harness.state().actions;
    assert!(actions.contains(&ResourceAction::Import), "{actions:?}");
    assert!(actions.contains(&ResourceAction::NewGraph), "{actions:?}");
}

#[test]
fn the_relocate_button_of_a_missing_file_is_clickable() {
    let mut harness = harness(true);
    harness.run_steps(2);
    let id = harness.state().project.resources[0].id;
    // A real pointer click, which a drag source around the button would swallow.
    let at = harness.get_by_label("Relocate...").rect().center();
    click_at(&mut harness, at, PointerButton::Primary);
    assert!(
        harness
            .state()
            .actions
            .contains(&ResourceAction::Relocate(id)),
        "{:?}",
        harness.state().actions
    );
}

#[test]
fn clicking_the_rename_field_keeps_the_menu_open_for_media_and_graphs() {
    for graphs in [false, true] {
        let mut harness = harness(false);
        let name = if graphs {
            harness.state().project.graph_entries()[1].name.clone()
        } else {
            harness.state().project.resources[0].name.clone()
        };
        harness.get_by_label(&name).click_secondary();
        harness.run_steps(2);
        let field = rename_field(&harness)
            .expect("the rename field is in the menu")
            .rect()
            .center();
        click_at(&mut harness, field, PointerButton::Primary);
        harness.run_steps(2);
        assert!(
            rename_field(&harness).is_some(),
            "the menu closed when the field was clicked (graphs: {graphs})"
        );
    }
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

#[test]
fn double_clicking_a_media_card_adds_it_to_the_timeline() {
    let mut harness = quick_harness();
    harness.run_steps(2);
    let resource = &harness.state().project.resources[1];
    let (id, name) = (resource.id, resource.name.clone());
    let card = harness.get_by_label(&name).rect().center();
    double_click_at(&mut harness, card);
    assert!(
        harness
            .state()
            .actions
            .contains(&ResourceAction::AddToTimeline(id)),
        "{:?}",
        harness.state().actions
    );
}

#[test]
fn a_media_cards_menu_renames_and_removes_it() {
    let mut harness = harness(false);
    harness.run_steps(2);
    let resource = &harness.state().project.resources[0];
    let (id, name) = (resource.id, resource.name.clone());
    harness.get_by_label(&name).click_secondary();
    harness.run_steps(2);
    rename_field(&harness).unwrap().focus();
    harness.run_steps(1);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(Event::Text("Renamed".into()));
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert!(
        harness
            .state()
            .actions
            .contains(&ResourceAction::Rename(id, "Renamed".into())),
        "{:?}",
        harness.state().actions
    );

    // The menu stays open after a rename; the next right-click is on the card itself.
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    harness.get_by_label(&name).click_secondary();
    harness.run_steps(2);
    harness.get_by_label("Remove").click();
    harness.run_steps(2);
    assert!(
        harness
            .state()
            .actions
            .contains(&ResourceAction::Remove(id))
    );
}

#[test]
fn double_clicking_a_graph_card_opens_it() {
    let mut harness = quick_harness();
    harness.run_steps(2);
    let entry = harness.state().project.graph_entries()[1].clone();
    let card = harness.get_by_label(&entry.name).rect().center();
    double_click_at(&mut harness, card);
    assert!(
        harness
            .state()
            .actions
            .contains(&ResourceAction::OpenGraph(entry.id)),
        "{:?}",
        harness.state().actions
    );
}

#[test]
fn a_graph_cards_menu_duplicates_and_removes_it() {
    let mut harness = harness(false);
    harness.run_steps(2);
    let entry = harness.state().project.graph_entries()[1].clone();
    harness.get_by_label(&entry.name).click_secondary();
    harness.run_steps(2);
    harness.get_by_label("Duplicate").click();
    harness.run_steps(2);
    harness.get_by_label(&entry.name).click_secondary();
    harness.run_steps(2);
    harness.get_by_label("Remove").click();
    harness.run_steps(2);
    let actions = &harness.state().actions;
    assert!(actions.contains(&ResourceAction::DuplicateGraph(entry.id)));
    assert!(actions.contains(&ResourceAction::RemoveGraph(entry.id)));
}
