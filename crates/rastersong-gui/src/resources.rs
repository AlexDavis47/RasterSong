//! The Resources panel, listing the media the project uses, and the dialog that picks which
//! streams of a file to import.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Id, RichText, Sense, Ui};
use rastersong_engine::{Project, ResourceId, ResourceKind, StreamInfo, StreamKind};
use rastersong_lang::{tr, tr_args};

use crate::name_edit::name_edit;

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
}

/// What a resource row hands over while it is dragged: drop it on the timeline to make a track.
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

fn channels_label(channels: u32) -> String {
    match channels {
        1 => tr("resources.channels.mono").to_owned(),
        2 => tr("resources.channels.stereo").to_owned(),
        n => tr_args("resources.channels.n", &[("n", &n.to_string())]),
    }
}

/// The panel: a header with Import, then a row per resource. Rows can be dragged onto the
/// timeline; a row whose file is missing says so.
pub fn resources_panel(
    ui: &mut Ui,
    project: &Project,
    missing: &HashSet<ResourceId>,
) -> Vec<ResourceAction> {
    let mut actions = Vec::new();
    ui.horizontal(|ui| {
        ui.strong(tr("resources.title"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(tr("resources.import"))
                .on_hover_text(tr("resources.import.help"))
                .clicked()
            {
                actions.push(ResourceAction::Import);
            }
        });
    });
    ui.separator();
    if project.resources.is_empty() {
        ui.weak(tr("resources.empty"));
        return actions;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for resource in &project.resources {
                let id = Id::new(("resource", resource.id));
                let missing = missing.contains(&resource.id);
                let users = project.resource_users(resource.id).len();
                let row = ui.dnd_drag_source(id, DraggedResource(resource.id), |ui| {
                    ui.horizontal(|ui| {
                        let (glyph, help) = match resource.kind {
                            ResourceKind::Video => ("🎞", tr("resources.kind.video")),
                            ResourceKind::Audio => ("🔊", tr("resources.kind.audio")),
                        };
                        ui.label(glyph).on_hover_text(help);
                        let mut text = RichText::new(&resource.name);
                        if missing {
                            text = text.color(ui.visuals().error_fg_color);
                        }
                        ui.add(egui::Label::new(text).truncate().sense(Sense::hover()));
                        if missing
                            && ui
                                .small_button(tr("resources.relocate"))
                                .on_hover_text(tr("resources.relocate.help"))
                                .clicked()
                        {
                            actions.push(ResourceAction::Relocate(resource.id));
                        }
                    });
                });
                let file = resource
                    .path
                    .file_name()
                    .map_or_else(String::new, |f| f.to_string_lossy().into_owned());
                let mut help = resource.path.display().to_string();
                if missing {
                    help = tr_args("resources.missing", &[("path", &help)]);
                }
                if users > 0 {
                    help.push('\n');
                    help.push_str(&tr_args(
                        "resources.used_by",
                        &[("count", &users.to_string())],
                    ));
                }
                help.push('\n');
                help.push_str(tr("resources.drag.help"));
                let row = row.response.on_hover_text(help);
                row.context_menu(|ui| {
                    ui.label(RichText::new(file).weak());
                    let rename = name_edit(
                        ui,
                        Id::new(("resource-name", resource.id)),
                        &resource.name,
                        |t| t.desired_width(180.0),
                    );
                    if let Some(name) = rename.committed {
                        actions.push(ResourceAction::Rename(resource.id, name));
                    }
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
                if row.double_clicked() {
                    actions.push(ResourceAction::AddToTimeline(resource.id));
                }
            }
        });
    actions
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
