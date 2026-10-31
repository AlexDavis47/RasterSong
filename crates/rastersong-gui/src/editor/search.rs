//! The add-node search palette and the node context menu.

use std::collections::BTreeSet;

use eframe::egui::{
    self, Align2, FontId, Key, Pos2, Rect, Sense, Stroke, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use rastersong_engine::NodeType;
use rastersong_lang::{tr, tr_args};

use super::canvas::{Geometry, Pin};
use super::ports;
use super::{GraphEditor, NodeKey};
use crate::theme::{SMALL_SIZE, Theme};
use crate::widgets::{kbd, paint_kbd};

static NEXT_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Width of the palette.
pub(super) const WIDTH: f32 = 370.0;
/// Height of the list of node types, fixed so the popup doesn't change size as it filters.
const LIST_HEIGHT: f32 = 320.0;
/// Height of one result: its name over its one-line description.
const RESULT_HEIGHT: f32 = 44.0;

/// Opened by right-clicking empty canvas, Tab, the canvas header's Add node, or dropping a wire on
/// empty canvas. Typing filters; arrows move; Enter or a click adds the node where the palette
/// was opened.
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
            let label = t.label().to_lowercase();
            let rank = if query.is_empty() || label.starts_with(&query) {
                0
            } else if label.contains(&query) || t.kind.contains(&query) {
                1
            } else if t.spec.category.label().to_lowercase().contains(&query)
                || (query.len() >= 3 && t.description().to_lowercase().contains(&query))
            {
                2
            } else {
                return None;
            };
            Some((rank, t))
        })
        .collect();
    found.sort_by_key(|&(rank, t)| (rank, t.spec.category, t.label()));
    found.into_iter().map(|(_, t)| t).collect()
}

/// `found` gathered by category for the palette's headings, each category where its best match
/// was and each keeping its order.
pub(super) fn grouped<'a>(found: &[&'a NodeType]) -> Vec<&'a NodeType> {
    let mut categories = Vec::new();
    for t in found {
        if !categories.contains(&t.spec.category) {
            categories.push(t.spec.category);
        }
    }
    categories
        .into_iter()
        .flat_map(|c| found.iter().copied().filter(move |t| t.spec.category == c))
        .collect()
}

impl GraphEditor {
    pub(super) fn show_search(&mut self, ui: &Ui, geometry: &[Geometry]) {
        let Some(mut menu) = self.search.take() else {
            return;
        };
        let types = self.registry.types();
        let found = grouped(&matches(&types, &menu.query, menu.from));
        menu.selected = menu.selected.min(found.len().saturating_sub(1));
        let theme = Theme::of(ui.ctx());
        let wire = menu
            .from
            .and_then(|pin| self.pin_name(geometry, pin, theme));

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
                    ui.set_width(WIDTH);
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut menu.query)
                            .hint_text(tr("editor.search.hint"))
                            .font(FontId::proportional(13.5))
                            .margin(vec2(10.0, 8.0))
                            .desired_width(f32::INFINITY),
                    );
                    if !menu.opened {
                        edit.request_focus();
                        menu.opened = true;
                    }
                    if edit.changed() {
                        menu.selected = 0;
                    }
                    // What the new node will be connected to, when a wire was dropped.
                    if let Some((name, color)) = &wire {
                        ui.horizontal(|ui| {
                            ui.weak(tr("editor.search.connects_to"));
                            let (swatch, _) =
                                ui.allocate_exact_size(vec2(12.0, 3.0), Sense::hover());
                            ui.painter().rect_filled(swatch, 1, *color);
                            ui.label(name);
                        });
                    }
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .min_scrolled_height(LIST_HEIGHT)
                        .max_height(LIST_HEIGHT)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            if found.is_empty() {
                                ui.weak(tr("editor.search.none"));
                            }
                            let mut category = None;
                            for (i, t) in found.iter().enumerate() {
                                if category != Some(t.spec.category) {
                                    category = Some(t.spec.category);
                                    ui.add_space(if i == 0 { 2.0 } else { 6.0 });
                                    ui.label(
                                        egui::RichText::new(t.spec.category.label().to_uppercase())
                                            .size(SMALL_SIZE)
                                            .color(theme.surfaces.text_faint),
                                    );
                                }
                                let row = result_row(ui, theme, t, i == menu.selected);
                                if i == menu.selected && (up || down) {
                                    row.scroll_to_me(None);
                                }
                                if row.clicked() {
                                    chosen = Some(t.kind.clone());
                                }
                            }
                        });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let hint = |ui: &mut Ui, text: &str| {
                            ui.label(egui::RichText::new(text).size(11.0).color(theme.text_dim));
                            ui.add_space(8.0);
                        };
                        kbd(ui, tr("editor.key.up"));
                        kbd(ui, tr("editor.key.down"));
                        hint(ui, tr("editor.search.move"));
                        kbd(ui, tr("editor.key.enter"));
                        hint(ui, tr("editor.search.add"));
                        kbd(ui, tr("editor.key.escape"));
                        hint(ui, tr("editor.search.close"));
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

    /// A pin as the palette names it ("Three-Band Split.high"), with its wire's colour.
    fn pin_name(
        &self,
        geometry: &[Geometry],
        pin: Pin,
        theme: &Theme,
    ) -> Option<(String, egui::Color32)> {
        let (key, port) = match pin {
            Pin::Out(key, port) | Pin::In(key, port) => (key, port),
        };
        let g = geometry.iter().find(|g| g.key == key)?;
        let kind = self.kind_of(key)?;
        let (port_name, color) = match pin {
            Pin::Out(..) => {
                let shown = g.outputs.get(port).map_or("", |o| o.1);
                let name = if shown.is_empty() {
                    super::editor_outputs(kind).get(port)?.name
                } else {
                    shown
                };
                (name, self.output_color(key, port, theme).solid())
            }
            Pin::In(..) => {
                let name = g.inputs.iter().find(|p| p.port == port)?.name;
                (name, theme.pin_required)
            }
        };
        Some((format!("{}.{port_name}", g.title), color))
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
        // One node's wording, or the many-nodes wording with the count.
        let count = selection.len().to_string();
        let words = |one: &str, many: &str| {
            if selection.len() > 1 {
                tr_args(many, &[("count", &count)])
            } else {
                tr(one).to_owned()
            }
        };
        let area = egui::Area::new(ui.id().with("node-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(menu.screen)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui: &mut Ui| {
                    ui.set_min_width(140.0);
                    if ui
                        .button(words("editor.menu.copy", "editor.menu.copy_many"))
                        .clicked()
                    {
                        self.selected = selection.clone();
                        if let Some(text) = self.copy_selection() {
                            ui.ctx().copy_text(text);
                        }
                        close = true;
                    }
                    if ui
                        .button(words("editor.menu.duplicate", "editor.menu.duplicate_many"))
                        .clicked()
                    {
                        self.duplicate(&selection, self.keep_connections);
                        close = true;
                    }
                    let bypassable = selection
                        .iter()
                        .filter_map(|&k| self.node(k))
                        .filter(|n| n.kind != ports::OUTPUT)
                        .collect::<Vec<_>>();
                    if !bypassable.is_empty() {
                        let mut bypassed = bypassable.iter().all(|n| n.bypass);
                        if ui
                            .checkbox(
                                &mut bypassed,
                                words("editor.menu.bypass", "editor.menu.bypass_many"),
                            )
                            .clicked()
                        {
                            self.toggle_bypass(&selection);
                            close = true;
                        }
                    }
                    if ui
                        .button(words("editor.menu.delete", "editor.menu.delete_many"))
                        .clicked()
                    {
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

/// One result in the palette: its category's dot, its name over its description, and the Enter
/// key on the selected one.
fn result_row(ui: &mut Ui, theme: &Theme, t: &NodeType, selected: bool) -> egui::Response {
    let size = vec2(ui.available_width(), RESULT_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, t.label())
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    let fill = if selected {
        theme.accent.gamma_multiply(0.16)
    } else if response.hovered() {
        theme.surfaces.raised_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    painter.rect(rect, 7, fill, Stroke::NONE, egui::StrokeKind::Inside);
    painter.circle_filled(
        pos2(rect.left() + 14.0, rect.center().y),
        4.0,
        theme.category(Some(t.spec.category)),
    );
    let mut right = rect.right() - 10.0;
    if selected {
        let key = paint_kbd(
            painter,
            theme,
            pos2(right, rect.center().y),
            tr("editor.key.enter"),
        );
        right = key.left() - 8.0;
    }
    let left = rect.left() + 28.0;
    let text = Rect::from_x_y_ranges(left..=right.max(left), rect.y_range());
    painter.text(
        pos2(text.left(), rect.center().y - 1.0),
        Align2::LEFT_BOTTOM,
        t.label(),
        FontId::proportional(13.0),
        theme.surfaces.text,
    );
    let mut description = egui::text::LayoutJob::simple_singleline(
        t.description().to_owned(),
        FontId::proportional(11.0),
        theme.text_dim,
    );
    description.wrap = egui::text::TextWrapping::truncate_at_width(text.width());
    let galley = painter.layout_job(description);
    painter.galley(
        pos2(text.left(), rect.center().y + 1.0),
        galley,
        theme.text_dim,
    );
    response
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
                .map(|t| t.label())
                .collect::<Vec<_>>()
        };
        assert_eq!(labels("del")[0], "Delay");
        assert_eq!(labels("fil")[0], "Filter");
        // "split" matches Split Channels by prefix and Three-Band Split by substring.
        assert_eq!(labels("split")[..2], ["Split Channels", "Three-Band Split"]);
        assert!(labels("zzz").is_empty());
        // Everything but the base nodes every graph has one of (Video In, Audio In, Output);
        // sources, named outputs and audio outputs for other buses are offered.
        let all = labels("");
        assert_eq!(all.len(), types.len() - 3);
        assert!(!all.contains(&"Output") && !all.contains(&"Video In"));
        assert!(all.contains(&"Video Source") && all.contains(&"Named Output"));
    }

    #[test]
    fn results_are_gathered_by_category_best_match_first() {
        let registry = Registry::shared();
        let types = registry.types();
        for query in ["", "de", "split", "a"] {
            let found = matches(&types, query, None);
            let gathered = grouped(&found);
            assert_eq!(gathered.len(), found.len(), "{query}");
            // Each category's results sit together, in the order of their best matches.
            let mut seen = Vec::new();
            for t in &gathered {
                let c = t.spec.category;
                if seen.last() != Some(&c) {
                    assert!(!seen.contains(&c), "{query}: {c:?} split up");
                    seen.push(c);
                }
            }
            if let Some(first) = found.first() {
                assert_eq!(gathered[0].spec.category, first.spec.category, "{query}");
            }
        }
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
