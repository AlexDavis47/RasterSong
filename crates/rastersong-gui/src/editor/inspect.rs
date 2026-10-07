//! Inspecting a connection: hovering one shows what it carries at the playhead, as a picture, a
//! scope or a spectrum, whatever the kind of signal. Holding the inspect key and scrolling
//! changes the view; holding the listen key plays the connection.

use std::sync::Arc;

use eframe::egui::{self, Id, Ui, Vec2, vec2};
use rastersong_engine::{Tap, TapOutcome, TapRequest};
use rastersong_lang::tr;
use serde::{Deserialize, Serialize};

use crate::theme::Theme;
use crate::widgets::Icon;

/// How long a change of view takes to fade and resize, in seconds.
const TRANSITION: f64 = 0.18;

/// How long the pointer rests on a connection before it is looked at, in seconds. Moving across
/// wires asks for nothing.
const DWELL: f64 = 0.15;
/// The popup's width.
pub const WIDTH: f32 = 240.0;
const SCOPE_HEIGHT: f32 = 72.0;
const SPECTRUM_HEIGHT: f32 = 100.0;
/// The shape of a picture before there is one to measure.
const DEFAULT_ASPECT: f32 = 16.0 / 9.0;
/// Scrolled distance that moves the view by one for a smooth (trackpad) wheel, in points.
const SCROLL_STEP: f32 = 30.0;

/// A way of looking at a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum InspectMode {
    /// The signal stretched over a picture, whatever it is.
    #[default]
    Picture,
    /// The waveform.
    Scope,
    /// The frequency content.
    Spectrum,
    /// Only the numbers and the meter.
    Readings,
}

impl InspectMode {
    pub const ALL: [InspectMode; 4] = [Self::Picture, Self::Scope, Self::Spectrum, Self::Readings];

    pub fn icon(self) -> Icon {
        match self {
            Self::Picture => Icon::Picture,
            Self::Scope => Icon::Scope,
            Self::Spectrum => Icon::Spectrum,
            Self::Readings => Icon::Readings,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Picture => tr("inspect.picture"),
            Self::Scope => tr("inspect.scope"),
            Self::Spectrum => tr("inspect.spectrum"),
            Self::Readings => tr("inspect.readings"),
        }
    }

    /// The mode `steps` along the list (wrapping), for scrolling.
    pub fn stepped(self, steps: i32) -> Self {
        let len = Self::ALL.len() as i32;
        let at = Self::ALL.iter().position(|m| *m == self).unwrap_or(0) as i32;
        Self::ALL[(at + steps).rem_euclid(len) as usize]
    }

    /// The height of this mode's view.
    fn height(self) -> f32 {
        match self {
            Self::Scope => SCOPE_HEIGHT,
            Self::Spectrum => SPECTRUM_HEIGHT,
            _ => WIDTH / DEFAULT_ASPECT,
        }
    }
}

/// The view chosen for each kind of signal: what scrolling changes is the view of the kind of
/// signal under the pointer, so audio can stay a spectrum while video stays a picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewChoice {
    pub audio: InspectMode,
    pub other: InspectMode,
}

impl Default for ViewChoice {
    fn default() -> Self {
        Self {
            audio: InspectMode::Scope,
            other: InspectMode::Picture,
        }
    }
}

impl ViewChoice {
    /// The view for a signal that carries audio or not.
    pub fn of(&self, audio: bool) -> InspectMode {
        if audio { self.audio } else { self.other }
    }

    /// Moves the view of that kind of signal `steps` along the list.
    pub fn step(&mut self, audio: bool, steps: i32) {
        let view = if audio {
            &mut self.audio
        } else {
            &mut self.other
        };
        *view = view.stepped(steps);
    }
}

/// Whether Alt is held, which turns the wheel into a way to change the view. (Ctrl is taken by
/// zooming, and Shift by listening.)
pub fn changing_view(ui: &Ui) -> bool {
    !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.modifiers.alt)
}

/// Whether Shift is held, which plays the connection under the pointer.
pub fn listening(ui: &Ui) -> bool {
    !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.modifiers.shift && !i.modifiers.alt)
}

/// How many steps through the views this frame's wheel events make: one for each notch of a
/// wheel, and one for every [`SCROLL_STEP`] points of a smooth wheel, in the order they came.
/// Down the wheel is down the list. `rest` carries the smooth wheel's remainder from frame to
/// frame, and is cleared whenever a notch counts, so a click of the wheel never moves two.
pub fn scroll_steps(
    events: impl IntoIterator<Item = (egui::MouseWheelUnit, f32)>,
    rest: &mut f32,
) -> i32 {
    let mut steps = 0;
    for (unit, delta) in events {
        match unit {
            egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => {
                if delta != 0.0 {
                    steps -= delta.signum() as i32;
                    *rest = 0.0;
                }
            }
            egui::MouseWheelUnit::Point => {
                *rest -= delta;
                while rest.abs() >= SCROLL_STEP {
                    steps += rest.signum() as i32;
                    *rest -= rest.signum() * SCROLL_STEP;
                }
            }
        }
    }
    steps
}

/// What inspecting needs from the app: where the playhead is, how to ask the engine, and how
/// the user wants to look.
pub struct InspectContext<'a> {
    pub frame: usize,
    pub tap: &'a dyn Fn(&TapRequest) -> TapOutcome,
    /// The soonest a new frame is asked for while the playhead moves, in seconds.
    pub refresh: f64,
    pub views: ViewChoice,
}

impl std::fmt::Debug for InspectContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InspectContext")
            .field("frame", &self.frame)
            .field("views", &self.views)
            .finish_non_exhaustive()
    }
}

/// What the popup shows.
#[derive(Clone)]
pub enum Visual {
    /// Nothing yet: the pointer has not rested, or the engine has not answered.
    Waiting,
    Ready {
        tap: Arc<Tap>,
        /// Changes when the tap does, so the picture is rebuilt only then.
        version: u64,
    },
    /// The connection is not part of what is rendered.
    NotRendered,
    Failed(String),
}

/// What the tool has worked out about the connection under the pointer.
#[derive(Clone)]
struct State {
    target: (String, usize),
    /// When the pointer arrived on the target.
    since: f64,
    /// The frame being shown, and when it was asked for.
    frame: usize,
    asked: f64,
    /// The last thing the engine answered, shown while the next answer is on its way.
    last: Option<Arc<Tap>>,
    version: u64,
}

/// Inspects output `output` of node `node`.
pub fn inspect(ui: &Ui, node: &str, output: usize, ctx: &InspectContext) -> Visual {
    let id = Id::new("inspect-state");
    let now = ui.input(|i| i.time);
    let target = (node.to_owned(), output);
    let mut state = ui
        .data(|d| d.get_temp::<State>(id))
        .filter(|s| s.target == target)
        .unwrap_or(State {
            target,
            since: now,
            frame: ctx.frame,
            asked: now,
            last: None,
            version: 0,
        });

    let visual = if now - state.since < DWELL {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(DWELL));
        Visual::Waiting
    } else {
        if state.frame != ctx.frame && now - state.asked >= ctx.refresh {
            state.frame = ctx.frame;
            state.asked = now;
        }
        let request = TapRequest {
            frame: state.frame,
            node: node.to_owned(),
            output,
        };
        match (ctx.tap)(&request) {
            TapOutcome::Ready(tap) => {
                if state
                    .last
                    .as_ref()
                    .is_none_or(|last| !Arc::ptr_eq(last, &tap))
                {
                    state.version += 1;
                }
                state.last = Some(tap);
                ready(&state)
            }
            TapOutcome::Pending => {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
                ready(&state)
            }
            TapOutcome::NotRendered => Visual::NotRendered,
            TapOutcome::Failed(message) => Visual::Failed(message),
        }
    };
    ui.data_mut(|d| d.insert_temp(id, state));
    visual
}

/// The view of the last answer, or waiting if there is none.
fn ready(state: &State) -> Visual {
    match &state.last {
        Some(tap) => Visual::Ready {
            tap: tap.clone(),
            version: state.version,
        },
        None => Visual::Waiting,
    }
}

impl Visual {
    /// Draws the view for `mode` in a space of fixed size for it.
    pub fn show(&self, ui: &mut Ui, mode: InspectMode) {
        match self {
            Self::Ready { tap, version } => show_tap(ui, tap, *version, mode),
            Self::Waiting => {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(WIDTH, mode.height()), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    tr("inspect.waiting"),
                    egui::FontId::proportional(12.0),
                    ui.visuals().weak_text_color(),
                );
            }
            Self::NotRendered => {
                ui.label(egui::RichText::new(tr("inspect.not_rendered")).weak());
            }
            Self::Failed(message) => {
                ui.colored_label(Theme::of(ui.ctx()).error, message);
            }
        }
    }
}

impl Visual {
    /// The height the view for `mode` takes: a picture is as tall as its shape asks, readings
    /// only take none.
    fn height(&self, mode: InspectMode) -> f32 {
        match (self, mode) {
            (_, InspectMode::Readings) => 0.0,
            (Self::Ready { tap, .. }, InspectMode::Picture) => {
                tap.picture.as_ref().map_or(mode.height(), |p| {
                    WIDTH * p.height as f32 / p.width.max(1) as f32
                })
            }
            (Self::NotRendered | Self::Failed(_), _) => 20.0,
            _ => mode.height(),
        }
    }
}

/// Which view was shown and which is on its way, so a change fades and resizes.
#[derive(Clone, Copy)]
struct Transition {
    from: InspectMode,
    to: InspectMode,
    /// The height shown when the change began, and when it began.
    from_height: f32,
    since: f64,
    /// The height shown last frame.
    shown: f32,
}

/// Draws the view for `mode` and, when it has just changed, fades the old
/// view out and the new one in while the popup grows or shrinks to the new height, so changing
/// views never makes the popup jump.
pub fn show_view(ui: &mut Ui, visual: &Visual, mode: InspectMode) -> f32 {
    let id = Id::new("inspect-transition");
    let now = ui.input(|i| i.time);
    let target = visual.height(mode);
    let mut transition = ui
        .data(|d| d.get_temp::<Transition>(id))
        .unwrap_or(Transition {
            from: mode,
            to: mode,
            from_height: target,
            since: now - TRANSITION,
            shown: target,
        });
    if transition.to != mode {
        transition = Transition {
            from: transition.to,
            to: mode,
            from_height: transition.shown,
            since: now,
            shown: transition.shown,
        };
    }
    let progress = ((now - transition.since) / TRANSITION).clamp(0.0, 1.0) as f32;
    // Smoothstep: eases in and out.
    let eased = progress * progress * (3.0 - 2.0 * progress);
    let height = transition.from_height + (target - transition.from_height) * eased;
    transition.shown = height;

    let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, height), egui::Sense::hover());
    let draw = |ui: &mut Ui, mode: InspectMode, opacity: f32| {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        child.set_opacity(opacity);
        visual.show(&mut child, mode);
    };
    if progress < 1.0 {
        draw(ui, transition.from, 1.0 - eased);
        draw(ui, transition.to, eased);
        ui.ctx().request_repaint();
    } else if height > 0.0 {
        draw(ui, mode, 1.0);
    }
    ui.data_mut(|d| d.insert_temp(id, transition));
    height
}

fn show_tap(ui: &mut Ui, tap: &Tap, version: u64, mode: InspectMode) {
    let channels = tap.layout.samples_per_pixel.max(1) as usize;
    let size: Vec2 = vec2(WIDTH, mode.height());
    let id = Id::new("inspect-view");
    match mode {
        InspectMode::Picture => match &tap.picture {
            Some(picture) => {
                crate::widgets::picture(
                    ui,
                    id,
                    version,
                    [picture.width as usize, picture.height as usize],
                    &picture.rgb,
                    WIDTH,
                );
            }
            None => too_big(ui, size),
        },
        InspectMode::Scope => match &tap.samples {
            Some(samples) => crate::widgets::scope(ui, samples, channels, size),
            None => too_big(ui, size),
        },
        InspectMode::Spectrum => match &tap.samples {
            Some(samples) => crate::widgets::spectrum(ui, id, samples, channels, tap.rate, size),
            None => too_big(ui, size),
        },
        InspectMode::Readings => {}
    }
}

/// A box saying the signal is too large to show this way.
fn too_big(ui: &mut Ui, size: Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        tr("inspect.too_big"),
        egui::FontId::proportional(12.0),
        ui.visuals().weak_text_color(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolling_steps_through_the_modes_in_order_and_wraps() {
        let mut mode = InspectMode::Picture;
        let mut seen = Vec::new();
        for _ in 0..5 {
            mode = mode.stepped(1);
            seen.push(mode);
        }
        assert_eq!(
            seen,
            [
                InspectMode::Scope,
                InspectMode::Spectrum,
                InspectMode::Readings,
                InspectMode::Picture,
                InspectMode::Scope
            ]
        );
        assert_eq!(InspectMode::Picture.stepped(-1), InspectMode::Readings);
    }

    #[test]
    fn changing_view_resizes_and_fades_over_a_moment_instead_of_jumping() {
        let ctx = egui::Context::default();
        let visual = Visual::Waiting;
        let mut heights = Vec::new();
        for (time, mode) in [
            (0.0, InspectMode::Scope),
            (1.0, InspectMode::Scope),
            (1.0, InspectMode::Spectrum),
            (1.09, InspectMode::Spectrum),
            (1.5, InspectMode::Spectrum),
        ] {
            let input = egui::RawInput {
                time: Some(time),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                heights.push(show_view(ui, &visual, mode));
            });
            output.textures_delta.clear();
        }
        assert_eq!(heights[1], SCOPE_HEIGHT);
        assert_eq!(heights[2], SCOPE_HEIGHT, "starts where it was");
        assert!(
            heights[3] > SCOPE_HEIGHT && heights[3] < SPECTRUM_HEIGHT,
            "{heights:?}"
        );
        assert_eq!(heights[4], SPECTRUM_HEIGHT);
    }

    #[test]
    fn a_notch_of_the_wheel_is_one_step_whatever_its_size() {
        use egui::MouseWheelUnit::Line;
        let mut rest = 0.0;
        // Three notches down, one up: down the wheel is further down the list.
        assert_eq!(scroll_steps([(Line, -1.0)], &mut rest), 1);
        assert_eq!(scroll_steps([(Line, -3.0)], &mut rest), 1);
        assert_eq!(scroll_steps([(Line, 1.0)], &mut rest), -1);
        // Separate notches in one frame each count.
        assert_eq!(scroll_steps([(Line, -1.0), (Line, -1.0)], &mut rest), 2);
    }

    #[test]
    fn a_smooth_wheel_steps_every_so_many_points_and_keeps_its_remainder() {
        use egui::MouseWheelUnit::Point;
        let mut rest = 0.0;
        assert_eq!(scroll_steps([(Point, -20.0)], &mut rest), 0);
        assert_eq!(scroll_steps([(Point, -20.0)], &mut rest), 1);
        assert!((rest - 10.0).abs() < 1e-4);
        assert_eq!(scroll_steps([(Point, 40.0)], &mut rest), -1);
    }
}
