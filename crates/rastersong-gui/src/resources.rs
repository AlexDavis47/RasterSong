//! The Resources panel, listing the media the project uses, and the dialog that picks which
//! streams of a file to import.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Id, RichText, Sense, Ui};
use rastersong_engine::{Project, ResourceId, ResourceKind, StreamInfo, StreamKind};
use rastersong_lang::{tr, tr_args};

use crate::name_edit::name_edit;
use crate::widgets::channels_label;

/// What the user did in the Resources panel.
#[derive(Debug, Clone, PartialEq)]
pub enum ResourceAction {
    /// Pick files to import.
    Import,
    /// Add a track playing the resource, at the playhead.
    AddToTimeline(ResourceId),
    Rename(ResourceId, String),
    /// Remove the resource (and, once confirmed, its tracks).
    Remove(ResourceId),
    /// Pick the file again, for a resource whose file moved.
    Relocate(ResourceId),
    /// Make a new graph (a passthrough).
    NewGraph,
    /// Open the graph with this id in the editor.
    OpenGraph(u32),
    RenameGraph(u32, String),
    DuplicateGraph(u32),
    RemoveGraph(u32),
}

/// What a media card hands over while it is dragged: drop it on the timeline to make a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DraggedResource(pub ResourceId);

/// A file whose streams wait on the import dialog, each with whether it is chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingImport {
    pub path: PathBuf,
    pub streams: Vec<(StreamInfo, bool)>,
}

impl PendingImport {
    /// Every stream starts chosen.
    pub fn new(path: PathBuf, streams: Vec<StreamInfo>) -> Self {
        Self {
            path,
            streams: streams.into_iter().map(|s| (s, true)).collect(),
        }
    }
}

/// The kind of resource a stream becomes.
pub fn resource_kind(stream: &StreamInfo) -> ResourceKind {
    match stream.kind {
        StreamKind::Video { .. } => ResourceKind::Video,
        StreamKind::Audio { .. } => ResourceKind::Audio,
    }
}

/// A name for the resource made from `stream` of the file at `path`: the file's (see
/// [`rastersong_engine::resource_name_for`]), with the stream's title when it has one.
pub fn resource_name(path: &Path, stream: &StreamInfo) -> String {
    let name = rastersong_engine::resource_name_for(path, resource_kind(stream));
    match &stream.title {
        Some(title) => format!("{name} {title}"),
        None => name,
    }
}

/// One line describing a stream: its kind and number in the file, title and language, codec
/// and shape. `n` counts streams of its kind from 1.
pub fn stream_label(stream: &StreamInfo, n: usize) -> String {
    let mut parts = Vec::new();
    let kind = match stream.kind {
        StreamKind::Video { .. } => tr("resources.kind.video"),
        StreamKind::Audio { .. } => tr("resources.kind.audio"),
    };
    let mut head = format!("{kind} {n}");
    if let Some(title) = &stream.title {
        head.push_str(&format!(": {title}"));
    }
    if let Some(language) = &stream.language {
        head.push_str(&format!(" ({language})"));
    }
    parts.push(head);
    parts.push(stream.codec.clone());
    match stream.kind {
        StreamKind::Video {
            width,
            height,
            frame_rate,
        } => {
            parts.push(format!("{width}×{height}"));
            if frame_rate > 0.0 {
                parts.push(format!("{frame_rate:.3} fps").replace(".000 fps", " fps"));
            }
        }
        StreamKind::Audio {
            sample_rate,
            channels,
        } => {
            parts.push(format!("{sample_rate} Hz"));
            parts.push(channels_label(channels));
        }
    }
    parts.join(" · ")
}

/// Which group of resources the panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResourceTab {
    #[default]
    Media,
    Graphs,
}

/// Width of one card in the resource grid.
pub const CARD_WIDTH: f32 = 92.0;
const CARD_HEIGHT: f32 = 54.0;

/// What a [`resource_card`] shows. Every resource, whatever its kind, is drawn from one of these,
/// so media and graphs look and drag the same way.
#[derive(Debug)]
pub struct Card<'a> {
    pub id: Id,
    pub glyph: &'a str,
    pub glyph_help: &'a str,
    pub name: &'a str,
    pub hover: String,
    /// The file is gone: the card is drawn in the error color.
    pub missing: bool,
    /// The graph is the one open in the editor.
    pub highlighted: bool,
    /// Shown as a button under the card when it is set (Relocate for a missing file): label, help.
    pub fix_button: Option<(&'a str, &'a str)>,
}

/// What the user did to a card this frame.
#[derive(Debug, Default)]
pub struct CardResponse {
    pub double_clicked: bool,
    pub renamed: Option<String>,
    /// The card's fix button was clicked.
    pub fixed: bool,
}

/// Like [`Ui::dnd_drag_source`], but the body also senses clicks. egui's version only senses
/// drags, so double-click and the right-click menu never reach it.
fn drag_source<P: std::any::Any + Send + Sync>(
    ui: &mut Ui,
    id: Id,
    payload: P,
    add_contents: impl FnOnce(&mut Ui),
) -> egui::Response {
    if ui.ctx().is_being_dragged(id) {
        egui::DragAndDrop::set_payload(ui.ctx(), payload);
        let layer = egui::LayerId::new(egui::Order::Tooltip, id);
        let response = ui
            .scope_builder(egui::UiBuilder::new().layer_id(layer), add_contents)
            .response;
        if let Some(pointer) = ui.ctx().pointer_interact_pos() {
            let delta = pointer - response.rect.center();
            ui.ctx()
                .transform_layer_shapes(layer, egui::emath::TSTransform::from_translation(delta));
        }
        response
    } else {
        let rect = ui.scope(add_contents).response.rect;
        ui.interact(rect, id, Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::Grab)
    }
}

/// The one card every resource is drawn with. The card body is a drag source carrying `payload`;
/// the fix button sits below it, outside the drag area, so a drag never swallows its click.
/// Right-click opens a menu with a rename field, then whatever `menu` adds.
pub fn resource_card<P: std::any::Any + Send + Sync>(
    ui: &mut Ui,
    card: &Card<'_>,
    payload: P,
    menu: impl FnOnce(&mut Ui),
) -> CardResponse {
    let mut out = CardResponse::default();
    let mut stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    if card.highlighted {
        stroke = egui::Stroke::new(1.5, ui.visuals().selection.stroke.color);
    }
    if card.missing {
        stroke = egui::Stroke::new(1.0, ui.visuals().error_fg_color);
    }
    let frame = egui::Frame::new()
        .stroke(stroke)
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(4)
        .inner_margin(4);
    frame.show(ui, |ui| {
        ui.set_width(CARD_WIDTH - 10.0);
        ui.set_min_height(CARD_HEIGHT - 10.0);
        let body = drag_source(ui, card.id, payload, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(card.glyph).size(18.0))
                    .on_hover_text(card.glyph_help);
                let mut text = RichText::new(card.name);
                if card.missing {
                    text = text.color(ui.visuals().error_fg_color);
                } else if card.highlighted {
                    text = text.strong();
                }
                ui.add(egui::Label::new(text).truncate().sense(Sense::hover()));
            });
        });
        let body = body.on_hover_text(&card.hover);
        // Clicking the rename field must not close the menu, so only clicks outside do; the
        // menu's buttons close it themselves.
        egui::Popup::context_menu(&body)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                let rename = name_edit(ui, card.id.with("name"), card.name, |t| {
                    t.desired_width(180.0)
                });
                out.renamed = rename.committed;
                menu(ui);
            });
        out.double_clicked = body.double_clicked();
        if let Some((label, help)) = card.fix_button {
            out.fixed = ui.small_button(label).on_hover_text(help).clicked();
        }
    });
    out
}

/// The panel: tabs for Media and Graphs, and a grid of cards for the chosen one.
pub fn resources_panel(
    ui: &mut Ui,
    project: &Project,
    missing: &HashSet<ResourceId>,
) -> Vec<ResourceAction> {
    let mut actions = Vec::new();
    let tab_id = Id::new("resources-tab");
    let mut tab: ResourceTab = ui.data(|d| d.get_temp(tab_id)).unwrap_or_default();
    ui.horizontal(|ui| {
        for (value, label) in [
            (ResourceTab::Media, tr("resources.tab.media")),
            (ResourceTab::Graphs, tr("resources.graphs")),
        ] {
            ui.selectable_value(&mut tab, value, label);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (label, help, action) = match tab {
                ResourceTab::Media => (
                    tr("resources.import"),
                    tr("resources.import.help"),
                    ResourceAction::Import,
                ),
                ResourceTab::Graphs => (
                    tr("resources.graph.new"),
                    tr("resources.graph.new.help"),
                    ResourceAction::NewGraph,
                ),
            };
            if ui.small_button(label).on_hover_text(help).clicked() {
                actions.push(action);
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(tab_id, tab));
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match tab {
            ResourceTab::Media => media_cards(ui, project, missing, &mut actions),
            ResourceTab::Graphs => graph_cards(ui, project, &mut actions),
        });
    actions
}

fn media_cards(
    ui: &mut Ui,
    project: &Project,
    missing: &HashSet<ResourceId>,
    actions: &mut Vec<ResourceAction>,
) {
    if project.resources.is_empty() {
        ui.weak(tr("resources.empty"));
    }
    ui.horizontal_wrapped(|ui| {
        for resource in &project.resources {
            let missing = missing.contains(&resource.id);
            let users = project.resource_users(resource.id).len();
            let (glyph, kind) = match resource.kind {
                ResourceKind::Video => ("🎞", tr("resources.kind.video")),
                ResourceKind::Audio => ("🔊", tr("resources.kind.audio")),
            };
            let file = resource
                .path
                .file_name()
                .map_or_else(String::new, |f| f.to_string_lossy().into_owned());
            let mut hover = resource.path.display().to_string();
            if missing {
                hover = tr_args("resources.missing", &[("path", &hover)]);
            }
            if users > 0 {
                hover.push('\n');
                hover.push_str(&tr_args(
                    "resources.used_by",
                    &[("count", &users.to_string())],
                ));
            }
            hover.push('\n');
            hover.push_str(tr("resources.drag.help"));
            let card = Card {
                id: Id::new(("resource", resource.id)),
                glyph,
                glyph_help: kind,
                name: &resource.name,
                hover,
                missing,
                highlighted: false,
                fix_button: missing
                    .then(|| (tr("resources.relocate"), tr("resources.relocate.help"))),
            };
            let out = resource_card(ui, &card, DraggedResource(resource.id), |ui| {
                ui.label(RichText::new(file).weak());
                if ui.button(tr("resources.add_to_timeline")).clicked() {
                    actions.push(ResourceAction::AddToTimeline(resource.id));
                    ui.close();
                }
                if ui.button(tr("resources.relocate")).clicked() {
                    actions.push(ResourceAction::Relocate(resource.id));
                    ui.close();
                }
                if ui.button(tr("resources.remove")).clicked() {
                    actions.push(ResourceAction::Remove(resource.id));
                    ui.close();
                }
            });
            if let Some(name) = out.renamed {
                actions.push(ResourceAction::Rename(resource.id, name));
            }
            if out.fixed {
                actions.push(ResourceAction::Relocate(resource.id));
            }
            if out.double_clicked {
                actions.push(ResourceAction::AddToTimeline(resource.id));
            }
        }
    });
}

/// What a graph card hands over while it is dragged. The timeline takes it: dropped on a graph
/// layer it places the graph there, anywhere else it makes a new layer for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DraggedGraph(pub u32);

/// The graphs, the open one marked. Double-click opens a graph in the editor.
fn graph_cards(ui: &mut Ui, project: &Project, actions: &mut Vec<ResourceAction>) {
    ui.horizontal_wrapped(|ui| {
        for entry in project.graph_entries() {
            let card = Card {
                id: Id::new(("graph", entry.id)),
                glyph: "🕸",
                glyph_help: tr("resources.graph.kind"),
                name: &entry.name,
                hover: if entry.open {
                    tr("resources.graph.open_now")
                } else {
                    tr("resources.graph.help")
                }
                .to_owned(),
                missing: false,
                highlighted: entry.open,
                fix_button: None,
            };
            let out = resource_card(ui, &card, DraggedGraph(entry.id), |ui| {
                if !entry.open && ui.button(tr("resources.graph.open")).clicked() {
                    actions.push(ResourceAction::OpenGraph(entry.id));
                    ui.close();
                }
                if ui.button(tr("resources.graph.duplicate")).clicked() {
                    actions.push(ResourceAction::DuplicateGraph(entry.id));
                    ui.close();
                }
                if !entry.open && ui.button(tr("resources.remove")).clicked() {
                    actions.push(ResourceAction::RemoveGraph(entry.id));
                    ui.close();
                }
            });
            if let Some(name) = out.renamed {
                actions.push(ResourceAction::RenameGraph(entry.id, name));
            }
            if out.double_clicked && !entry.open {
                actions.push(ResourceAction::OpenGraph(entry.id));
            }
        }
    });
}

/// The "found multiple tracks" dialog for `pending`: a checkbox per stream. `Some(true)` when
/// the user imports the chosen streams, `Some(false)` when they cancel.
pub fn import_dialog(ctx: &egui::Context, pending: &mut PendingImport) -> Option<bool> {
    let mut choice = None;
    let file = pending
        .path
        .file_name()
        .map_or_else(String::new, |f| f.to_string_lossy().into_owned());
    egui::Modal::new(Id::new("import-streams")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading(tr("dialog.import.title"));
        ui.add_space(4.0);
        ui.label(tr_args("dialog.import.body", &[("file", &file)]));
        ui.add_space(8.0);
        let (mut videos, mut audios) = (0, 0);
        for (stream, chosen) in &mut pending.streams {
            let n = if stream.kind.is_video() {
                videos += 1;
                videos
            } else {
                audios += 1;
                audios
            };
            ui.checkbox(chosen, stream_label(stream, n));
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let any = pending.streams.iter().any(|(_, chosen)| *chosen);
            if ui
                .add_enabled(any, egui::Button::new(tr("dialog.import.confirm")))
                .clicked()
            {
                choice = Some(true);
            }
            if ui.button(tr("dialog.cancel")).clicked()
                || ui.input(|i| i.key_pressed(egui::Key::Escape))
            {
                choice = Some(false);
            }
        });
    });
    choice
}

/// The "remove resource?" dialog, for a resource tracks still play. `Some(true)` removes it
/// with its tracks.
pub fn remove_dialog(ctx: &egui::Context, name: &str, tracks: &[String]) -> Option<bool> {
    let mut choice = None;
    egui::Modal::new(Id::new("remove-resource")).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.heading(tr_args("dialog.remove_resource.title", &[("name", name)]));
        ui.add_space(4.0);
        ui.label(tr_args(
            "dialog.remove_resource.tracks",
            &[("tracks", &tracks.join(", "))],
        ));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button(tr("dialog.remove")).clicked() {
                choice = Some(true);
            }
            if ui.button(tr("dialog.cancel")).clicked()
                || ui.input(|i| i.key_pressed(egui::Key::Escape))
            {
                choice = Some(false);
            }
        });
    });
    choice
}
