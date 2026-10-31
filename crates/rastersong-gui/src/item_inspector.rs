//! The inspector for a selected graph item, in the inspector's one layout: its graph and track,
//! and pre-roll. What the graph reads is set on its input nodes, not here.

use eframe::egui::{RichText, Ui};
use rastersong_engine::Project;
use rastersong_lang::{tr, tr_args};

use crate::theme::{self, Theme};
use crate::widgets::{about, inspector_body, inspector_header, kind_row, section, setting_toggle};

/// A change the user made in the item inspector.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemEdit {
    PreRoll(bool),
}

/// Shows the inspector of item `item` of graph track `track` and returns what the user changed.
pub fn graph_item_inspector(
    ui: &mut Ui,
    project: &Project,
    track: &str,
    item: usize,
) -> Vec<ItemEdit> {
    let mut edits = Vec::new();
    let Some(it) = project.graph_item(track, item) else {
        return edits;
    };
    let theme = Theme::of(ui.ctx());
    let name = project
        .graph_entries()
        .into_iter()
        .find(|g| g.id == it.graph)
        .map(|g| g.name);
    inspector_header(ui, |ui| {
        kind_row(
            ui,
            tr("item.header"),
            Some((tr("item.graph"), theme.graph_block)),
            |_| {},
        );
        match &name {
            Some(name) => {
                ui.label(RichText::new(name).font(theme::semibold(15.0)));
            }
            None => {
                ui.colored_label(theme.warning, tr("item.graph_gone"));
            }
        }
        about(ui, &tr_args("item.about", &[("track", track)]));
    });
    inspector_body(ui, |ui| {
        section(ui, tr("item.playing"), |ui| {
            let mut pre_roll = it.pre_roll;
            if setting_toggle(
                ui,
                tr("item.pre_roll"),
                tr("item.pre_roll.help"),
                &mut pre_roll,
            ) {
                edits.push(ItemEdit::PreRoll(pre_roll));
            }
        });
    });
    edits
}
