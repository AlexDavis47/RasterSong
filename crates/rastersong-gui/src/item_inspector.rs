//! The inspector for a selected graph item, in the inspector's one layout: its graph and track,
//! pre-roll, and the source of each of the graph's input ports (what is below, the mix, a track's
//! stem, a graph track below, or nothing).

use eframe::egui::{self, RichText, Ui};
use rastersong_engine::{InputKind, Project, Source, TrackKind, input_ports};
use rastersong_lang::{tr, tr_args};

use crate::editor::port_label;
use crate::theme::{self, Theme};
use crate::widgets::{
    CHOICE_WIDTH, about, inspector_body, inspector_header, kind_row, section, setting_row,
    setting_toggle,
};

/// A change the user made in the item inspector.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemEdit {
    PreRoll(bool),
    /// Input port `port` now reads `source`.
    Source {
        port: String,
        source: Source,
    },
}

/// The sources a port of `kind` on graph track `track` (by its place, top first) can pick, each
/// with its label: what is below, the mix, every track of its kind, the main output of every
/// graph track below, nothing.
pub fn source_choices(project: &Project, track: usize, kind: InputKind) -> Vec<(Source, String)> {
    let (mix, port, tracks) = match kind {
        InputKind::Video => (
            tr("item.source.video_mix"),
            rastersong_engine::DEFAULT_VIDEO,
            TrackKind::Video,
        ),
        InputKind::Audio => (
            tr("item.source.audio_mix"),
            rastersong_engine::DEFAULT_AUDIO,
            TrackKind::Audio,
        ),
    };
    let mut choices = vec![
        (Source::Below, tr("item.source.below").to_owned()),
        (Source::Mix, mix.to_owned()),
    ];
    for t in project.tracks_of(tracks) {
        choices.push((
            Source::Track(t.name.clone()),
            tr_args("item.source.track", &[("track", &t.name)]),
        ));
    }
    for below in project.graph_tracks.iter().skip(track + 1) {
        choices.push((
            Source::Graph {
                track: below.name.clone(),
                port: port.to_owned(),
            },
            tr_args("item.source.graph", &[("track", &below.name)]),
        ));
    }
    choices.push((Source::Nothing, tr("item.source.nothing").to_owned()));
    choices
}

/// Shows the inspector of item `item` of graph track `track` and returns what the user changed.
pub fn graph_item_inspector(
    ui: &mut Ui,
    project: &Project,
    track: &str,
    item: usize,
) -> Vec<ItemEdit> {
    let mut edits = Vec::new();
    let Some(row) = project.graph_tracks.iter().position(|t| t.name == track) else {
        return edits;
    };
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
        section(ui, tr("item.inputs"), |ui| {
            let ports = project
                .graph_desc(it.graph)
                .map(input_ports)
                .unwrap_or_default();
            if ports.is_empty() {
                about(ui, tr("item.no_inputs"));
            }
            for port in ports {
                let video = port.kind == InputKind::Video;
                let current = it.source(&port.name, video);
                let choices = source_choices(project, row, port.kind);
                let label = choices
                    .iter()
                    .find(|(s, _)| *s == current)
                    .map(|(_, l)| l.clone());
                let mut choice = current.clone();
                setting_row(
                    ui,
                    &port_label(&port.name, port.kind),
                    tr("item.source.help"),
                    |ui| {
                        egui::ComboBox::from_id_salt((
                            "graph-item-source",
                            track,
                            item,
                            &port.name,
                            video,
                        ))
                        .width(CHOICE_WIDTH)
                        .selected_text(
                            label
                                .clone()
                                .unwrap_or_else(|| tr("item.source.gone").to_owned()),
                        )
                        .show_ui(ui, |ui| {
                            for (source, label) in &choices {
                                ui.selectable_value(&mut choice, source.clone(), label);
                            }
                        });
                    },
                );
                if choice != current {
                    edits.push(ItemEdit::Source {
                        port: port.name.clone(),
                        source: choice,
                    });
                }
                if label.is_none() {
                    ui.colored_label(theme.warning, tr("item.source.gone_note"));
                }
            }
        });
    });
    edits
}
