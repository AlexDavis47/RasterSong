//! The inspector for a selected graph item: its graph, pre-roll, and the source of each of the
//! graph's input ports (what is below, the mix, a track's stem, a graph track below, or nothing).

use eframe::egui::{self, Ui};
use rastersong_engine::{InputKind, Project, Source, TrackKind, input_ports};
use rastersong_lang::{tr, tr_args};

use crate::editor::port_label;
use crate::theme::Theme;

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
    ui.add_space(4.0);
    ui.heading(tr("item.header"));
    ui.add_space(4.0);
    let name = project
        .graph_entries()
        .into_iter()
        .find(|g| g.id == it.graph)
        .map(|g| g.name);
    ui.horizontal(|ui| {
        ui.label(tr("item.graph"));
        match &name {
            Some(name) => ui.strong(name),
            None => ui.colored_label(theme.warning, tr("item.graph_gone")),
        };
    });
    let mut pre_roll = it.pre_roll;
    if ui
        .checkbox(&mut pre_roll, tr("item.pre_roll"))
        .on_hover_text(tr("item.pre_roll.help"))
        .changed()
    {
        edits.push(ItemEdit::PreRoll(pre_roll));
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new(tr("item.inputs")).strong());
    let ports = project
        .graph_desc(it.graph)
        .map(input_ports)
        .unwrap_or_default();
    if ports.is_empty() {
        ui.weak(tr("item.no_inputs"));
    }
    for port in ports {
        let video = port.kind == InputKind::Video;
        let current = it.source(&port.name, video);
        let choices = source_choices(project, row, port.kind);
        let label = choices
            .iter()
            .find(|(s, _)| *s == current)
            .map(|(_, l)| l.clone());
        ui.horizontal(|ui| {
            ui.label(port_label(&port.name, port.kind));
            let mut choice = current.clone();
            egui::ComboBox::from_id_salt(("graph-item-source", track, item, &port.name, video))
                .selected_text(
                    label
                        .clone()
                        .unwrap_or_else(|| tr("item.source.gone").to_owned()),
                )
                .show_ui(ui, |ui| {
                    for (source, label) in &choices {
                        ui.selectable_value(&mut choice, source.clone(), label);
                    }
                })
                .response
                .on_hover_text(tr("item.source.help"));
            if choice != current {
                edits.push(ItemEdit::Source {
                    port: port.name.clone(),
                    source: choice,
                });
            }
        });
        if label.is_none() {
            ui.colored_label(theme.warning, tr("item.source.gone_note"));
        }
    }
    edits
}
