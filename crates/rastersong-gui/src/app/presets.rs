//! Graph presets in the app: the **New graph** picker (an empty graph, the factory presets and the
//! user's), and the canvas header's **Presets** menu, which loads a preset into the open graph or
//! saves the open graph as one. Presets live in [`rastersong_engine::library`]; the user's are files
//! in the folder Settings names.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Ui, vec2};
use rastersong_engine::{
    GraphDesc, Preset, ResourceId, factory_presets, save_preset, user_presets,
};
use rastersong_lang::{tr, tr_args};

use super::{App, HEADER_BUTTON_HEIGHT};
use crate::widgets::{choice_card, section_label};

/// The presets on offer, read when a picker or menu opens, so files added to the folder meanwhile
/// show up.
#[derive(Debug, Clone)]
pub(super) struct Presets {
    factory: Vec<Preset>,
    user: Vec<Preset>,
    /// Graph files in the folder that couldn't be read.
    unreadable: usize,
}

impl Presets {
    pub(super) fn read(dir: &Path) -> Self {
        let (user, failed) = user_presets(dir);
        Self {
            factory: factory_presets(),
            user,
            unreadable: failed.len(),
        }
    }

    fn all(&self) -> impl Iterator<Item = &Preset> {
        self.factory.iter().chain(&self.user)
    }
}

/// The New graph picker while it is open: what it offers, and the one picked (`None` for the
/// empty graph).
#[derive(Debug, Clone)]
pub(super) struct NewGraphPicker {
    presets: Presets,
    picked: Option<usize>,
}

/// The Save as preset dialog while it is open.
#[derive(Debug, Clone)]
pub(super) struct SavePresetDialog {
    name: String,
    /// The name typed is taken: Save asks to replace it first.
    confirm_replace: bool,
}

/// Opens `dir` in the system's file manager, making it first if it doesn't exist yet.
pub(super) fn open_folder(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program).arg(dir).spawn()?;
    Ok(())
}

impl App {
    /// The folder the user's presets are kept in.
    pub fn preset_dir(&self) -> PathBuf {
        self.settings.library_dir()
    }

    /// Opens the New graph picker; making the graph waits for the user's pick.
    pub fn open_new_graph_picker(&mut self) {
        self.new_graph_picker = Some(NewGraphPicker {
            presets: Presets::read(&self.preset_dir()),
            picked: None,
        });
    }

    /// Adds a graph resource from `preset` (an empty graph without one), named after it, and opens
    /// it.
    pub fn new_graph_from(&mut self, preset: Option<&Preset>) -> ResourceId {
        self.store_editor();
        let id = match preset {
            Some(preset) => self
                .project
                .add_graph(&preset.name, Some(preset.graph.clone())),
            None => self
                .project
                .add_graph(rastersong_engine::DEFAULT_GRAPH_NAME, None),
        };
        self.open_graph(id);
        id
    }

    /// Replaces what the open graph holds with `graph` (one step to undo), keeping the graph's
    /// name and items. With no graph open, makes one first. Importing a graph file and loading a
    /// preset both go through here.
    pub fn load_into_open_graph(&mut self, graph: &GraphDesc) {
        if self.open_graph.is_none() {
            self.new_graph();
        }
        self.editor.load(graph);
        if !self.editor.warnings.is_empty() {
            self.error = Some(self.editor.warnings.join("\n"));
        }
    }

    /// Saves the open graph as preset `name` in the preset folder.
    pub fn save_open_graph_as_preset(&mut self, name: &str) {
        self.store_editor();
        let Some(graph) = self.open_desc().cloned() else {
            return;
        };
        if let Err(e) = save_preset(&self.preset_dir(), name, &graph) {
            self.error = Some(tr_args(
                "presets.save.failed",
                &[("name", name), ("error", &e.to_string())],
            ));
        }
    }

    /// The picker over the window: the empty graph, then the factory presets and the user's, each
    /// a card with what it does. A double-click makes the graph at once.
    pub(super) fn new_graph_picker(&mut self, ctx: &egui::Context) {
        let Some(picker) = &mut self.new_graph_picker else {
            return;
        };
        let mut create = false;
        let mut close = false;
        egui::Modal::new(egui::Id::new("new-graph")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading(tr("presets.new.title"));
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(ctx.content_rect().height() * 0.6)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    let mut card = |ui: &mut Ui, index: Option<usize>, name: &str, about: &str| {
                        let response = choice_card(ui, picker.picked == index, name, about);
                        if response.clicked() {
                            picker.picked = index;
                        }
                        if response.double_clicked() {
                            picker.picked = index;
                            create = true;
                        }
                    };
                    card(
                        ui,
                        None,
                        tr("presets.new.empty"),
                        tr("presets.new.empty.about"),
                    );
                    let factory = picker.presets.factory.len();
                    section_label(ui, tr("presets.new.factory"));
                    for (i, preset) in picker.presets.factory.iter().enumerate() {
                        card(ui, Some(i), &preset.name, preset.description());
                    }
                    section_label(ui, tr("presets.new.user"));
                    if picker.presets.user.is_empty() {
                        ui.weak(tr("presets.new.user_empty"));
                    }
                    for (i, preset) in picker.presets.user.iter().enumerate() {
                        card(ui, Some(factory + i), &preset.name, preset.description());
                    }
                    if picker.presets.unreadable > 0 {
                        ui.weak(tr_args(
                            "presets.new.unreadable",
                            &[("count", &picker.presets.unreadable.to_string())],
                        ));
                    }
                });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(tr("presets.new.create")).clicked() {
                    create = true;
                }
                if ui.button(tr("dialog.cancel")).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    close = true;
                }
            });
        });
        if create {
            let preset = picker
                .picked
                .and_then(|i| picker.presets.all().nth(i).cloned());
            self.new_graph_picker = None;
            self.new_graph_from(preset.as_ref());
        } else if close {
            self.new_graph_picker = None;
        }
    }

    /// The canvas header's Presets button and its menu: Load preset (factory and the user's),
    /// Save as preset… and Open preset folder.
    pub(super) fn presets_menu(&mut self, ui: &mut Ui) {
        let response = ui
            .add(
                egui::Button::new(tr("editor.header.presets"))
                    .min_size(vec2(0.0, HEADER_BUTTON_HEIGHT)),
            )
            .on_hover_text(tr("editor.header.presets.help"));
        if response.clicked() {
            self.header_presets = Some(Presets::read(&self.preset_dir()));
        }
        let Some(presets) = self.header_presets.clone() else {
            return;
        };
        let mut load = None;
        let mut save = false;
        let mut folder = false;
        egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(200.0);
            ui.menu_button(tr("presets.menu.load"), |ui| {
                ui.weak(tr("presets.menu.load.help"));
                ui.set_min_width(200.0);
                section_label(ui, tr("presets.new.factory"));
                for preset in &presets.factory {
                    if load_button(ui, preset) {
                        load = Some(preset.clone());
                    }
                }
                section_label(ui, tr("presets.new.user"));
                if presets.user.is_empty() {
                    ui.weak(tr("presets.menu.none"));
                }
                for preset in &presets.user {
                    if load_button(ui, preset) {
                        load = Some(preset.clone());
                    }
                }
            });
            if ui.button(tr("presets.menu.save")).clicked() {
                save = true;
                ui.close();
            }
            if ui.button(tr("presets.menu.folder")).clicked() {
                folder = true;
                ui.close();
            }
        });
        if let Some(preset) = load {
            self.load_into_open_graph(&preset.graph);
            self.header_presets = None;
        }
        if save {
            self.save_preset_dialog = Some(SavePresetDialog {
                name: self.open_graph_name().to_owned(),
                confirm_replace: false,
            });
        }
        if folder && let Err(e) = open_folder(&self.preset_dir()) {
            self.error = Some(e.to_string());
        }
    }

    /// Asks for the preset's name (the graph's to begin with), and before replacing a preset of
    /// that name.
    pub(super) fn save_preset_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.save_preset_dialog else {
            return;
        };
        let dir = self.settings.library_dir();
        let mut save = false;
        let mut close = false;
        egui::Modal::new(egui::Id::new("save-preset")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading(tr("presets.save.title"));
            ui.add_space(4.0);
            ui.label(tr("presets.save.name"));
            let field = ui.add(
                egui::TextEdit::singleline(&mut dialog.name)
                    .id(egui::Id::new("preset-name"))
                    .desired_width(f32::INFINITY),
            );
            if field.changed() {
                dialog.confirm_replace = false;
            }
            if !field.has_focus() && dialog.name.is_empty() {
                field.request_focus();
            }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.weak(tr_args(
                "presets.save.where",
                &[("folder", &dir.display().to_string())],
            ));
            let path = rastersong_engine::library::preset_path(&dir, &dialog.name);
            if dialog.confirm_replace {
                ui.colored_label(
                    crate::theme::Theme::of(ui.ctx()).warning,
                    tr_args("presets.save.exists", &[("name", dialog.name.trim())]),
                );
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let label = if dialog.confirm_replace {
                    tr("presets.save.replace")
                } else {
                    tr("presets.save.save")
                };
                let clicked = ui
                    .add_enabled(path.is_some(), egui::Button::new(label))
                    .clicked();
                if (clicked || enter)
                    && let Some(path) = &path
                {
                    if path.exists() && !dialog.confirm_replace {
                        dialog.confirm_replace = true;
                    } else {
                        save = true;
                    }
                }
                if ui.button(tr("dialog.cancel")).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    close = true;
                }
            });
        });
        if save {
            let name = dialog.name.trim().to_owned();
            self.save_preset_dialog = None;
            self.save_open_graph_as_preset(&name);
        } else if close {
            self.save_preset_dialog = None;
        }
    }
}

/// A preset in the Load preset menu, with what it does in its tooltip. Returns whether it was
/// clicked.
fn load_button(ui: &mut Ui, preset: &Preset) -> bool {
    let clicked = ui
        .button(&preset.name)
        .on_hover_text(preset.description())
        .clicked();
    if clicked {
        ui.close();
    }
    clicked
}
