//! A text field for renaming something that other parts of the UI also show.
//!
//! The text being typed is kept apart from the name itself, so the name only changes when the
//! user commits (Enter, or clicking away) and every place showing it stays consistent. Escape
//! cancels. While the field isn't being edited it always shows the current name, so a rename
//! made elsewhere (the graph or the timeline) appears here at once.
//!
//! Names shown as plain text (resource cards, track headers, the inspector's track and node
//! names) are renamed in place: a double-click on the name or F2 calls [`start_rename`], and
//! [`rename_field`] then shows the field over the name, focused with the text selected, until it
//! is committed or cancelled. [`name_label`] is that name as a widget of its own, for every
//! place but the resource card (which draws its name inside its drag source).

use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{
    self, Context, FontId, Id, Key, Rect, Response, Sense, Stroke, TextEdit, Ui, UiBuilder, vec2,
};

use crate::theme::Theme;

/// A name in plain text `font`, as wide as the space left, outlined while hovered, that a
/// double-click (or [`start_rename`] with `id`, from F2 or a menu's Rename) turns into a field
/// in place, outlined in the accent. Its response takes the caller's hover text and menu.
pub fn name_label(ui: &mut Ui, id: Id, name: &str, font: FontId) -> NameEdit {
    let theme = Theme::of(ui.ctx());
    let color = ui.visuals().text_color();
    let width = ui.available_width();
    let mut job = egui::text::LayoutJob::simple_singleline(name.to_owned(), font.clone(), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    let galley = ui.painter().layout_job(job);
    let (rect, response) = ui.allocate_exact_size(vec2(width, galley.size().y), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, name));
    if response.double_clicked() {
        start_rename(ui.ctx(), id);
    }
    let edit = rename_field(ui, id, rect, name, |e| {
        e.frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(font)
    });
    let outline = match &edit {
        Some(_) => Some(theme.accent),
        None if response.hovered() => Some(theme.surfaces.border),
        None => None,
    };
    if edit.is_none() {
        ui.painter().galley(rect.left_top(), galley, color);
    }
    if let Some(color) = outline {
        ui.painter().rect_stroke(
            rect.expand2(vec2(3.0, 1.0)),
            3,
            Stroke::new(1.0, color),
            egui::StrokeKind::Outside,
        );
    }
    NameEdit {
        response,
        committed: edit.and_then(|e| e.committed),
    }
}

/// What a [`name_edit`] did this frame.
#[derive(Debug)]
pub struct NameEdit {
    pub response: Response,
    /// The new name, when the user just committed one that differs from the current name.
    pub committed: Option<String>,
}

/// Shows the field. `style` adjusts the [`TextEdit`] (width, font, hint).
pub fn name_edit(
    ui: &mut Ui,
    id: Id,
    current: &str,
    style: impl FnOnce(TextEdit<'_>) -> TextEdit<'_>,
) -> NameEdit {
    let editing = ui.memory(|m| m.has_focus(id));
    let mut text: String = if editing {
        ui.data(|d| d.get_temp(id))
            .unwrap_or_else(|| current.to_owned())
    } else {
        current.to_owned()
    };
    let response = ui.add(style(TextEdit::singleline(&mut text).id(id)));
    let mut committed = None;
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    }
    if response.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        let cancelled = ui.input(|i| i.key_pressed(Key::Escape));
        let name = text.trim();
        if !cancelled && !name.is_empty() && name != current {
            committed = Some(name.to_owned());
        }
    }
    NameEdit {
        response,
        committed,
    }
}

/// Where whether the name `id` is being renamed is kept: how many more frames its field may
/// ask for the focus before giving up, until it has it, then 0 while it keeps it.
fn renaming_id(id: Id) -> Id {
    id.with("renaming")
}

/// How many frames a field that was just asked for keeps asking for the focus: the click that
/// started the rename (a menu entry) can take it away again in the frame it lands.
const FOCUS_TRIES: u8 = 3;

/// Starts renaming the name whose [`rename_field`] has `id`: from the next frame that field
/// shows, focused with the whole name selected.
pub fn start_rename(ctx: &Context, id: Id) {
    ctx.data_mut(|d| d.insert_temp(renaming_id(id), FOCUS_TRIES));
}

/// Whether the name `id` is being renamed.
pub fn is_renaming(ctx: &Context, id: Id) -> bool {
    ctx.data(|d| d.get_temp::<u8>(renaming_id(id))).is_some()
}

/// While the name `id` is being renamed (after [`start_rename`]), a [`name_edit`] field filling
/// `rect`; `None`, drawing nothing, otherwise, so the caller draws the name. Renaming ends when
/// the field loses the focus: Enter or a click elsewhere commits, Escape cancels.
pub fn rename_field(
    ui: &mut Ui,
    id: Id,
    rect: Rect,
    current: &str,
    style: impl FnOnce(TextEdit<'_>) -> TextEdit<'_>,
) -> Option<NameEdit> {
    let tries = ui.data(|d| d.get_temp::<u8>(renaming_id(id)))?;
    let focused = ui.memory(|m| m.has_focus(id));
    if tries > 0 && !focused {
        let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        let end = CCursor::new(current.chars().count());
        state
            .cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), end)));
        state.store(ui.ctx(), id);
        ui.memory_mut(|m| m.request_focus(id));
    }
    let edit = ui
        .scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            name_edit(ui, id, current, |e| style(e.desired_width(rect.width())))
        })
        .inner;
    let has_focus = edit.response.has_focus();
    let left = match tries {
        _ if has_focus => Some(0),
        0 | 1 => None,
        n => Some(n - 1),
    };
    ui.data_mut(|d| {
        if let Some(n) = left {
            d.insert_temp(renaming_id(id), n);
        } else {
            d.remove::<u8>(renaming_id(id));
        }
    });
    Some(edit)
}
