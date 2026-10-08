//! The FX window: one FX chain (a track's, a folder's, an item's or the master's), each FX with
//! its on switch, its place in the chain, what fills its ports and whether it passes the picture
//! or sound through untouched.

use eframe::egui::{self, Ui};
use rastersong_engine::{
    AUDIO_OUTPUT, FxTarget, InputKind, OUTPUT, Project, audio_output_bus, input_ports,
};
use rastersong_lang::{tr, tr_args};

use crate::theme::Theme;

/// A change the user made in the FX window.
#[derive(Debug, Clone, PartialEq)]
pub enum FxEdit {
    /// Add graph `graph` at the end of the chain.
    Add(u32),
    Remove(usize),
    /// Move FX `from` to place `to`.
    Move {
        from: usize,
        to: usize,
    },
    Bypass(usize, bool),
    /// Port `port` of FX `fx` now receives from `track` (`None` for nothing).
    Receive {
        fx: usize,
        port: String,
        track: Option<String>,
    },
    /// Open the graph in the editor.
    Open(u32),
    /// Whether the item's FX warm up before it.
    PreRoll(bool),
}

/// What the window is titled for the chain on `target`.
pub fn fx_title(target: &FxTarget) -> String {
    match target {
        FxTarget::Master => tr("fx.title.master").to_owned(),
        FxTarget::Track(track) => tr_args("fx.title.track", &[("track", track)]),
        FxTarget::Item(item) => tr_args(
            "fx.title.item",
            &[
                ("track", &item.track),
                ("item", &(item.item + 1).to_string()),
            ],
        ),
    }
}

/// The tracks a port of an FX on `target` can receive from: every track but the one the FX is
/// on, which would feed itself.
fn senders(project: &Project, target: &FxTarget) -> Vec<String> {
    let own = match target {
        FxTarget::Master => None,
        FxTarget::Track(track) => Some(track),
        FxTarget::Item(item) => Some(&item.track),
    };
    project
        .tracks()
        .filter(|t| Some(&t.name) != own)
        .map(|t| t.name.clone())
        .collect()
}

/// Shows the FX chain on `target` and returns what the user changed.
pub fn fx_chain(ui: &mut Ui, project: &Project, target: &FxTarget) -> Vec<FxEdit> {
    let mut edits = Vec::new();
    let Some(chain) = project.fx(target) else {
        return edits;
    };
    let theme = Theme::of(ui.ctx());
    if let FxTarget::Item(item) = target
        && let Some(it) = project
            .track(&item.track)
            .and_then(|t| t.items.get(item.item))
    {
        let mut pre_roll = it.pre_roll;
        if ui
            .checkbox(&mut pre_roll, tr("timeline.item.pre_roll"))
            .on_hover_text(tr("timeline.item.pre_roll.help"))
            .changed()
        {
            edits.push(FxEdit::PreRoll(pre_roll));
        }
        ui.add_space(4.0);
    }
    if chain.is_empty() {
        ui.weak(tr("fx.empty"));
    }
    let graphs = project.graph_entries();
    let senders = senders(project, target);
    let bus = project.master_bus().to_owned();
    for (i, fx) in chain.iter().enumerate() {
        let desc = project.graph_desc(fx.graph);
        let name = graphs.iter().find(|g| g.id == fx.graph).map(|g| &g.name);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let mut on = !fx.bypass;
                if ui
                    .checkbox(&mut on, "")
                    .on_hover_text(tr("fx.on.help"))
                    .changed()
                {
                    edits.push(FxEdit::Bypass(i, !on));
                }
                match name {
                    Some(name) => {
                        let label = ui
                            .add(egui::Button::new(egui::RichText::new(name).strong()).frame(false))
                            .on_hover_text(tr("fx.name.help"));
                        if label.double_clicked() {
                            edits.push(FxEdit::Open(fx.graph));
                        }
                    }
                    None => {
                        ui.colored_label(theme.warning, tr("fx.graph_gone"));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new("×").frame(false))
                        .on_hover_text(tr("fx.remove"))
                        .clicked()
                    {
                        edits.push(FxEdit::Remove(i));
                    }
                    if ui
                        .add_enabled(i + 1 < chain.len(), egui::Button::new("⏷").frame(false))
                        .on_hover_text(tr("fx.down"))
                        .clicked()
                    {
                        edits.push(FxEdit::Move { from: i, to: i + 1 });
                    }
                    if ui
                        .add_enabled(i > 0, egui::Button::new("⏶").frame(false))
                        .on_hover_text(tr("fx.up"))
                        .clicked()
                    {
                        edits.push(FxEdit::Move { from: i, to: i - 1 });
                    }
                });
            });
            let Some(desc) = desc else { return };
            let video_through = !desc.nodes.iter().any(|n| n.kind == OUTPUT);
            let audio_through = !desc
                .nodes
                .iter()
                .any(|n| n.kind == AUDIO_OUTPUT && audio_output_bus(n) == bus);
            if video_through || audio_through {
                ui.horizontal(|ui| {
                    if video_through {
                        ui.weak(tr("fx.through.video"))
                            .on_hover_text(tr("fx.through.video.help"));
                    }
                    if audio_through {
                        ui.weak(tr("fx.through.audio"))
                            .on_hover_text(tr("fx.through.audio.help"));
                    }
                });
            }
            let mut ports: Vec<(String, Vec<InputKind>)> = Vec::new();
            for port in input_ports(desc).into_iter().filter(|p| !p.is_main()) {
                match ports.iter_mut().find(|(n, _)| *n == port.name) {
                    Some((_, kinds)) => kinds.push(port.kind),
                    None => ports.push((port.name, vec![port.kind])),
                }
            }
            for (port, kinds) in ports {
                let current = fx.receives.get(&port);
                let gone = current.is_some_and(|t| !project.has_track(t));
                ui.horizontal(|ui| {
                    let glyph: String = kinds
                        .iter()
                        .map(|k| match k {
                            InputKind::Video => "▣",
                            InputKind::Audio => "♪",
                        })
                        .collect();
                    ui.label(glyph);
                    ui.label(&port);
                    let mut choice = current.cloned();
                    let shown = match current {
                        None => tr("fx.receive.none").to_owned(),
                        Some(t) if gone => tr_args("fx.receive.gone", &[("track", t)]),
                        Some(t) => t.clone(),
                    };
                    egui::ComboBox::from_id_salt(("fx-receive", i, &port))
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for track in &senders {
                                ui.selectable_value(&mut choice, Some(track.clone()), track);
                            }
                            ui.selectable_value(&mut choice, None, tr("fx.receive.none"));
                        })
                        .response
                        .on_hover_text(tr("fx.receive.help"));
                    if choice.as_ref() != current {
                        edits.push(FxEdit::Receive {
                            fx: i,
                            port: port.clone(),
                            track: choice,
                        });
                    }
                });
                if gone && let Some(track) = current {
                    ui.colored_label(
                        theme.warning,
                        tr_args("fx.receive.gone_note", &[("track", track)]),
                    );
                }
            }
        });
    }
    ui.add_space(4.0);
    ui.menu_button(tr("fx.add"), |ui| {
        for graph in &graphs {
            if ui.button(&graph.name).clicked() {
                edits.push(FxEdit::Add(graph.id));
                ui.close();
            }
        }
    })
    .response
    .on_hover_text(tr("fx.add.help"));
    edits
}
