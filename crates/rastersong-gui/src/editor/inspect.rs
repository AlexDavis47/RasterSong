//! Inspecting a connection: hovering one shows what it carries at the playhead, as a picture, a
//! scope or a spectrum, whatever the kind of signal. Holding the inspect key and scrolling
//! changes the view; holding the listen key plays the connection.

use std::sync::Arc;

use eframe::egui::{self, Id, Key, Ui, Vec2, vec2};
use rastersong_engine::{Tap, TapOutcome, TapRequest};
use rastersong_lang::tr;
use serde::{Deserialize, Serialize};

use crate::theme::Theme;
use crate::widgets::Icon;

/// Hold to scroll through the views.
pub const INSPECT_KEY: Key = Key::I;
/// Hold to hear the connection.
pub const LISTEN_KEY: Key = Key::H;

/// How long the pointer rests on a connection before it is looked at, in seconds. Moving across
/// wires asks for nothing.
const DWELL: f64 = 0.15;
/// The popup's width.
pub const WIDTH: f32 = 240.0;
const SCOPE_HEIGHT: f32 = 72.0;
const SPECTRUM_HEIGHT: f32 = 100.0;
/// The shape of a picture before there is one to measure.
const DEFAULT_ASPECT: f32 = 16.0 / 9.0;
/// Scrolled distance that moves the view by one, in points.
pub const SCROLL_STEP: f32 = 30.0;

/// A way of looking at a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum InspectMode {
    /// A picture for video-like signals, a scope for audio-like ones.
    #[default]
    Auto,
    /// The signal stretched over a picture, whatever it is.
    Picture,
    /// The waveform.
    Scope,
    /// The frequency content.
    Spectrum,
    /// Only the numbers and the meter.
    Readings,
}

impl InspectMode {
    pub const ALL: [InspectMode; 5] = [
        Self::Auto,
        Self::Picture,
        Self::Scope,
        Self::Spectrum,
        Self::Readings,
    ];

    pub fn icon(self) -> Icon {
        match self {
            Self::Auto => Icon::Auto,
            Self::Picture => Icon::Picture,
            Self::Scope => Icon::Scope,
            Self::Spectrum => Icon::Spectrum,
            Self::Readings => Icon::Readings,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => tr("inspect.auto"),
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

    /// What `Auto` means for a signal that carries audio or not.
    pub fn resolved(self, audio: bool) -> Self {
        match (self, audio) {
            (Self::Auto, true) => Self::Scope,
            (Self::Auto, false) => Self::Picture,
            (mode, _) => mode,
        }
    }

    /// The height of this mode's view, once resolved.
    fn height(self) -> f32 {
        match self {
            Self::Scope => SCOPE_HEIGHT,
            Self::Spectrum => SPECTRUM_HEIGHT,
            _ => WIDTH / DEFAULT_ASPECT,
        }
    }
}

/// Whether the key is down and the interface isn't taking keystrokes.
pub fn key_held(ui: &Ui, key: Key) -> bool {
    !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| !i.modifiers.any() && i.key_down(key))
}

/// What inspecting needs from the app: where the playhead is, how to ask the engine, and how
/// the user wants to look.
pub struct InspectContext<'a> {
    pub frame: usize,
    pub tap: &'a dyn Fn(&TapRequest) -> TapOutcome,
    /// The soonest a new frame is asked for while the playhead moves, in seconds.
    pub refresh: f64,
    pub mode: InspectMode,
}

impl std::fmt::Debug for InspectContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InspectContext")
            .field("frame", &self.frame)
            .field("mode", &self.mode)
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
    /// Draws the view for `mode` (already resolved) in a space of fixed size for it.
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

fn show_tap(ui: &mut Ui, tap: &Tap, version: u64, mode: InspectMode) {
    let channels = tap.layout.samples_per_pixel.max(1) as usize;
    let size: Vec2 = vec2(WIDTH, mode.height());
    let id = Id::new("inspect-view");
    match mode {
        InspectMode::Picture | InspectMode::Auto => match &tap.picture {
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
    fn scrolling_steps_through_the_modes_and_wraps() {
        assert_eq!(InspectMode::Auto.stepped(1), InspectMode::Picture);
        assert_eq!(InspectMode::Readings.stepped(1), InspectMode::Auto);
        assert_eq!(InspectMode::Auto.stepped(-1), InspectMode::Readings);
        assert_eq!(InspectMode::Scope.stepped(5), InspectMode::Scope);
    }

    #[test]
    fn auto_picks_by_what_the_signal_carries() {
        assert_eq!(InspectMode::Auto.resolved(true), InspectMode::Scope);
        assert_eq!(InspectMode::Auto.resolved(false), InspectMode::Picture);
        assert_eq!(InspectMode::Spectrum.resolved(false), InspectMode::Spectrum);
    }
}
