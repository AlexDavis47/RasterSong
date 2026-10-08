//! The node parameter field on its own, driven with real pointer events: every interaction its
//! hints promise (the slider track, the value box and the modulation amount knob).

use eframe::egui::{self, Color32, Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rastersong_engine::{ModMode, Modulation, ParamSpec};
use rastersong_gui::editor::param_field::{
    FieldResponse, GUTTER_WIDTH, Modulated, NumberRange, param_field,
};

const TRACK: f32 = 120.0;
const VALUE: f32 = 60.0;
const SPEC: ParamSpec = ParamSpec::number("amount", 0.5, 0.0, 1.0);
const RANGE: NumberRange = NumberRange {
    default: 0.5,
    soft: (0.0, 1.0),
    limits: (0.0, 1.0),
    whole: false,
};

struct State {
    value: f64,
    custom: Option<(f64, f64)>,
    modulation: Option<Modulation>,
    /// Where the field's row starts, and the spacing between its parts.
    left: Pos2,
    spacing: f32,
    height: f32,
    response: FieldResponse,
}

fn harness(modulation: Option<Modulation>) -> Harness<'static, State> {
    let state = State {
        value: 0.25,
        custom: None,
        modulation,
        left: Pos2::ZERO,
        spacing: 0.0,
        height: 0.0,
        response: FieldResponse::default(),
    };
    let mut harness = Harness::builder()
        .with_size(vec2(400.0, 300.0))
        .with_step_dt(0.02)
        .build_ui_state(
            |ui, state: &mut State| {
                ui.horizontal(|ui| {
                    state.left = ui.cursor().min;
                    state.spacing = ui.spacing().item_spacing.x;
                    state.height = ui.spacing().interact_size.y;
                    let modulated = state.modulation.as_mut().map(|modulation| Modulated {
                        spec: &SPEC,
                        modulation,
                        color: Color32::LIGHT_BLUE,
                        live: None,
                    });
                    let response = param_field(
                        ui,
                        "field",
                        &mut state.value,
                        RANGE,
                        (TRACK, VALUE),
                        modulated,
                        &mut state.custom,
                    );
                    if response.disconnect {
                        state.response.disconnect = true;
                    }
                });
            },
            state,
        );
    harness.run_steps(2);
    harness
}

fn height(harness: &Harness<'_, State>) -> f32 {
    harness.state().height
}

/// The knob, in the gutter.
fn knob(harness: &Harness<'_, State>) -> Pos2 {
    let s = harness.state();
    s.left + vec2(GUTTER_WIDTH / 2.0, height(harness) / 2.0)
}

/// The point at `t` (`0..=1`) along the slider track.
fn track(harness: &Harness<'_, State>, t: f32) -> Pos2 {
    let s = harness.state();
    // The handle's rail is inset 4 px from each end.
    let left = s.left.x + GUTTER_WIDTH + s.spacing + 4.0;
    pos2(left + (TRACK - 8.0) * t, s.left.y + height(harness) / 2.0)
}

fn value_box(harness: &Harness<'_, State>) -> Pos2 {
    let s = harness.state();
    let left = s.left.x + GUTTER_WIDTH + s.spacing + TRACK + s.spacing;
    pos2(left + VALUE / 2.0, s.left.y + height(harness) / 2.0)
}

fn button(
    harness: &mut Harness<'_, State>,
    pos: Pos2,
    button: PointerButton,
    modifiers: Modifiers,
) {
    harness.event(Event::ModifiersChanged(modifiers));
    harness.event(Event::PointerMoved(pos));
    harness.run_steps(1);
    for pressed in [true, false] {
        harness.event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers,
        });
        harness.run_steps(1);
    }
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
    harness.run_steps(2);
}

fn click(harness: &mut Harness<'_, State>, pos: Pos2) {
    button(harness, pos, PointerButton::Primary, Modifiers::NONE);
}

fn alt_click(harness: &mut Harness<'_, State>, pos: Pos2) {
    button(harness, pos, PointerButton::Primary, Modifiers::ALT);
}

fn right_click(harness: &mut Harness<'_, State>, pos: Pos2) {
    button(harness, pos, PointerButton::Secondary, Modifiers::NONE);
}

fn drag(harness: &mut Harness<'_, State>, from: Pos2, to: Pos2) {
    harness.event(Event::PointerMoved(from));
    harness.run_steps(1);
    harness.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
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
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(2);
}

#[test]
fn clicking_or_dragging_the_track_sets_the_value() {
    let mut harness = harness(None);
    let at = track(&harness, 0.75);
    click(&mut harness, at);
    assert!(
        (harness.state().value - 0.75).abs() < 0.02,
        "{}",
        harness.state().value
    );
    let (from, to) = (track(&harness, 0.75), track(&harness, 0.1));
    drag(&mut harness, from, to);
    assert!(
        (harness.state().value - 0.1).abs() < 0.02,
        "{}",
        harness.state().value
    );
}

#[test]
fn alt_click_or_right_click_resets_the_track() {
    let mut harness = harness(None);
    let at = track(&harness, 0.9);
    alt_click(&mut harness, at);
    assert_eq!(harness.state().value, 0.5);

    harness.state_mut().value = 0.25;
    right_click(&mut harness, at);
    harness.get_by_label_contains("Reset to default").click();
    harness.run_steps(2);
    assert_eq!(harness.state().value, 0.5);
}

#[test]
fn the_value_box_drags_and_takes_typing() {
    let mut harness = harness(None);
    let at = value_box(&harness);
    drag(&mut harness, at, at + vec2(60.0, 0.0));
    assert!(harness.state().value > 0.3, "{}", harness.state().value);

    click(&mut harness, at);
    harness.key_press_modifiers(Modifiers::COMMAND, egui::Key::A);
    harness.event(Event::Text("0.8".into()));
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert_eq!(harness.state().value, 0.8);
}

#[test]
fn alt_click_or_right_click_resets_the_value_box() {
    let mut harness = harness(None);
    let at = value_box(&harness);
    alt_click(&mut harness, at);
    assert_eq!(harness.state().value, 0.5);

    harness.state_mut().value = 0.25;
    harness.run_steps(1);
    right_click(&mut harness, at);
    harness.get_by_label_contains("Reset to default").click();
    harness.run_steps(2);
    assert_eq!(harness.state().value, 0.5);
}

fn modulated() -> Option<Modulation> {
    Some(Modulation {
        amount: 60.0,
        mode: ModMode::Bipolar,
    })
}

fn amount(harness: &Harness<'_, State>) -> f64 {
    harness.state().modulation.unwrap().amount
}

#[test]
fn dragging_the_knob_changes_the_amount() {
    let mut harness = harness(modulated());
    let at = knob(&harness);
    drag(&mut harness, at, at - vec2(0.0, 30.0));
    assert!(amount(&harness) > 60.0, "{}", amount(&harness));
}

#[test]
fn double_click_or_alt_click_resets_the_knob() {
    let default = SPEC.default_modulation_amount();
    let mut harness = harness(modulated());
    let at = knob(&harness);
    click(&mut harness, at);
    click(&mut harness, at);
    assert_eq!(amount(&harness), default);

    harness.state_mut().modulation = modulated();
    // Long enough after the double-click that this click isn't counted with it.
    harness.run_steps(50);
    alt_click(&mut harness, at);
    assert_eq!(amount(&harness), default);
}

#[test]
fn right_clicking_the_knob_offers_its_options() {
    let mut harness = harness(modulated());
    let at = knob(&harness);
    right_click(&mut harness, at);
    harness.get_by_label("One way").click();
    harness.run_steps(2);
    assert_eq!(harness.state().modulation.unwrap().mode, ModMode::Unipolar);
    harness.get_by_label("Disconnect signal").click();
    harness.run_steps(2);
    assert!(harness.state().response.disconnect);
}
