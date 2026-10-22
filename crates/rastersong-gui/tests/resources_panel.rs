//! The Resources panel on its own: tabs, the card grid, the Relocate button, renaming, and the
//! clicks and menus the cards' hints promise.

use std::collections::HashSet;

use eframe::egui::{self, Event, PointerButton, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rastersong_engine::{GraphDesc, Project, ResourceId, TrackKind};
use rastersong_gui::STARTER_GRAPH;
use rastersong_gui::resources::{ResourceAction, resources_panel};

struct State {
    project: Project,
    missing: HashSet<ResourceId>,
    actions: Vec<ResourceAction>,
}

fn harness(missing_first: bool) -> Harness<'static, State> {
    harness_with(missing_first, Harness::builder())
}

/// The panel with short steps, so two clicks land inside egui's double-click delay.
fn quick_harness() -> Harness<'static, State> {
    harness_with(false, Harness::builder().with_step_dt(0.02))
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
    let state = State {
        project,
        missing,
        actions: Vec::new(),
    };
    builder.with_size(vec2(260.0, 400.0)).build_ui_state(
        |ui, state: &mut State| {
            let actions = resources_panel(ui, &state.project, &state.missing);
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

#[test]
fn media_cards_form_a_grid() {
    let mut harness = harness(false);
    harness.run_steps(2);
    let names: Vec<_> = harness
        .state()
        .project
        .resources
        .iter()
        .map(|r| r.name.clone())
        .collect();
    assert!(names.len() >= 2);
    let a = harness.get_by_label(&names[0]).rect();
    let b = harness.get_by_label(&names[1]).rect();
    // A 260 px wide panel fits two cards side by side: the second is beside the first.
    assert!((a.center().y - b.center().y).abs() < 4.0, "{a:?} {b:?}");
    assert!(b.left() > a.right() - 1.0);
}

#[test]
fn media_and_graphs_are_separate_tabs() {
    let mut harness = harness(false);
    harness.run_steps(2);
    let media = harness.state().project.resources[0].name.clone();
    let graph = harness.state().project.graph_entries()[1].name.clone();
    assert!(harness.query_by_label(&media).is_some());
    assert!(harness.query_by_label(&graph).is_none());
    harness.get_by_label("Graphs").click();
    harness.run_steps(2);
    assert!(harness.query_by_label(&media).is_none());
    assert!(harness.query_by_label(&graph).is_some());
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
            harness.get_by_label("Graphs").click();
            harness.run_steps(2);
            harness.state().project.graph_entries()[1].name.clone()
        } else {
            harness.state().project.resources[0].name.clone()
        };
        harness.get_by_label(&name).click_secondary();
        harness.run_steps(2);
        let field = harness
            .query_by_role(egui::accesskit::Role::TextInput)
            .expect("the rename field is in the menu")
            .rect()
            .center();
        click_at(&mut harness, field, PointerButton::Primary);
        harness.run_steps(2);
        assert!(
            harness
                .query_by_role(egui::accesskit::Role::TextInput)
                .is_some(),
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
    harness
        .get_by_role(egui::accesskit::Role::TextInput)
        .focus();
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
    let tab = harness.get_by_label("Graphs").rect().center();
    click_at(&mut harness, tab, PointerButton::Primary);
    let entry = harness.state().project.graph_entries()[1].clone();
    let card = harness.get_by_label(&entry.name).rect().center();
    // Long enough after the tab's click that the card's clicks aren't counted with it.
    harness.run_steps(50);
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
    harness.get_by_label("Graphs").click();
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
