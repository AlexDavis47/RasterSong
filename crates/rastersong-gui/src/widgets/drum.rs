//! A drum: a vertical wheel of choices, the chosen one in the middle and highlighted, its
//! neighbours tilting away above and below. It wraps around, so past the last choice comes the
//! first again. It turns with a click on a choice, the wheel over it, and ↑/↓ while focused,
//! settling with a short ease.

use eframe::egui::{
    self, Align2, Color32, EventFilter, Id, Key, Mesh, Rect, Response, Sense, Stroke, Ui,
    WidgetInfo, WidgetType, pos2, vec2,
};

use super::{Icon, paint_icon};
use crate::theme::{self, CONTROL_RADIUS, Theme};

/// The drum's size, and the height of one choice on it.
pub const DRUM_WIDTH: f32 = 96.0;
pub const DRUM_HEIGHT: f32 = 150.0;
const SLOT: f32 = 30.0;
/// How long a turn takes to settle, in seconds.
const SETTLE: f64 = 0.26;
/// How far a choice one slot away tilts, in radians, and how much it shrinks and fades.
const TILT: f32 = 24.0 * std::f32::consts::PI / 180.0;
const SHRINK: f32 = 0.07;
const FADE: f32 = 0.32;
/// The part of the drum at each end that fades into the background.
const FADE_EDGE: f32 = 0.34;
const TEXT_SIZE: f32 = 11.5;
const ICON_SIZE: f32 = 18.0;
/// Scrolled distance that turns a smooth (trackpad) wheel by one, in points.
const SCROLL_STEP: f32 = 30.0;

/// How many steps this frame's wheel events make: one for each notch of a wheel, and one for
/// every [`SCROLL_STEP`] points of a smooth wheel, in the order they came. Down the wheel is
/// down the list. `rest` carries the smooth wheel's remainder from frame to frame, and is
/// cleared whenever a notch counts, so a click of the wheel never moves two.
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

/// This frame's vertical wheel events, for [`scroll_steps`].
pub fn wheel_events(ui: &Ui) -> Vec<(egui::MouseWheelUnit, f32)> {
    ui.input(|i| {
        i.events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel { unit, delta, .. } => Some((*unit, delta.y)),
                _ => None,
            })
            .collect()
    })
}

/// Where the drum is and where it is turning to. Positions count choices without wrapping, so
/// turning past the last choice keeps going the same way; the choice at a position is the
/// position modulo the number of choices.
#[derive(Clone, Copy, Debug)]
struct Turn {
    /// The position it is turning to.
    to: i64,
    /// Where it was when this turn began, and when that was.
    from: f32,
    since: f64,
    /// The smooth wheel's remainder.
    rest: f32,
}

impl Turn {
    /// Where the drum is at `now`.
    fn at(&self, now: f64) -> f32 {
        let t = ((now - self.since) / SETTLE).clamp(0.0, 1.0) as f32;
        // Ease out: quick to start, slow to settle.
        let eased = 1.0 - (1.0 - t).powi(3);
        self.from + (self.to as f32 - self.from) * eased
    }

    /// Turns to `to` from wherever the drum is now.
    fn turn_to(&mut self, to: i64, now: f64) {
        if to != self.to {
            self.from = self.at(now);
            self.to = to;
            self.since = now;
        }
    }
}

/// The steps from choice `from` to choice `to` the short way round, of `len`.
fn shortest(from: usize, to: usize, len: usize) -> i64 {
    let len = len as i64;
    let d = (to as i64 - from as i64).rem_euclid(len);
    if d > len / 2 { d - len } else { d }
}

/// What a drum did this frame.
#[derive(Debug)]
pub struct DrumResponse {
    pub response: Response,
    /// The steps the user turned it (down the list is positive); the caller moves its choice by
    /// as many, wrapping, and the drum follows on the next frame.
    pub turned: i32,
}

/// Draws a drum of `items`, `selected` in the middle. A change of `selected` from elsewhere
/// turns it the short way round. The wheel over it turns it unless Alt is held, which leaves the
/// wheel to whoever turns it with Alt.
pub fn drum(ui: &mut Ui, id: Id, items: &[(Icon, &str)], selected: usize) -> DrumResponse {
    let len = items.len().max(1);
    let (rect, _) = ui.allocate_exact_size(vec2(DRUM_WIDTH, DRUM_HEIGHT), Sense::hover());
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::ComboBox,
            ui.is_enabled(),
            true,
            items.get(selected).map_or("", |item| item.1),
        )
    });
    let now = ui.input(|i| i.time);
    let mut turn = ui.data(|d| d.get_temp::<Turn>(id)).unwrap_or(Turn {
        to: selected as i64,
        from: selected as f32,
        since: now - SETTLE,
        rest: 0.0,
    });
    let showing = turn.to.rem_euclid(len as i64) as usize;
    turn.turn_to(turn.to + shortest(showing, selected, len), now);

    let mut turned = 0;
    if response.clicked() {
        response.request_focus();
        if let Some(p) = response.interact_pointer_pos() {
            let at = turn.at(now);
            let picked = (at + (p.y - rect.center().y) / SLOT).round() as i64;
            turned += (picked - turn.to) as i32;
        }
    }
    if response.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                EventFilter {
                    vertical_arrows: true,
                    ..Default::default()
                },
            );
        });
        ui.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                turned -= 1;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                turned += 1;
            }
        });
    }
    if response.hovered() && !ui.input(|i| i.modifiers.alt) {
        turned += scroll_steps(wheel_events(ui), &mut turn.rest);
    }
    turn.turn_to(turn.to + turned as i64, now);
    let at = turn.at(now);
    if at != turn.to as f32 {
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| d.insert_temp(id, turn));

    if ui.is_rect_visible(rect) {
        paint(ui, rect, items, at, response.has_focus());
    }
    DrumResponse { response, turned }
}

/// Draws the drum at position `at`.
fn paint(ui: &Ui, rect: Rect, items: &[(Icon, &str)], at: f32, focused: bool) {
    let theme = Theme::of(ui.ctx());
    let painter = ui.painter_at(rect);
    let center = rect.center().y;
    let middle = Rect::from_center_size(rect.center(), vec2(rect.width() - 12.0, SLOT));
    painter.rect(
        middle,
        CONTROL_RADIUS,
        theme.surfaces.raised_active,
        Stroke::new(1.0, theme.accent),
        egui::StrokeKind::Inside,
    );
    let len = items.len() as i64;
    let reach = (DRUM_HEIGHT / SLOT / 2.0).ceil() as i64 + 1;
    let first = at.floor() as i64 - reach;
    for k in first..=at.ceil() as i64 + reach {
        let (icon, label) = items[k.rem_euclid(len) as usize];
        let d = k as f32 - at;
        let away = d.abs().min(3.0);
        let opacity = (1.0 - away * FADE).max(0.0);
        if opacity <= 0.0 {
            continue;
        }
        // Tilting away squashes a choice; it also shrinks a little the further it is.
        let scale = 1.0 - away * SHRINK;
        let squash = (d * TILT).cos().max(0.0) * scale;
        let y = center + d * SLOT;
        let chosen = d.abs() < 0.5;
        let color = if chosen {
            theme.surfaces.text
        } else {
            theme.text_dim
        }
        .gamma_multiply(opacity);
        let icon_size = ICON_SIZE * scale;
        let icon_rect = Rect::from_center_size(
            pos2(rect.left() + 10.0 + icon_size / 2.0, y),
            vec2(icon_size, icon_size * squash),
        );
        paint_icon(&painter, icon_rect, icon, color);
        let font = if chosen {
            theme::semibold(TEXT_SIZE * scale)
        } else {
            egui::FontId::proportional(TEXT_SIZE * scale)
        };
        painter.text(
            pos2(icon_rect.right() + 3.0, y),
            Align2::LEFT_CENTER,
            label,
            font,
            color,
        );
    }
    fade_ends(&painter, rect, ui.visuals().window_fill);
    if focused {
        painter.rect_stroke(
            rect.shrink(1.0),
            CONTROL_RADIUS,
            Stroke::new(2.0, theme.accent),
            egui::StrokeKind::Inside,
        );
    }
}

/// Fades the top and bottom of `rect` into `background`, so the drum looks round.
fn fade_ends(painter: &egui::Painter, rect: Rect, background: Color32) {
    let edge = rect.height() * FADE_EDGE;
    let clear = background.gamma_multiply(0.0);
    let mut mesh = Mesh::default();
    let mut band = |top: f32, bottom: f32, from: Color32, to: Color32| {
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(rect.left(), top), from);
        mesh.colored_vertex(pos2(rect.right(), top), from);
        mesh.colored_vertex(pos2(rect.right(), bottom), to);
        mesh.colored_vertex(pos2(rect.left(), bottom), to);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    };
    band(rect.top(), rect.top() + edge, background, clear);
    band(rect.bottom() - edge, rect.bottom(), clear, background);
    painter.add(mesh);
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn it_turns_the_short_way_round() {
        assert_eq!(shortest(3, 0, 4), 1, "past the last comes the first");
        assert_eq!(shortest(0, 3, 4), -1);
        assert_eq!(shortest(1, 2, 4), 1);
        assert_eq!(shortest(2, 2, 4), 0);
    }

    #[test]
    fn a_turn_settles_with_an_ease_and_ends_where_it_was_going() {
        let mut turn = Turn {
            to: 0,
            from: 0.0,
            since: 0.0,
            rest: 0.0,
        };
        turn.turn_to(1, 1.0);
        let early = turn.at(1.0 + SETTLE * 0.25);
        let late = turn.at(1.0 + SETTLE * 0.75);
        assert!(
            early > 0.25,
            "eases out: more than a quarter of the way at a quarter of the time"
        );
        assert!(late < 1.0 && late > early);
        assert_eq!(turn.at(1.0 + SETTLE), 1.0);
        // Turning again partway starts from where it is, not from where it was.
        turn.turn_to(2, 1.0 + SETTLE * 0.5);
        assert!((turn.from - turn.at(1.0 + SETTLE * 0.5)).abs() < 1e-6);
        assert!(turn.from > 0.0 && turn.from < 1.0);
    }
}
