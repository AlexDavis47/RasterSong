//! The add-node search menu and the node context menu.

use std::collections::BTreeSet;

use eframe::egui::{self, Key, Pos2, Ui};
use rastersong_engine::NodeType;

use super::canvas::{Geometry, Pin};
use super::linked;
use super::{GraphEditor, NodeKey};
use crate::theme::Theme;

static NEXT_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Height of the list of node types, fixed so the popup doesn't change size as it filters.
const LIST_HEIGHT: f32 = 320.0;

/// Opened by right-clicking empty canvas (or dropping a wire there). Typing filters; arrows move;
/// Enter or a click adds the node where the menu was opened.
#[derive(Debug, Clone)]
pub(super) struct SearchMenu {
    /// Screen position of the menu.
    pub screen: Pos2,
    /// Where the node goes, in graph space.
    pub graph: Pos2,
    /// A wire being dragged when the menu opened; the new node is connected to it.
    pub from: Option<Pin>,
    pub query: String,
    pub selected: usize,
    pub opened: bool,
    /// Distinguishes this opening from earlier ones, so egui doesn't size the popup from a
    /// previous, shorter list.
    pub serial: u64,
}

impl SearchMenu {
    pub fn new(screen: Pos2, graph: Pos2, from: Option<Pin>) -> Self {
        Self {
            screen,
            graph,
            from,
            query: String::new(),
            selected: 0,
            opened: false,
            serial: NEXT_SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }
}

/// Opened by right-clicking a node.
#[derive(Debug, Clone, Copy)]
pub(super) struct NodeMenu {
    pub screen: Pos2,
    pub node: NodeKey,
}

/// Node types matching `query`, best first: label prefix matches, then any match in the label,
/// type name, category or description. Types that can't take the dragged wire are left out.
pub(super) fn matches<'a>(
    types: &[&'a NodeType],
    query: &str,
    wire: Option<Pin>,
) -> Vec<&'a NodeType> {
    let query = query.trim().to_lowercase();
    let fits = |t: &NodeType| match wire {
        Some(Pin::Out(..)) => !t.spec.inputs.is_empty(),
        Some(Pin::In(_, port)) if super::as_param(port).is_some() => {
            !super::editor_outputs(t).is_empty()
        }
        Some(Pin::In(..)) => !super::editor_outputs(t).is_empty(),
        None => true,
    };
    let mut found: Vec<(u8, &NodeType)> = types
        .iter()
        .filter(|t| GraphEditor::user_addable(t))
        .filter(|t| fits(t))
        .filter_map(|&t| {
            let label = t.spec.label.to_lowercase();
            let rank = if query.is_empty() || label.starts_with(&query) {
                0
            } else if label.contains(&query) || t.kind.contains(&query) {
                1
            } else if t.spec.category.label().to_lowercase().contains(&query)
                || (query.len() >= 3 && t.spec.description.to_lowercase().contains(&query))
            {
                2
            } else {
                return None;
            };
            Some((rank, t))
        })
        .collect();
    found.sort_by_key(|&(rank, t)| (rank, t.spec.category, t.spec.label));
    found.into_iter().map(|(_, t)| t).collect()
}

impl GraphEditor {
    pub(super) fn show_search(&mut self, ui: &Ui, _geometry: &[Geometry]) {
        let Some(mut menu) = self.search.take() else {
            return;
        };
        let types = self.registry.types();
        let found = matches(&types, &menu.query, menu.from);
        menu.selected = menu.selected.min(found.len().saturating_sub(1));

        let (up, down, enter, escape) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::Enter),
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
            )
        });
        if up {
            menu.selected = menu.selected.saturating_sub(1);
        }
        if down && menu.selected + 1 < found.len() {
            menu.selected += 1;
        }

        let mut chosen = enter
            .then(|| found.get(menu.selected).map(|t| t.kind.clone()))
            .flatten();
        let area = egui::Area::new(ui.id().with(("node-search", menu.serial)))
            .order(egui::Order::Foreground)
            .fixed_pos(menu.screen)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(240.0);
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut menu.query)
                            .hint_text("Search nodes…")
                            .desired_width(f32::INFINITY),
                    );
                    if !menu.opened {
                        edit.request_focus();
                        menu.opened = true;
                    }
                    if edit.changed() {
                        menu.selected = 0;
                    }
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .min_scrolled_height(LIST_HEIGHT)
                        .max_height(LIST_HEIGHT)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if found.is_empty() {
                                ui.weak("No matching nodes");
                            }
                            for (i, t) in found.iter().enumerate() {
                                let row = ui.horizontal(|ui| {
                                    let (dot, _) = ui.allocate_exact_size(
                                        egui::vec2(8.0, 8.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().circle_filled(
                                        dot.center(),
                                        3.5,
                                        Theme::of(ui.ctx()).category(Some(t.spec.category)),
                                    );
                                    let label =
                                        ui.selectable_label(i == menu.selected, t.spec.label);
                                    ui.weak(t.spec.category.label());
                                    label
                                });
                                let label = row.inner.on_hover_text(t.spec.description);
                                if i == menu.selected && (up || down) {
                                    label.scroll_to_me(None);
                                }
                                if label.clicked() {
                                    chosen = Some(t.kind.clone());
                                }
                            }
                        });
                });
            });

        // A click anywhere else closes the menu.
        let clicked_outside = ui.input(|i| i.pointer.any_pressed())
            && ui
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| !area.response.rect.contains(p));
        if let Some(kind) = chosen {
            self.add_from_search(&menu, &kind);
        } else if !(escape || clicked_outside) {
            self.search = Some(menu);
        }
    }

    fn add_from_search(&mut self, menu: &SearchMenu, kind: &str) {
        let Some(key) = self.add_node(kind, menu.graph) else {
            return;
        };
        match menu.from {
            Some(Pin::Out(node, output)) => self.connect_new((node, output), (key, 0)),
            Some(Pin::In(node, input)) => self.connect_new((key, 0), (node, input)),
            None => {}
        }
        self.set_active(Some(key));
    }

    pub(super) fn show_node_menu(&mut self, ui: &Ui) {
        let Some(menu) = self.node_menu else {
            return;
        };
        let mut close = ui.input(|i| i.key_pressed(Key::Escape));
        let selection: BTreeSet<NodeKey> = if self.selected.contains(&menu.node) {
            self.selected.clone()
        } else {
            BTreeSet::from([menu.node])
        };
        let plural = if selection.len() > 1 {
            format!(" {} nodes", selection.len())
        } else {
            String::new()
        };
        let area = egui::Area::new(ui.id().with("node-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(menu.screen)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui: &mut Ui| {
                    ui.set_min_width(140.0);
                    if ui.button(format!("Copy{plural}")).clicked() {
                        self.selected = selection.clone();
                        if let Some(text) = self.copy_selection() {
                            ui.ctx().copy_text(text);
                        }
                        close = true;
                    }
                    if ui.button(format!("Duplicate{plural}")).clicked() {
                        self.duplicate(&selection, self.keep_connections);
                        close = true;
                    }
                    let bypassable = selection
                        .iter()
                        .filter_map(|&k| self.node(k))
                        .filter(|n| n.kind != linked::OUTPUT)
                        .collect::<Vec<_>>();
                    if !bypassable.is_empty() {
                        let mut bypassed = bypassable.iter().all(|n| n.bypass);
                        if ui
                            .checkbox(&mut bypassed, format!("Bypass{plural}"))
                            .clicked()
                        {
                            self.toggle_bypass(&selection);
                            close = true;
                        }
                    }
                    if ui.button(format!("Delete{plural}")).clicked() {
                        self.remove_nodes(&selection);
                        close = true;
                    }
                });
            });
        let clicked_outside = ui.input(|i| i.pointer.any_pressed())
            && ui
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| !area.response.rect.contains(p));
        if close || clicked_outside {
            self.node_menu = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use rastersong_engine::Registry;

    use super::*;

    #[test]
    fn ranks_prefix_matches_first() {
        let registry = Registry::shared();
        let types = registry.types();
        let labels = |q: &str| {
            matches(&types, q, None)
                .iter()
                .map(|t| t.spec.label)
                .collect::<Vec<_>>()
        };
        assert_eq!(labels("del")[0], "Delay");
        assert_eq!(labels("fil")[0], "Filter");
        // "split" matches Split Channels by prefix and Three-Band Split by substring.
        assert_eq!(labels("split")[..2], ["Split Channels", "Three-Band Split"]);
        assert!(labels("zzz").is_empty());
        // Everything but the project's inputs and output.
        let all = labels("");
        assert_eq!(all.len(), types.len() - 3);
        assert!(!all.contains(&"Output") && !all.contains(&"Video"));
    }

    #[test]
    fn dragged_wires_only_offer_nodes_that_can_take_them() {
        let registry = Registry::shared();
        let types = registry.types();
        let from_output = matches(&types, "", Some(Pin::Out(1, 0)));
        assert!(from_output.iter().all(|t| !t.spec.inputs.is_empty()));
        assert!(!from_output.iter().any(|t| t.kind == "video_input"));
        let from_input = matches(&types, "", Some(Pin::In(1, 0)));
        assert!(!from_input.iter().any(|t| t.kind == "output"));
    }
}
