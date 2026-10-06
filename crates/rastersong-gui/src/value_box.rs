//! The one numeric field of the UI: drag to change, click to type.
//!
//! egui's `DragValue` turns into a text field that sizes itself to what is typed, so a long
//! number pushed its neighbours (and the inspector) wider. This box has a width its caller
//! chooses, clips what doesn't fit and accepts at most [`MAX_CHARS`] characters, so nothing
//! about the text can change the layout. Everything that edits a number uses it.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, Key, Response, Sense, TextEdit, Ui, Vec2, Widget, WidgetInfo, text::CCursor,
    text::CCursorRange, vec2,
};

/// The most characters a value box accepts while typing.
pub const MAX_CHARS: usize = 12;

/// The widest a box sizes itself to, when its caller gives no width.
const MAX_AUTO_WIDTH: f32 = 220.0;

/// What is being typed, and the value it belongs to.
#[derive(Clone)]
struct Edit {
    text: String,
    value: f64,
}

#[derive(Debug)]
pub struct ValueBox<'a> {
    value: &'a mut f64,
    range: RangeInclusive<f64>,
    speed: f64,
    max_decimals: usize,
    prefix: &'a str,
    suffix: &'a str,
    size: Option<Vec2>,
    whole: bool,
}

impl<'a> ValueBox<'a> {
    pub fn new(value: &'a mut f64) -> Self {
        Self {
            value,
            range: f64::NEG_INFINITY..=f64::INFINITY,
            speed: 1.0,
            max_decimals: 3,
            prefix: "",
            suffix: "",
            size: None,
            whole: false,
        }
    }

    /// Only whole numbers: dragging steps through them and typed numbers are rounded.
    pub fn whole(mut self, whole: bool) -> Self {
        self.whole = whole;
        self
    }

    /// The values typing and dragging are clamped to.
    pub fn range(mut self, range: RangeInclusive<f64>) -> Self {
        self.range = range;
        self
    }

    /// How much one dragged point changes the value.
    pub fn speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    pub fn max_decimals(mut self, max_decimals: usize) -> Self {
        self.max_decimals = max_decimals;
        self
    }

    /// Text shown before the number, such as `"first beat "`.
    pub fn prefix(mut self, prefix: &'a str) -> Self {
        self.prefix = prefix;
        self
    }

    /// Text shown after the number, such as `" s"`.
    pub fn suffix(mut self, suffix: &'a str) -> Self {
        self.suffix = suffix;
        self
    }

    /// The box's exact size. Without it, the width follows the shown text (up to a limit) and is
    /// held while the number is being typed.
    pub fn size(mut self, size: Vec2) -> Self {
        self.size = Some(size);
        self
    }
}

fn clamp(value: f64, range: &RangeInclusive<f64>) -> f64 {
    value.clamp(*range.start(), *range.end())
}

impl Widget for ValueBox<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let Self {
            value,
            range,
            speed,
            max_decimals,
            prefix,
            suffix,
            size,
            whole,
        } = self;
        let id = ui.next_auto_id();
        let old = *value;
        if ui.is_enabled() {
            ui.memory_mut(|m| m.interested_in_focus(id, ui.layer_id()));
        }
        let editing = ui.is_enabled() && ui.memory(|m| m.has_focus(id));
        // Whether this is the first frame of editing, to start from the current value.
        let started = editing && !ui.data(|d| d.get_temp(id.with("editing")).unwrap_or(false));
        ui.data_mut(|d| d.insert_temp(id.with("editing"), editing));
        if started {
            ui.data_mut(|d| d.remove::<Edit>(id));
        }

        // Decimals follow the drag speed, as DragValue's do: a fine drag shows more.
        let aim = f64::from(ui.input(|i| i.aim_radius()));
        let auto = (aim / speed.abs()).log10().ceil().clamp(0.0, 15.0) as usize;
        let decimals = auto.min(max_decimals)..=max_decimals;
        let number = |v: f64| ui.style().number_formatter.format(v, decimals.clone());
        let font = ui.style().drag_value_text_style.clone();
        let height = ui.spacing().interact_size.y;
        let width_id = id.with("width");

        let mut response = if editing {
            let width = size.map_or_else(
                || {
                    ui.data(|d| d.get_temp(width_id))
                        .unwrap_or(ui.spacing().interact_size.x)
                },
                |s| s.x,
            );
            let rect = egui::Rect::from_min_size(
                ui.next_widget_position(),
                vec2(width, size.map_or(height, |s| s.y)),
            );
            let mut edit = ui
                .data(|d| d.get_temp::<Edit>(id))
                .filter(|e| e.value == old)
                .unwrap_or_else(|| Edit {
                    text: number(old),
                    value: old,
                });
            if started {
                // Typing replaces the old number: select it before the field sees any input.
                let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                state.cursor.set_char_range(Some(CCursorRange::two(
                    CCursor::default(),
                    CCursor::new(edit.text.chars().count()),
                )));
                state.store(ui.ctx(), id);
            }
            let padding = ui.spacing().button_padding;
            let response = ui.put(
                rect,
                TextEdit::singleline(&mut edit.text)
                    .id(id)
                    .font(font)
                    .char_limit(MAX_CHARS)
                    .clip_text(true)
                    .margin(padding)
                    .desired_width(width - 2.0 * padding.x),
            );
            if response.changed()
                && let Some(parsed) = parse(&edit.text)
            {
                *value = clamp(if whole { parsed.round() } else { parsed }, &range);
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
                *value = old;
            }
            edit.value = *value;
            ui.data_mut(|d| d.insert_temp(id, edit));
            response
        } else {
            let shown = format!("{prefix}{}{suffix}", number(*value));
            let galley = ui.painter().layout_no_wrap(
                shown.clone(),
                font.resolve(ui.style()),
                ui.visuals().text_color(),
            );
            let padding = ui.spacing().button_padding;
            let natural = galley.size().x + 2.0 * padding.x;
            let size = size.unwrap_or_else(|| {
                vec2(
                    natural.clamp(ui.spacing().interact_size.x, MAX_AUTO_WIDTH),
                    height,
                )
            });
            ui.data_mut(|d| d.insert_temp(width_id, size.x));
            let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
            let response = response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            if ui.is_rect_visible(rect) {
                let visuals = ui.style().interact(&response);
                ui.painter().rect(
                    rect,
                    visuals.corner_radius,
                    visuals.weak_bg_fill,
                    visuals.bg_stroke,
                    egui::StrokeKind::Inside,
                );
                let clipped = ui
                    .painter()
                    .with_clip_rect(rect.shrink2(vec2(padding.x, 0.0)));
                // Text too long for the box shows its start.
                let at = if natural > size.x {
                    egui::pos2(rect.left() + padding.x, rect.center().y)
                } else {
                    rect.center()
                };
                let anchor = if natural > size.x {
                    egui::Align2::LEFT_CENTER
                } else {
                    egui::Align2::CENTER_CENTER
                };
                clipped.text(
                    at,
                    anchor,
                    shown.as_str(),
                    font.resolve(ui.style()),
                    visuals.text_color(),
                );
            }
            if response.clicked() {
                ui.memory_mut(|m| m.request_focus(id));
                ui.data_mut(|d| d.remove::<Edit>(id));
            } else if response.dragged() {
                let delta = response.drag_delta();
                let slow = ui.input(|i| i.modifiers.shift_only());
                let step = f64::from(delta.x - delta.y) * if slow { speed / 10.0 } else { speed };
                if step != 0.0 {
                    // The value is rounded as it's shown, so the exact one is kept between frames.
                    let precise = ui.data(|d| d.get_temp::<f64>(id)).unwrap_or(*value) + step;
                    ui.data_mut(|d| d.insert_temp(id, precise));
                    let rounded = emath_round(precise, auto.min(max_decimals));
                    *value = clamp(rounded, &range);
                }
            } else if !response.dragged() && ui.input(|i| i.pointer.any_released()) {
                ui.data_mut(|d| d.remove::<f64>(id));
            }
            response
        };

        if *value != old {
            response.mark_changed();
        }
        if !editing {
            let shown_value = *value;
            response.widget_info(|| WidgetInfo::drag_value(true, shown_value));
        }
        response
    }
}

fn emath_round(value: f64, decimals: usize) -> f64 {
    egui::emath::round_to_decimals(value, decimals)
}

/// A typed number: spaces are ignored and the typographic minus counts as a minus.
fn parse(text: &str) -> Option<f64> {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| if c == '\u{2212}' { '-' } else { c })
        .collect();
    cleaned.parse().ok().filter(|v: &f64| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_numbers_parse_leniently_and_reject_junk() {
        assert_eq!(parse(" 1 234.5 "), Some(1234.5));
        assert_eq!(parse("\u{2212}3"), Some(-3.0));
        assert_eq!(parse("1e-3"), Some(0.001));
        assert_eq!(parse("abc"), None);
        assert_eq!(parse("inf"), None);
        assert_eq!(parse(""), None);
    }
}
