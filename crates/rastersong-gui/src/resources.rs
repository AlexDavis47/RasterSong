//! The Resources panel, the media and the graphs the project uses as cards under two tabs, and
//! the dialog that picks which streams of a file to import.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontId, Galley, Id, Margin, Painter, Rect, RichText, Sense,
    Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use rastersong_engine::{
    GraphEntry, LabelColor, Project, Resource, ResourceId, ResourceKind, StreamInfo, StreamKind,
};
use rastersong_lang::{tr, tr_args};

use crate::name_edit::{is_renaming, rename_field, start_rename};
use crate::theme::{self, CONTROL_RADIUS, DENSE_HEIGHT, SMALL_SIZE, Theme};
use crate::widgets::{Icon, card_header, card_title, channels_label};

/// What the user did in the Resources panel.
#[derive(Debug, Clone, PartialEq)]
pub enum ResourceAction {
    /// Pick files to import.
    Import,
    /// Add a track playing the resource, at the playhead.
    AddToTimeline(ResourceId),
    Rename(ResourceId, String),
    /// Give the resource a label colour (`None` for its kind's own), which its items take too.
    SetColor(ResourceId, Option<LabelColor>),
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

/// What a media card hands over while it is dragged: drop it on the timeline to make a track. When
/// the dragged card is one of several selected, they all come along, in the panel's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraggedResource(pub Vec<ResourceId>);

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

/// The narrowest a card gets: the grid fits as many columns as allow this, then widens them to
/// fill the panel.
pub const CARD_MIN_WIDTH: f32 = 104.0;
/// A card's height for its width: 4:3, wider than high so names have room.
const CARD_ASPECT: f32 = 0.75;
/// Space between cards.
const CARD_GAP: f32 = 8.0;
/// Space inside a card, around the picture and the text.
const CARD_PAD: f32 = 6.0;
/// Height of a card's text under its picture: the name and the detail line.
const CARD_TEXT_HEIGHT: f32 = 32.0;

/// The picture at the top of a card: what kind of resource it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thumb {
    Video,
    Audio,
    Graph,
}

/// What a [`resource_card`] shows. Every resource, whatever its kind, is drawn from one of these,
/// so media and graphs look and drag the same way.
#[derive(Debug)]
pub struct Card<'a> {
    pub id: Id,
    pub thumb: Thumb,
    /// The glyph on the drag bubble.
    pub glyph: &'a str,
    pub name: &'a str,
    /// The line under the name: the stream's shape, or where a graph is used.
    pub detail: String,
    pub hover: String,
    /// The file is gone: the card is drawn in the error color.
    pub missing: bool,
    /// The graph is the one open in the editor.
    pub highlighted: bool,
    /// The card is selected: a drag takes the selection along, and F2 renames it.
    pub selected: bool,
    /// How many cards a drag of this one carries (the selection, when it is in it).
    pub drag_count: usize,
    /// Shown as a button over the card's picture when it is set (Relocate for a missing file):
    /// label, help.
    pub fix_button: Option<(&'a str, &'a str)>,
    /// The label colour, drawn as a band along the top of the picture.
    pub color: Option<LabelColor>,
}

/// What the user did to a card this frame.
#[derive(Debug, Default)]
pub struct CardResponse {
    /// Double-clicked away from the name (a double-click on the name renames).
    pub double_clicked: bool,
    /// Clicked (a double-click counts as two).
    pub clicked: bool,
    pub renamed: Option<String>,
    /// The card's fix button was clicked.
    pub fixed: bool,
}

/// The id of the name on the card `card`: [`start_rename`] with it renames the card in place.
pub fn card_name_id(card: Id) -> Id {
    card.with("name")
}

/// Like [`Ui::dnd_drag_source`] over `rect`, but it also senses clicks (egui's version only
/// senses drags, so double-click and the right-click menu never reach it), and while it is
/// dragged the [drag bubble](crate::widgets::drag_bubble) with `bubble` (icon, label) follows the
/// pointer and `paint` draws the body faded in place.
fn drag_source<P: std::any::Any + Send + Sync>(
    ui: &Ui,
    id: Id,
    rect: Rect,
    payload: P,
    bubble: (&str, &str),
    paint: impl FnOnce(&Painter, bool),
) -> egui::Response {
    let response = ui
        .interact(rect, id, Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::Grab);
    if ui.ctx().is_being_dragged(id) {
        egui::DragAndDrop::set_payload(ui.ctx(), payload);
        let mut painter = ui.painter().clone();
        painter.multiply_opacity(0.4);
        paint(&painter, false);
        crate::widgets::drag_bubble(ui.ctx(), rect, bubble.0, bubble.1);
    } else {
        paint(ui.painter(), response.hovered());
    }
    response
}

/// `text` on one line at most `width` wide, cut short with an ellipsis.
fn one_line(ui: &Ui, text: &str, font: FontId, color: Color32, width: f32) -> Arc<Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));
    ui.painter().layout_job(job)
}

/// Paints the picture for `thumb` in `rect`: a frame for video, a waveform for sound, two joined
/// nodes for a graph, a warning sign for a missing file.
fn paint_thumb(painter: &Painter, rect: Rect, thumb: Thumb, missing: bool, theme: &Theme) {
    let square = Rect::from_center_size(rect.center(), Vec2::splat(rect.height().min(30.0)));
    if missing {
        painter.rect_filled(rect, 4, theme.error.gamma_multiply(0.15));
        let sign = square.shrink(4.0);
        let stroke = Stroke::new(1.5, theme.error);
        let at = |x: f32, y: f32| sign.lerp_inside(vec2(x, y));
        painter.add(egui::Shape::closed_line(
            vec![at(0.5, 0.0), at(1.0, 0.95), at(0.0, 0.95)],
            stroke,
        ));
        painter.line_segment([at(0.5, 0.35), at(0.5, 0.6)], stroke);
        painter.circle_filled(at(0.5, 0.78), 1.1, theme.error);
        return;
    }
    match thumb {
        Thumb::Video => {
            painter.rect_filled(rect, 4, theme.video_item.fill);
            crate::widgets::paint_icon(painter, square, Icon::Picture, theme.surfaces.text_soft);
        }
        Thumb::Audio => {
            painter.rect_filled(rect, 4, theme.ports.audio.gamma_multiply(0.14));
            // A still waveform: the same shape on every card, standing for sound.
            const LEVELS: [f32; 16] = [
                0.0, 0.5, -0.8, 0.9, -0.5, 0.4, -0.7, 0.8, -0.3, 0.6, -0.9, 0.5, -0.4, 0.3, -0.1,
                0.0,
            ];
            let wave = rect.shrink2(vec2(10.0, rect.height() * 0.22));
            let points = LEVELS
                .iter()
                .enumerate()
                .map(|(i, level)| {
                    let x = wave.left() + wave.width() * i as f32 / (LEVELS.len() - 1) as f32;
                    pos2(x, wave.center().y - level * wave.height() / 2.0)
                })
                .collect();
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1.3, theme.ports.audio),
            ));
        }
        Thumb::Graph => {
            painter.rect_filled(rect, 4, theme.graph_block.gamma_multiply(0.16));
            let ink = Stroke::new(1.4, theme.graph_block);
            let side = square.width() * 0.28;
            let node = |x: f32, y: f32| {
                Rect::from_min_size(square.lerp_inside(vec2(x, y)), Vec2::splat(side))
            };
            let (a, b) = (node(0.05, 0.1), node(0.67, 0.62));
            painter.rect_stroke(a, 1, ink, StrokeKind::Inside);
            painter.rect_stroke(b, 1, ink, StrokeKind::Inside);
            let bend = (a.right() + b.left()) / 2.0;
            painter.add(egui::Shape::line(
                vec![
                    a.right_center(),
                    pos2(bend, a.center().y),
                    pos2(bend, b.center().y),
                    b.left_center(),
                ],
                ink,
            ));
        }
    }
}

/// The label colours a resource of `kind` can take, its kind's own first, each with its swatch
/// at the end of the row and the `current` one checked. Returns the one clicked.
fn label_color_menu(
    ui: &mut Ui,
    kind: ResourceKind,
    current: Option<LabelColor>,
) -> Option<Option<LabelColor>> {
    let theme = Theme::of(ui.ctx());
    let mut picked = None;
    for color in std::iter::once(None).chain(LabelColor::ALL.map(Some)) {
        let name = color.map_or(tr("label_color.none"), theme::label_color_name);
        let button = egui::Button::new(name)
            .selected(current == color)
            .min_size(vec2(140.0, 0.0));
        let response = ui.add(button);
        let colors = theme.item_colors(kind.track_kind(), color);
        let dot = Rect::from_center_size(
            pos2(response.rect.right() - 12.0, response.rect.center().y),
            Vec2::splat(10.0),
        );
        ui.painter().rect(
            dot,
            CornerRadius::same(3),
            colors.bar,
            Stroke::new(1.0, colors.text.gamma_multiply(0.5)),
            StrokeKind::Inside,
        );
        if response.clicked() {
            picked = Some(color);
        }
    }
    picked
}

/// The one card every resource is drawn with, `size` big: a picture of its kind, the name and a
/// detail line under it. The card is a drag source carrying `payload`; the fix button sits over
/// the picture. Double-clicking the name (or F2, through [`start_rename`] with
/// [`card_name_id`]) renames it in place; right-click opens `menu`, after a Rename entry.
pub fn resource_card<P: std::any::Any + Send + Sync>(
    ui: &mut Ui,
    card: &Card<'_>,
    size: Vec2,
    payload: P,
    menu: impl FnOnce(&mut Ui),
) -> CardResponse {
    let mut out = CardResponse::default();
    let theme = Theme::of(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let inner = rect.shrink(CARD_PAD);
    let picture = inner.with_max_y(inner.bottom() - CARD_TEXT_HEIGHT);
    let name_row = Rect::from_min_size(
        pos2(inner.left(), picture.bottom() + 3.0),
        vec2(inner.width(), 16.0),
    );
    let detail_top = name_row.bottom() + 1.0;
    let fix_font = FontId::new(
        11.5,
        ui.style().text_styles[&egui::TextStyle::Button]
            .family
            .clone(),
    );
    let label = if card.drag_count > 1 {
        tr_args(
            "resources.drag.more",
            &[
                ("name", card.name),
                ("count", &(card.drag_count - 1).to_string()),
            ],
        )
    } else {
        card.name.to_owned()
    };
    let name_id = card_name_id(card.id);
    let renaming = is_renaming(ui.ctx(), name_id);
    let paint = |painter: &Painter, hovered: bool| {
        let (fill, stroke) = if card.missing {
            (
                theme.error.gamma_multiply(0.06),
                Stroke::new(1.0, theme.error.gamma_multiply(0.6)),
            )
        } else if card.selected {
            (
                theme.surfaces.raised,
                Stroke::new(1.5, ui.visuals().selection.stroke.color),
            )
        } else if card.highlighted {
            (
                theme.surfaces.raised,
                Stroke::new(1.0, theme.surfaces.border),
            )
        } else if hovered {
            (
                theme.surfaces.raised,
                Stroke::new(1.0, theme.surfaces.divider),
            )
        } else {
            (
                theme.surfaces.inner_card,
                Stroke::new(1.0, theme.surfaces.divider),
            )
        };
        painter.rect(rect, CONTROL_RADIUS, fill, stroke, StrokeKind::Inside);
        paint_thumb(painter, picture, card.thumb, card.missing, theme);
        if let Some(color) = card.color {
            let band = picture.with_max_y(picture.top() + 4.0);
            let colors = theme.label_colors(color);
            painter.rect_filled(
                band,
                CornerRadius {
                    nw: 4,
                    ne: 4,
                    sw: 0,
                    se: 0,
                },
                colors.bar,
            );
        }
        let (name_font, name_color) = if card.missing {
            (FontId::proportional(12.5), theme.error)
        } else if card.highlighted {
            (theme::semibold(12.5), theme.surfaces.text)
        } else {
            (FontId::proportional(12.5), theme.surfaces.text)
        };
        if !renaming {
            let name = one_line(ui, card.name, name_font, name_color, inner.width());
            let top = name_row.center().y - name.size().y / 2.0;
            painter.galley(pos2(inner.left(), top), name, name_color);
        }
        let detail = one_line(
            ui,
            &card.detail,
            FontId::proportional(SMALL_SIZE),
            theme.surfaces.text_faint,
            inner.width(),
        );
        painter.galley(
            pos2(inner.left(), detail_top),
            detail,
            theme.surfaces.text_faint,
        );
    };
    let body = drag_source(ui, card.id, rect, payload, (card.glyph, &label), paint);
    body.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, card.name));
    // The detail line too, for screen readers (and tests).
    ui.ctx()
        .accesskit_node_builder(body.id, |node| node.set_description(card.detail.clone()));
    let body = body.on_hover_text(&card.hover);
    body.context_menu(|ui| {
        let rename = egui::Button::new(tr("resources.rename")).shortcut_text("F2");
        if ui.add(rename).clicked() {
            start_rename(ui.ctx(), name_id);
            ui.close();
        }
        menu(ui);
    });
    if let Some(edit) = rename_field(ui, name_id, name_row, card.name, |t| t) {
        out.renamed = edit.committed;
    }
    let on_name = body
        .interact_pointer_pos()
        .is_some_and(|p| name_row.contains(p));
    if body.double_clicked() {
        if on_name {
            start_rename(ui.ctx(), name_id);
        } else {
            out.double_clicked = true;
        }
    }
    out.clicked = body.clicked();
    if let Some((label, help)) = card.fix_button {
        let button = egui::Button::new(RichText::new(label).font(fix_font));
        let width = ui
            .painter()
            .layout_no_wrap(
                label.to_owned(),
                FontId::proportional(11.5),
                theme.surfaces.text,
            )
            .size()
            .x
            + 20.0;
        let fix = Rect::from_center_size(
            pos2(
                picture.center().x,
                picture.bottom() - DENSE_HEIGHT / 2.0 - 2.0,
            ),
            vec2(width.min(picture.width()), DENSE_HEIGHT),
        );
        // Placed over the card's picture, on top of the drag area, so it takes the click.
        out.fixed = ui.place(fix, button).on_hover_text(help).clicked();
    }
    out
}

/// Lays `count` cards out in rows, as many to a row as fit at [`CARD_MIN_WIDTH`], widened to
/// fill the width and 4:3. `add` draws card `i` at the size it is given.
fn card_grid(ui: &mut Ui, count: usize, mut add: impl FnMut(&mut Ui, usize, Vec2)) {
    let width = ui.available_width();
    let columns = (((width + CARD_GAP) / (CARD_MIN_WIDTH + CARD_GAP)).floor() as usize).max(1);
    let card_width = (width - CARD_GAP * (columns - 1) as f32) / columns as f32;
    let size = vec2(card_width, (card_width * CARD_ASPECT).round());
    for row in (0..count).collect::<Vec<_>>().chunks(columns) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = CARD_GAP;
            for &i in row {
                add(ui, i, size);
            }
        });
        ui.add_space(CARD_GAP - ui.spacing().item_spacing.y);
    }
}

/// The panel's two tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Media,
    Graphs,
}

/// The panel: a title row with the filter and Import…, then the Media and Graphs tabs, each a
/// grid of cards. `streams` holds each linked file's streams as far as they are known, for the
/// cards' detail lines. With `open_on_click`, one click on a graph opens it (a setting).
pub fn resources_panel(
    ui: &mut Ui,
    project: &Project,
    missing: &HashSet<ResourceId>,
    streams: &HashMap<PathBuf, Vec<StreamInfo>>,
    open_on_click: bool,
) -> Vec<ResourceAction> {
    let mut actions = Vec::new();
    let filter_id = Id::new("resources-filter");
    let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
    card_header(ui, |ui| {
        card_title(ui, tr("resources.title"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(tr("resources.import"))
                .on_hover_text(tr("resources.import.help"))
                .clicked()
            {
                actions.push(ResourceAction::Import);
            }
            ui.add(
                egui::TextEdit::singleline(&mut filter)
                    .id(filter_id.with("field"))
                    .hint_text(tr("resources.filter"))
                    .vertical_align(egui::Align::Center)
                    .min_size(vec2(0.0, DENSE_HEIGHT))
                    .desired_width(ui.available_width()),
            )
            .on_hover_text(tr("resources.filter.help"));
        });
    });
    ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
    let needle = filter.trim().to_lowercase();
    let matches = |name: &str| name.to_lowercase().contains(&needle);
    let media: Vec<&Resource> = project
        .resources
        .iter()
        .filter(|r| matches(&r.name))
        .collect();
    let graphs: Vec<GraphEntry> = project
        .graph_entries()
        .into_iter()
        .filter(|g| matches(&g.name))
        .collect();
    let tab_id = Id::new("resources-tab");
    let mut tab: Tab = ui.data(|d| d.get_temp(tab_id)).unwrap_or_default();
    let panel = ui.available_rect_before_wrap();
    // F2 over the panel renames the selected card of the open tab.
    let rename = ui.rect_contains_pointer(panel)
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::F2));
    egui::Frame::new()
        .inner_margin(Margin::symmetric(8, 0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let media_tab = tr_args(
                    "resources.media_count",
                    &[("count", &media.len().to_string())],
                );
                let graph_tab = tr_args(
                    "resources.graph_count",
                    &[("count", &graphs.len().to_string())],
                );
                let options = [
                    (media_tab.as_str(), tr("resources.tab.media.help")),
                    (graph_tab.as_str(), tr("resources.tab.graphs.help")),
                ];
                let selected = usize::from(tab == Tab::Graphs);
                let row = ui.available_rect_before_wrap();
                if let Some(i) = crate::widgets::tabs(ui, &options, selected) {
                    tab = if i == 0 { Tab::Media } else { Tab::Graphs };
                }
                if tab == Tab::Graphs {
                    // "+ New" at the tab row's right end, over its divider.
                    let new =
                        egui::Button::new(RichText::new(tr("resources.graph.new")).size(11.5))
                            .frame(false)
                            .min_size(vec2(0.0, 22.0));
                    let at = Rect::from_min_max(
                        pos2(row.right() - 60.0, row.top()),
                        pos2(row.right(), row.top() + crate::widgets::TABS_HEIGHT),
                    );
                    let new = ui
                        .put(at, new)
                        .on_hover_text(tr("resources.graph.new.help"));
                    if new.clicked() {
                        actions.push(ResourceAction::NewGraph);
                    }
                }
            });
        });
    ui.data_mut(|d| d.insert_temp(tab_id, tab));
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(Margin::symmetric(8, 10))
                .show(ui, |ui| match tab {
                    Tab::Media => {
                        media_group(ui, project, &media, missing, streams, rename, &mut actions);
                    }
                    Tab::Graphs => {
                        graph_group(ui, project, &graphs, open_on_click, rename, &mut actions);
                    }
                });
        });
    actions
}

/// A quiet line in a tab: why it is empty, or the hint under the cards.
fn faint_note(ui: &mut Ui, text: &str) {
    let theme = Theme::of(ui.ctx());
    let text = RichText::new(text)
        .size(11.0)
        .color(theme.surfaces.text_faint);
    ui.add(egui::Label::new(text).wrap());
}

/// What a media card says under its name: the kind, then the stream's shape once it is known
/// ("video · 320×180 · 30 fps", "audio · stereo · 48 kHz").
pub fn media_detail(resource: &Resource, stream: Option<&StreamInfo>) -> String {
    let mut parts = vec![match resource.kind {
        ResourceKind::Video => tr("resources.detail.video").to_owned(),
        ResourceKind::Audio => tr("resources.detail.audio").to_owned(),
    }];
    match stream.map(|s| s.kind) {
        Some(StreamKind::Video {
            width,
            height,
            frame_rate,
        }) => {
            parts.push(format!("{width}×{height}"));
            if frame_rate > 0.0 {
                let rate = format!("{frame_rate:.2}");
                let rate = rate.trim_end_matches('0').trim_end_matches('.');
                parts.push(tr_args("resources.detail.fps", &[("n", rate)]));
            }
        }
        Some(StreamKind::Audio {
            sample_rate,
            channels,
        }) => {
            parts.push(channels_label(channels).to_lowercase());
            let khz = format!("{:.1}", f64::from(sample_rate) / 1000.0);
            let khz = khz.trim_end_matches(".0");
            parts.push(tr_args("resources.detail.khz", &[("n", khz)]));
        }
        None => {}
    }
    parts.join(" · ")
}

/// The stream of its file that `resource` plays, among `streams` (the file's): the one it names,
/// or the first of its kind.
fn resource_stream<'a>(resource: &Resource, streams: &'a [StreamInfo]) -> Option<&'a StreamInfo> {
    match resource.stream {
        Some(index) => streams.iter().find(|s| s.index == index),
        None => streams.iter().find(|s| resource_kind(s) == resource.kind),
    }
}

/// The media cards. Click selects a card, Ctrl or Shift+click adds it to the selection or takes
/// it out; `rename` (F2) renames the card clicked last.
fn media_group(
    ui: &mut Ui,
    project: &Project,
    shown: &[&Resource],
    missing: &HashSet<ResourceId>,
    streams: &HashMap<PathBuf, Vec<StreamInfo>>,
    rename: bool,
    actions: &mut Vec<ResourceAction>,
) {
    if project.resources.is_empty() {
        faint_note(ui, tr("resources.empty"));
    } else if shown.is_empty() {
        faint_note(ui, tr("resources.no_match"));
    }
    // The selected cards, which a drag of any of them takes along, the last clicked last.
    let selection_id = Id::new("resource-selection");
    let mut selection: Vec<ResourceId> = ui.data(|d| d.get_temp(selection_id)).unwrap_or_default();
    selection.retain(|&id| project.resource(id).is_some());
    let mut clicked = None;
    card_grid(ui, shown.len(), |ui, i, size| {
        let resource = shown[i];
        let dragged: Vec<ResourceId> = if selection.contains(&resource.id) {
            shown
                .iter()
                .map(|r| r.id)
                .filter(|id| selection.contains(id))
                .collect()
        } else {
            vec![resource.id]
        };
        let missing = missing.contains(&resource.id);
        let users = project.resource_users(resource.id).len();
        let (glyph, thumb) = match resource.kind {
            ResourceKind::Video => ("🎞", Thumb::Video),
            ResourceKind::Audio => ("🔊", Thumb::Audio),
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
        let detail = if missing {
            tr("resources.detail.missing").to_owned()
        } else {
            let stream = streams
                .get(&resource.path)
                .and_then(|s| resource_stream(resource, s));
            media_detail(resource, stream)
        };
        let card = Card {
            id: Id::new(("resource", resource.id)),
            thumb,
            glyph,
            name: &resource.name,
            detail,
            hover,
            missing,
            highlighted: false,
            selected: selection.contains(&resource.id),
            drag_count: dragged.len(),
            fix_button: missing.then(|| (tr("resources.relocate"), tr("resources.relocate.help"))),
            color: resource.color,
        };
        if rename && selection.last() == Some(&resource.id) {
            start_rename(ui.ctx(), card_name_id(card.id));
        }
        let out = resource_card(ui, &card, size, DraggedResource(dragged), |ui| {
            ui.label(RichText::new(file).weak());
            if ui.button(tr("resources.add_to_timeline")).clicked() {
                actions.push(ResourceAction::AddToTimeline(resource.id));
                ui.close();
            }
            ui.menu_button(tr("resources.color"), |ui| {
                if let Some(color) = label_color_menu(ui, resource.kind, resource.color) {
                    actions.push(ResourceAction::SetColor(resource.id, color));
                    ui.close();
                }
            })
            .response
            .on_hover_text(tr("resources.color.help"));
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
        if out.clicked {
            clicked = Some(resource.id);
        }
    });
    if let Some(id) = clicked {
        if ui.input(|i| i.modifiers.command || i.modifiers.shift) {
            match selection.iter().position(|&s| s == id) {
                Some(at) => {
                    selection.remove(at);
                }
                None => selection.push(id),
            }
        } else {
            selection = vec![id];
        }
    }
    ui.data_mut(|d| d.insert_temp(selection_id, selection));
    if !shown.is_empty() {
        faint_note(ui, tr("resources.hint.media"));
    }
}

/// What a graph hands over while it is dragged. The timeline takes it: dropped on a graph
/// layer it places the graph there, anywhere else it makes a new layer for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DraggedGraph(pub u32);

/// What a graph card says under its name: whether it is open, and how many items place it.
pub fn graph_detail(project: &Project, entry: &GraphEntry) -> String {
    let items = project
        .graph_tracks
        .iter()
        .flat_map(|t| &t.items)
        .filter(|item| item.graph == entry.id)
        .count();
    let placed = match items {
        0 => tr("resources.graph.unplaced").to_owned(),
        1 => tr("resources.graph.one_item").to_owned(),
        n => tr_args("resources.graph.items", &[("count", &n.to_string())]),
    };
    if entry.open {
        format!("{} · {placed}", tr("resources.graph.open_tag"))
    } else {
        placed
    }
}

/// The graph cards, the open one marked. Double-click opens a graph in the editor, or one click
/// with `open_on_click`; a click selects it, and `rename` (F2) renames the selected one.
fn graph_group(
    ui: &mut Ui,
    project: &Project,
    shown: &[GraphEntry],
    open_on_click: bool,
    rename: bool,
    actions: &mut Vec<ResourceAction>,
) {
    if shown.is_empty() {
        faint_note(ui, tr("resources.no_match"));
    }
    let selection_id = Id::new("graph-selection");
    let mut selection: Option<u32> = ui.data(|d| d.get_temp(selection_id)).flatten();
    let help = if open_on_click {
        tr("resources.graph.help_click")
    } else {
        tr("resources.graph.help")
    };
    card_grid(ui, shown.len(), |ui, i, size| {
        let entry = &shown[i];
        let card = Card {
            id: Id::new(("graph", entry.id)),
            thumb: Thumb::Graph,
            glyph: "🕸",
            name: &entry.name,
            detail: graph_detail(project, entry),
            hover: if entry.open {
                tr("resources.graph.open_now")
            } else {
                help
            }
            .to_owned(),
            missing: false,
            highlighted: entry.open,
            selected: selection == Some(entry.id),
            drag_count: 1,
            fix_button: None,
            color: None,
        };
        if rename && selection == Some(entry.id) {
            start_rename(ui.ctx(), card_name_id(card.id));
        }
        let out = resource_card(ui, &card, size, DraggedGraph(entry.id), |ui| {
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
        if out.clicked {
            selection = Some(entry.id);
        }
        let open = out.double_clicked || (open_on_click && out.clicked);
        if open && !entry.open {
            actions.push(ResourceAction::OpenGraph(entry.id));
        }
    });
    ui.data_mut(|d| d.insert_temp(selection_id, selection));
    if !shown.is_empty() {
        faint_note(
            ui,
            if open_on_click {
                tr("resources.hint.graphs_click")
            } else {
                tr("resources.hint.graphs")
            },
        );
    }
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
