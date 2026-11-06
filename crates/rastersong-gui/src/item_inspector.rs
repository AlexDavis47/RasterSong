//! The inspector for a selected item of any kind, in the inspector's one layout: the resource it
//! plays and its track, then how it plays. What a kind can't do isn't shown: pre-roll where the
//! kind [pre-rolls](TrackKind::pre_rolls), the speed where it [stretches](TrackKind::stretches).
//! What a graph reads is set on its input nodes, not here.

use eframe::egui::{RichText, Ui};
use rastersong_engine::{ItemRef, Project, TrackKind};
use rastersong_lang::{tr, tr_args};

use crate::theme::{self, Theme};
use crate::timeline::TrackAction;
use crate::widgets::{
    about, inspector_body, inspector_header, kind_row, section, setting_row, setting_toggle,
};

/// Shows the inspector of item `item` of the track in row `row` and returns what the user
/// changed, as the timeline would.
pub fn item_inspector(ui: &mut Ui, project: &Project, row: usize, item: usize) -> Vec<TrackAction> {
    let mut actions = Vec::new();
    let Some(track) = project.tracks.get(row) else {
        return actions;
    };
    let Some(it) = project.item(&ItemRef::new(track.name.clone(), item)) else {
        return actions;
    };
    let theme = Theme::of(ui.ctx());
    let (kind, color) = match track.kind {
        TrackKind::Graph => (tr("resources.kind.graph"), theme.graph_block),
        TrackKind::Video => (tr("resources.kind.video"), theme.ports.video),
        TrackKind::Audio => (tr("resources.kind.audio"), theme.ports.audio),
    };
    inspector_header(ui, |ui| {
        kind_row(ui, tr("item.header"), Some((kind, color)), |_| {});
        match project.resource(it.resource) {
            Some(resource) => {
                ui.label(RichText::new(&resource.name).font(theme::semibold(15.0)));
            }
            None => {
                ui.colored_label(theme.warning, tr("item.resource_gone"));
            }
        }
        about(ui, &tr_args("item.about", &[("track", &track.name)]));
    });
    inspector_body(ui, |ui| {
        section(ui, tr("item.playing"), |ui| {
            let mut muted = it.muted;
            if setting_toggle(ui, tr("item.mute"), tr("timeline.item.mute"), &mut muted) {
                actions.push(TrackAction::ToggleItemMute { row, item });
            }
            if track.kind.pre_rolls() {
                let mut pre_roll = it.pre_roll;
                if setting_toggle(
                    ui,
                    tr("item.pre_roll"),
                    tr("item.pre_roll.help"),
                    &mut pre_roll,
                ) {
                    actions.push(TrackAction::SetPreRoll {
                        row,
                        item,
                        on: pre_roll,
                    });
                }
            }
            if track.kind.stretches() {
                let speed = format!("{:.0}%", it.rate * 100.0);
                setting_row(ui, tr("item.speed"), tr("item.speed.help"), |ui| {
                    ui.label(RichText::new(speed).monospace().color(theme.text_dim));
                });
            }
        });
    });
    actions
}
