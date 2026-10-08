//! The inspector for a selected graph item: its graph, pre-roll, and what each of the graph's
//! input nodes reads (the layer below, a track, or nothing).

use eframe::egui::{self, Ui};
use rastersong_engine::{Binding, InputKind, Project, input_ports};
use rastersong_lang::{tr, tr_args};

use crate::theme::Theme;

/// A change the user made in the item inspector.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemEdit {
    PreRoll(bool),
    /// Input node `node` now reads `binding` (`None` for nothing).
    Bind {
        node: String,
        binding: Option<Binding>,
    },
}

/// The text a binding is shown as in the combo box.
fn binding_label(binding: Option<&Binding>, project: &Project, kind: InputKind) -> String {
    match binding {
        None => tr("layer.item.binding.none").to_owned(),
        Some(Binding::LayerBelow) => tr("layer.item.binding.below").to_owned(),
        Some(Binding::Track(name)) if track_names(project, kind).contains(name) => name.clone(),
        Some(Binding::Track(name)) => tr_args("layer.item.binding.gone", &[("track", name)]),
    }
}

/// The names of the tracks an input of `kind` can read.
fn track_names(project: &Project, kind: InputKind) -> Vec<String> {
    match kind {
        InputKind::Video => project
            .video_tracks
            .iter()
            .map(|t| t.name.clone())
            .collect(),
        InputKind::Audio => project
            .audio_tracks
            .iter()
            .map(|t| t.name.clone())
            .collect(),
    }
}

/// Shows the inspector of item `item` of layer `layer` and returns what the user changed.
pub fn graph_item_inspector(
    ui: &mut Ui,
    project: &Project,
    layer: usize,
    item: usize,
) -> Vec<ItemEdit> {
    let mut edits = Vec::new();
    let Some(it) = project.layers.get(layer).and_then(|l| l.items.get(item)) else {
        return edits;
    };
    let theme = Theme::of(ui.ctx());
    ui.add_space(4.0);
    ui.heading(tr("layer.item.header"));
    ui.add_space(4.0);
    let name = project
        .graph_entries()
        .into_iter()
        .find(|g| g.id == it.graph)
        .map(|g| g.name);
    ui.horizontal(|ui| {
        ui.label(tr("layer.item.graph"));
        match &name {
            Some(name) => ui.strong(name),
            None => ui.colored_label(theme.warning, tr("layer.item.graph_gone")),
        };
    });
    let mut pre_roll = it.pre_roll;
    if ui
        .checkbox(&mut pre_roll, tr("layer.item.pre_roll"))
        .on_hover_text(tr("layer.item.pre_roll.help"))
        .changed()
    {
        edits.push(ItemEdit::PreRoll(pre_roll));
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new(tr("layer.item.inputs")).strong());
    let ports = project
        .graph_desc(it.graph)
        .map(input_ports)
        .unwrap_or_default();
    if ports.is_empty() {
        ui.weak(tr("layer.item.no_inputs"));
    }
    for port in ports {
        let current = it.bindings.get(&port.node);
        let gone = matches!(current, Some(Binding::Track(t))
            if !track_names(project, port.kind).contains(t));
        ui.horizontal(|ui| {
            ui.label(match port.kind {
                InputKind::Video => "▣",
                InputKind::Audio => "♪",
            });
            ui.label(&port.node);
            let mut choice = current.cloned();
            egui::ComboBox::from_id_salt(("graph-item-binding", layer, item, &port.node))
                .selected_text(binding_label(current, project, port.kind))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut choice,
                        Some(Binding::LayerBelow),
                        tr("layer.item.binding.below"),
                    );
                    for track in track_names(project, port.kind) {
                        ui.selectable_value(
                            &mut choice,
                            Some(Binding::Track(track.clone())),
                            track,
                        );
                    }
                    ui.selectable_value(&mut choice, None, tr("layer.item.binding.none"));
                })
                .response
                .on_hover_text(tr("layer.item.input.help"));
            if choice.as_ref() != current {
                edits.push(ItemEdit::Bind {
                    node: port.node.clone(),
                    binding: choice,
                });
            }
        });
        if gone && let Some(Binding::Track(track)) = current {
            ui.colored_label(
                theme.warning,
                tr_args("layer.item.binding.gone_note", &[("track", track)]),
            );
        }
    }
    edits
}
