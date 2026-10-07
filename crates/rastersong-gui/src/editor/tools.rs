//! The graph editor's tools: what hovering a connection does.

use eframe::egui::{Key, Ui};
use rastersong_lang::{tr, tr_args};
use serde::{Deserialize, Serialize};

/// A tool for the graph editor. Select is the editor as it always was; Look and Listen add what
/// hovering a connection shows or plays. A tool's key, held, overrides the default tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Tool {
    #[default]
    Select,
    /// Hovering a connection shows a picture or scope of what it carries.
    Look,
    /// Hovering a connection plays what it carries.
    Listen,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Self::Select, Self::Look, Self::Listen];

    /// The key that holds the tool, if it has one.
    pub fn key(self) -> Option<Key> {
        match self {
            Self::Select => None,
            Self::Look => Some(Key::L),
            Self::Listen => Some(Key::H),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Select => tr("tool.select"),
            Self::Look => tr("tool.look"),
            Self::Listen => tr("tool.listen"),
        }
    }

    /// What the tool does, and its key.
    pub fn help(self) -> String {
        let help = match self {
            Self::Select => tr("tool.select.help"),
            Self::Look => tr("tool.look.help"),
            Self::Listen => tr("tool.listen.help"),
        };
        match self.key() {
            Some(key) => tr_args("tool.hold_key", &[("help", help), ("key", key.name())]),
            None => help.to_owned(),
        }
    }
}

/// The tool in use now: the one whose key is held, or `default`.
pub fn active(ui: &Ui, default: Tool) -> Tool {
    if ui.ctx().egui_wants_keyboard_input() {
        return default;
    }
    ui.input(|i| {
        if i.modifiers.any() {
            return default;
        }
        Tool::ALL
            .into_iter()
            .find(|tool| tool.key().is_some_and(|key| i.key_down(key)))
            .unwrap_or(default)
    })
}

/// The tool buttons, `default` chosen and `active` marked when a held key overrides it. Returns
/// the tool clicked, if any.
pub fn tool_bar(ui: &mut Ui, default: Tool, active: Tool) -> Option<Tool> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        for tool in Tool::ALL {
            let response = ui
                .selectable_label(tool == active, tool.label())
                .on_hover_text(tool.help());
            if response.clicked() && tool != default {
                clicked = Some(tool);
            }
        }
    });
    clicked
}
