//! A text field for renaming something that other parts of the UI also show.
//!
//! The text being typed is kept apart from the name itself, so the name only changes when the
//! user commits (Enter, or clicking away) and every place showing it stays consistent. Escape
//! cancels. While the field isn't being edited it always shows the current name, so a rename
//! made elsewhere (the graph or the timeline) appears here at once.

use eframe::egui::{Id, Key, Response, TextEdit, Ui};

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
