//! Comments, notes and node descriptions on the canvas.
//!
//! - A **comment** is a box drawn behind the nodes, with its title (the node's label) and its
//!   description above the box, so they never cover what is inside. The text grabs it: dragging
//!   it moves the comment and every node inside it; the box itself is canvas, so box selection
//!   and right-click work there as anywhere. Its bottom-right corner resizes it. The node's
//!   position and size are the box's; the text grows upward from it.
//! - A **note** is a sticky note drawn above the nodes, showing its description. A small tab sits
//!   in its top-right corner: dragging the tab onto any node (a comment or another note too) attaches
//!   the note to it, and dropping it on empty canvas loosens it. Attached, the note moves with
//!   its node and points at it with a speech bubble's tail.
//! - Any other node's description is drawn in small text under it.

use std::collections::BTreeSet;

use eframe::egui::{
    self, Color32, CornerRadius, FontId, Galley, Pos2, Rect, Stroke, StrokeKind, Vec2, pos2, vec2,
};
use rastersong_engine::{COMMENT, NOTE};
use rastersong_lang::tr;

use super::canvas::{Geometry, Interaction, Layer};
use super::{EditorNode, GraphEditor, NodeKey};
use crate::theme::{Theme, semibold};

/// A new comment's box, and the smallest it can be resized to.
pub(super) const COMMENT_SIZE: Vec2 = vec2(360.0, 220.0);
const COMMENT_MIN: Vec2 = vec2(160.0, 80.0);
/// The comment's title row above its box, and the text sizes of its title and description.
const COMMENT_TITLE_ROW: f32 = 24.0;
/// How far the comment's text sits in from the box's left edge, and the space under it.
const TEXT_INSET: f32 = 4.0;
const TEXT_GAP: f32 = 6.0;
const COMMENT_TITLE_FONT: f32 = 15.0;
const TEXT_FONT: f32 = 12.0;
const PAD: f32 = 10.0;
/// The square in a comment's bottom-right corner that resizes it, in screen pixels.
const HANDLE: f32 = 14.0;
/// How wide a note is, and how far its tail reaches toward the node it points at.
pub(super) const NOTE_WIDTH: f32 = 200.0;
const TAIL_BASE: f32 = 14.0;
/// The tab in a note's top-right corner that attaches it, at zoom 1: its side and how far in
/// from the note's top and right edges.
const TAB: f32 = 14.0;
const TAB_INSET: f32 = 5.0;
/// How wide a note's text wraps: inside its padding, clear of the tab.
const NOTE_TEXT_WIDTH: f32 = NOTE_WIDTH - 2.0 * PAD - (TAB + TAB_INSET - PAD + 3.0);
/// How wide a node's description wraps under it.
const DESCRIPTION_WIDTH: f32 = 220.0;
const DESCRIPTION_FONT: f32 = 11.0;

/// Whether `node` is a comment, a note or neither.
pub(super) fn layer_of(node: &EditorNode) -> Layer {
    match node.kind.as_str() {
        COMMENT => Layer::Comment,
        NOTE => Layer::Note,
        _ => Layer::Node,
    }
}

/// The description, when it has any text.
fn description(node: &EditorNode) -> Option<&str> {
    node.description.as_deref().filter(|d| !d.trim().is_empty())
}

impl GraphEditor {
    /// The whole of a comment or a note, and the part of it that grabs it (graph space): for a
    /// comment, its text above the box and the box; the text grabs it. `None` for other nodes.
    /// `wrapped_height` measures text wrapped at a width, at zoom 1.
    pub(super) fn annotation_rects(
        &self,
        node: &EditorNode,
        wrapped_height: &dyn Fn(&str, FontId, f32) -> f32,
    ) -> Option<(Rect, Rect)> {
        match layer_of(node) {
            Layer::Comment => {
                let size = node.size.unwrap_or(COMMENT_SIZE).max(COMMENT_MIN);
                let text = description(node).map_or(0.0, |d| {
                    wrapped_height(d, FontId::proportional(TEXT_FONT), size.x - TEXT_INSET)
                        + TEXT_GAP
                });
                let band = COMMENT_TITLE_ROW + text;
                let top = node.pos - vec2(0.0, band);
                let rect = Rect::from_min_max(top, node.pos + size);
                Some((rect, Rect::from_min_size(top, vec2(size.x, band))))
            }
            Layer::Note => {
                let text = description(node).unwrap_or_else(|| tr("editor.note.empty"));
                let height = wrapped_height(text, FontId::proportional(TEXT_FONT), NOTE_TEXT_WIDTH)
                    + 2.0 * PAD;
                let rect = Rect::from_min_size(node.pos, vec2(NOTE_WIDTH, height.max(36.0)));
                Some((rect, rect))
            }
            Layer::Node => None,
        }
    }

    /// The height of a node's description under it (graph space), or 0 without one.
    pub(super) fn description_height(
        &self,
        node: &EditorNode,
        width: f32,
        wrapped_height: &dyn Fn(&str, FontId, f32) -> f32,
    ) -> f32 {
        match (layer_of(node), description(node)) {
            (Layer::Node, Some(text)) => {
                wrapped_height(
                    text,
                    FontId::proportional(DESCRIPTION_FONT),
                    description_width(width),
                ) + 6.0
            }
            _ => 0.0,
        }
    }

    /// The nodes that move with `moving`: those inside a moving comment (and inside comments
    /// inside it), and the notes attached to anything that moves. `moving` itself is left out.
    pub(super) fn carried(
        &self,
        geometry: &[Geometry],
        moving: &BTreeSet<NodeKey>,
    ) -> BTreeSet<NodeKey> {
        let rect_of = |key: NodeKey| geometry.iter().find(|g| g.key == key).map(|g| g.rect);
        let mut all = moving.clone();
        let mut queue: Vec<NodeKey> = moving.iter().copied().collect();
        while let Some(key) = queue.pop() {
            let comment = self
                .node(key)
                .is_some_and(|n| layer_of(n) == Layer::Comment);
            let inside = rect_of(key).filter(|_| comment);
            for g in geometry {
                let contained = inside.is_some_and(|r| g.key != key && r.contains_rect(g.rect));
                let attached = self.node(g.key).and_then(|n| n.attached) == Some(key);
                if (contained || attached) && all.insert(g.key) {
                    queue.push(g.key);
                }
            }
        }
        all.retain(|k| !moving.contains(k));
        all
    }

    /// The comment whose resize corner is under `p` (screen space), topmost first.
    pub(super) fn resize_handle_at(
        &self,
        geometry: &[Geometry],
        to_screen: impl Fn(Pos2) -> Pos2,
        p: Pos2,
    ) -> Option<NodeKey> {
        geometry
            .iter()
            .rev()
            .filter(|g| g.layer == Layer::Comment)
            .find(|g| handle_rect(to_screen(g.rect.max)).contains(p))
            .map(|g| g.key)
    }

    /// Grows or shrinks comment `key` by `delta` (graph space), down to the smallest box.
    pub(super) fn resize_comment(&mut self, key: NodeKey, delta: Vec2) {
        if let Some(node) = self.node_mut(key) {
            let size = node.size.unwrap_or(COMMENT_SIZE) + delta;
            node.size = Some(size.max(COMMENT_MIN));
        }
    }

    /// Adds a note attached to `target`, above it, and selects it. Returns its key.
    pub fn add_note_to(&mut self, target: NodeKey) -> Option<NodeKey> {
        // Above what is drawn of the target: a comment's text sits above its position.
        let top = self
            .last_geometry
            .iter()
            .find(|g| g.key == target)
            .map_or(self.node(target)?.pos, |g| g.rect.min);
        let at = top - vec2(0.0, 90.0);
        let key = self.add_node(NOTE, at)?;
        self.node_mut(key)?.attached = Some(target);
        self.selected = BTreeSet::from([key]);
        self.active = Some(key);
        Some(key)
    }

    /// Adds a comment whose box goes around `keys`, and selects it. Returns its key.
    pub fn add_comment_around(&mut self, keys: &BTreeSet<NodeKey>) -> Option<NodeKey> {
        let bounds = self
            .last_geometry
            .iter()
            .filter(|g| keys.contains(&g.key))
            .map(|g| g.rect)
            .reduce(|a, b| a.union(b))?;
        let margin = vec2(30.0, 30.0);
        let min = bounds.min - margin;
        let size = bounds.max + margin - min;
        let key = self.add_node(COMMENT, min)?;
        self.node_mut(key)?.size = Some(size.max(COMMENT_MIN));
        self.selected = BTreeSet::from([key]);
        self.active = Some(key);
        Some(key)
    }

    /// Whether `key` is a comment or a note, which only explain the graph.
    pub fn is_annotation(&self, key: NodeKey) -> bool {
        self.node(key).is_some_and(|n| layer_of(n) != Layer::Node)
    }

    /// The node a note attached to, if `key` is an attached note.
    pub fn attached_to(&self, key: NodeKey) -> Option<NodeKey> {
        self.node(key)?.attached
    }

    /// Attaches note `key` to `target` (any node: a comment or another note too), or loosens it
    /// with `None`. A note can't point at itself, and only notes attach.
    pub fn attach(&mut self, key: NodeKey, target: Option<NodeKey>) {
        let target = target.filter(|&t| t != key && self.node(t).is_some());
        if let Some(node) = self.node_mut(key)
            && layer_of(node) == Layer::Note
        {
            node.attached = target;
        }
    }

    /// The note whose attach tab is under `p` (screen space), topmost first.
    pub(super) fn attach_tab_at(
        &self,
        geometry: &[Geometry],
        to_screen: impl Fn(Pos2) -> Pos2,
        p: Pos2,
    ) -> Option<NodeKey> {
        let zoom = self.view.zoom;
        geometry
            .iter()
            .rev()
            .filter(|g| g.layer == Layer::Note)
            .find(|g| {
                let rect = Rect::from_min_max(to_screen(g.rect.min), to_screen(g.rect.max));
                attach_tab(rect, zoom).expand(3.0).contains(p)
            })
            .map(|g| g.key)
    }

    /// Where note `key`'s attach tab was last drawn, in screen space.
    pub fn attach_tab_rect(&self, key: NodeKey) -> Option<Rect> {
        Some(attach_tab(self.node_screen_rect(key)?, self.view.zoom))
    }

    /// What dropping note `note`'s tab at `p` (graph space) attaches it to: the topmost node
    /// whose drawing holds `p`, the whole of a comment included, other than the note itself.
    pub(super) fn attach_target_at(
        &self,
        geometry: &[Geometry],
        note: NodeKey,
        p: Pos2,
    ) -> Option<NodeKey> {
        geometry
            .iter()
            .rev()
            .find(|g| g.key != note && g.rect.contains(p))
            .map(|g| g.key)
    }

    /// Draws the line from note `note`'s tab to the pointer while the tab is dragged, and
    /// outlines `target`, what a drop there would attach the note to.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_attach_drag(
        &self,
        painter: &egui::Painter,
        theme: &Theme,
        geometry: &[Geometry],
        note: NodeKey,
        pointer: Pos2,
        target: Option<NodeKey>,
        to_screen: impl Fn(Pos2) -> Pos2,
    ) {
        let Some(g) = geometry.iter().find(|g| g.key == note) else {
            return;
        };
        let rect = Rect::from_min_max(to_screen(g.rect.min), to_screen(g.rect.max));
        let start = attach_tab(rect, self.view.zoom).center();
        if let Some(t) = target.and_then(|t| geometry.iter().find(|o| o.key == t)) {
            let r = Rect::from_min_max(to_screen(t.rect.min), to_screen(t.rect.max));
            painter.rect_stroke(
                r.expand(3.0),
                CornerRadius::same(6),
                Stroke::new(2.0, theme.accent),
                StrokeKind::Outside,
            );
        }
        let stroke = Stroke::new(2.0, theme.accent);
        painter.add(egui::Shape::dashed_line(
            &[start, pointer],
            stroke,
            6.0,
            4.0,
        ));
        painter.circle_filled(pointer, 3.5, theme.accent);
    }

    /// Draws a comment: a tinted box, its title and description in the band along the top, and
    /// the resize corner.
    pub(super) fn draw_comment(
        &self,
        painter: &egui::Painter,
        theme: &Theme,
        g: &Geometry,
        to_screen: impl Fn(Pos2) -> Pos2,
    ) {
        let Some(node) = self.node(g.key) else {
            return;
        };
        let zoom = self.view.zoom;
        let band = Rect::from_min_max(to_screen(g.grab.min), to_screen(g.grab.max));
        let rect = Rect::from_min_max(pos2(band.left(), band.bottom()), to_screen(g.rect.max));
        let color = theme.category(g.category);
        let rounding = CornerRadius::same((8.0 * zoom).round().clamp(1.0, 10.0) as u8);
        painter.rect_filled(rect, rounding, color.gamma_multiply(0.10));
        let outline = self.outline(g.key, color.gamma_multiply(0.6), theme);
        painter.rect_stroke(rect, rounding, outline, StrokeKind::Inside);
        if zoom > 0.3 {
            // The title, then the description, above the box.
            let room = (band.width() - TEXT_INSET * zoom).max(0.0);
            let mut title = egui::text::LayoutJob::simple_singleline(
                g.title.clone(),
                semibold(COMMENT_TITLE_FONT * zoom),
                theme.surfaces.text,
            );
            title.wrap = egui::text::TextWrapping::truncate_at_width(room);
            let galley = painter.layout_job(title);
            let row = COMMENT_TITLE_ROW * zoom;
            painter.galley(
                band.min + vec2(TEXT_INSET * zoom, (row - galley.size().y) / 2.0),
                galley,
                theme.surfaces.text,
            );
            if let Some(text) = description(node) {
                let galley = painter.layout(
                    text.to_owned(),
                    FontId::proportional(TEXT_FONT * zoom),
                    theme.surfaces.text_soft,
                    room,
                );
                painter.galley(
                    band.min + vec2(TEXT_INSET * zoom, row),
                    galley,
                    theme.surfaces.text_soft,
                );
            }
        }
        // The resize corner: three short diagonal strokes.
        let corner = rect.max;
        for i in 1..=3 {
            let d = i as f32 * 3.5;
            painter.line_segment(
                [corner - vec2(d, 2.0), corner - vec2(2.0, d)],
                Stroke::new(1.0, color.gamma_multiply(0.8)),
            );
        }
    }

    /// Draws the tail of a note attached to a node, from the note to that node. Drawn before
    /// everything else, so it never covers a comment's text or a node.
    pub(super) fn draw_note_tail(
        &self,
        painter: &egui::Painter,
        theme: &Theme,
        g: &Geometry,
        geometry: &[Geometry],
        to_screen: impl Fn(Pos2) -> Pos2,
    ) {
        // While its tab is dragged, the dragged line shows where it will point instead.
        if self.interaction == (Interaction::Attach { note: g.key }) {
            return;
        }
        let Some(target) = self
            .node(g.key)
            .and_then(|n| n.attached)
            .and_then(|t| geometry.iter().find(|o| o.key == t))
        else {
            return;
        };
        let zoom = self.view.zoom;
        let rect = Rect::from_min_max(to_screen(g.rect.min), to_screen(g.rect.max));
        let target = Rect::from_min_max(to_screen(target.rect.min), to_screen(target.rect.max));
        if let Some([a, b, tip]) = tail(rect, target, TAIL_BASE * zoom.clamp(0.5, 1.5)) {
            // Filled, then its two sides drawn as lines: a stroked polygon this thin grows long
            // spikes at its tip.
            painter.add(egui::Shape::convex_polygon(
                vec![a, b, tip],
                theme.note_fill,
                Stroke::NONE,
            ));
            let side = Stroke::new(1.0, theme.note_border);
            painter.line_segment([a, tip], side);
            painter.line_segment([b, tip], side);
        }
    }

    /// Draws a note with its text, over the nodes.
    pub(super) fn draw_note(
        &self,
        painter: &egui::Painter,
        theme: &Theme,
        g: &Geometry,
        to_screen: impl Fn(Pos2) -> Pos2,
    ) {
        let Some(node) = self.node(g.key) else {
            return;
        };
        let zoom = self.view.zoom;
        let rect = Rect::from_min_max(to_screen(g.rect.min), to_screen(g.rect.max));
        let fill = theme.note_fill;
        let border = theme.note_border;
        let rounding = CornerRadius::same((4.0 * zoom).round().clamp(1.0, 6.0) as u8);
        painter.rect_filled(
            rect.translate(vec2(0.0, 2.0 * zoom)),
            rounding,
            theme.node_shadow,
        );
        let outline = self.outline(g.key, border, theme);
        painter.rect(rect, rounding, fill, outline, StrokeKind::Inside);
        // The attach tab in the top-right corner, with a dot in it: solid while the note is
        // attached.
        let tab = attach_tab(rect, zoom);
        painter.rect(
            tab,
            CornerRadius::same((3.0 * zoom).round().clamp(1.0, 4.0) as u8),
            border.gamma_multiply(0.18),
            Stroke::new(1.0, border.gamma_multiply(0.7)),
            StrokeKind::Inside,
        );
        let dot_color = if node.attached.is_some() {
            border
        } else {
            border.gamma_multiply(0.45)
        };
        painter.circle_filled(tab.center(), (tab.height() * 0.2).max(1.0), dot_color);
        if zoom > 0.3 {
            let (text, color) = match description(node) {
                Some(text) => (text, theme.note_text),
                None => (
                    tr("editor.note.empty"),
                    theme.note_text.gamma_multiply(0.55),
                ),
            };
            let galley = painter.layout(
                text.to_owned(),
                FontId::proportional(TEXT_FONT * zoom),
                color,
                NOTE_TEXT_WIDTH * zoom,
            );
            painter.galley(rect.min + Vec2::splat(PAD * zoom), galley, color);
        }
    }

    /// Draws a node's description under it, `offset` (screen pixels) below its box.
    pub(super) fn draw_description(
        &self,
        painter: &egui::Painter,
        theme: &Theme,
        g: &Geometry,
        to_screen: impl Fn(Pos2) -> Pos2,
        offset: f32,
    ) {
        let Some(text) = self.node(g.key).and_then(description) else {
            return;
        };
        let zoom = self.view.zoom;
        if zoom <= 0.45 {
            return;
        }
        let galley: std::sync::Arc<Galley> = painter.layout(
            text.to_owned(),
            FontId::proportional(DESCRIPTION_FONT * zoom),
            theme.surfaces.text_soft,
            description_width(g.rect.width()) * zoom,
        );
        let at = to_screen(g.rect.left_bottom()) + vec2(2.0 * zoom, 4.0 * zoom + offset);
        painter.galley(at, galley, theme.surfaces.text_soft);
    }

    /// The outline of an annotation: the accent when it is selected, else `color`.
    fn outline(&self, key: NodeKey, color: Color32, theme: &Theme) -> Stroke {
        if self.active == Some(key) {
            Stroke::new(2.0, theme.accent)
        } else if self.selected.contains(&key) {
            Stroke::new(1.5, theme.accent.gamma_multiply(0.7))
        } else {
            Stroke::new(1.0, color)
        }
    }
}

/// How wide a description wraps under a node `width` wide.
fn description_width(width: f32) -> f32 {
    width.max(DESCRIPTION_WIDTH)
}

/// The attach tab of a note drawn at `note` (screen space): a small square inside its top-right
/// corner, scaled with the note.
fn attach_tab(note: Rect, zoom: f32) -> Rect {
    let size = Vec2::splat(TAB * zoom);
    Rect::from_min_size(
        pos2(
            note.right() - TAB_INSET * zoom - size.x,
            note.top() + TAB_INSET * zoom,
        ),
        size,
    )
}

/// The resize corner of a comment whose bottom-right corner is at `corner` (screen space).
fn handle_rect(corner: Pos2) -> Rect {
    Rect::from_min_max(corner - Vec2::splat(HANDLE), corner + Vec2::splat(2.0))
}

/// A speech bubble's tail from `note` toward `target` (both screen space): two points on the
/// note's edge facing the target, `base` apart, and the tip on the facing edge of the target, as
/// level with the base as the two allow. The edge is the one across the wider gap between them,
/// so the tail stays short. `None` when they overlap.
fn tail(note: Rect, target: Rect, base: f32) -> Option<[Pos2; 3]> {
    if note.intersects(target) {
        return None;
    }
    // How far apart they are across and down (negative where they overlap that way).
    let gap_x = (target.left() - note.right()).max(note.left() - target.right());
    let gap_y = (target.top() - note.bottom()).max(note.top() - target.bottom());
    let half = base / 2.0;
    // The middle of where the two spans overlap or, when they don't, of the gap between them.
    let meet = |lo1: f32, hi1: f32, lo2: f32, hi2: f32| (lo1.max(lo2) + hi1.min(hi2)) / 2.0;
    let inset =
        |v: f32, lo: f32, hi: f32| v.clamp(lo + half + 4.0, (hi - half - 4.0).max(lo + half + 4.0));
    if gap_x >= gap_y {
        // Side by side: from the note's left or right edge to the target's facing edge.
        let (from, to) = if target.left() >= note.right() {
            (note.right(), target.left())
        } else {
            (note.left(), target.right())
        };
        let y = meet(note.top(), note.bottom(), target.top(), target.bottom());
        let base_y = inset(y, note.top(), note.bottom());
        let tip_y = y.clamp(target.top() + 4.0, target.bottom() - 4.0);
        Some([
            pos2(from, base_y - half),
            pos2(from, base_y + half),
            pos2(to, tip_y),
        ])
    } else {
        // One above the other: from the note's top or bottom edge.
        let (from, to) = if target.top() >= note.bottom() {
            (note.bottom(), target.top())
        } else {
            (note.top(), target.bottom())
        };
        let x = meet(note.left(), note.right(), target.left(), target.right());
        let base_x = inset(x, note.left(), note.right());
        let tip_x = x.clamp(target.left() + 4.0, target.right() - 4.0);
        Some([
            pos2(base_x - half, from),
            pos2(base_x + half, from),
            pos2(tip_x, to),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tail_leaves_the_edge_facing_the_target() {
        let note = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 60.0));
        // A node below the note: the tail leaves its bottom edge and ends on the node's top,
        // straight down where the two overlap across.
        let below = Rect::from_min_size(pos2(50.0, 200.0), vec2(120.0, 80.0));
        let [a, b, tip] = tail(note, below, 14.0).unwrap();
        assert_eq!((a.y, b.y), (60.0, 60.0));
        assert_eq!(tip, pos2(110.0, 200.0));
        assert_eq!((a.x + b.x) / 2.0, 110.0);
        // A node to the right and a little lower: the right edge, short and nearly level.
        let right = Rect::from_min_size(pos2(400.0, 30.0), vec2(120.0, 80.0));
        let [a, b, tip] = tail(note, right, 14.0).unwrap();
        assert_eq!((a.x, b.x), (200.0, 200.0));
        assert_eq!(tip.x, 400.0);
        assert!(
            (tip.y - (a.y + b.y) / 2.0).abs() < 15.0,
            "{a:?} {b:?} {tip:?}"
        );
        // Far to the left and slightly above: the left edge, not the top.
        let left = Rect::from_min_size(pos2(-400.0, -20.0), vec2(120.0, 60.0));
        let [a, _, tip] = tail(note, left, 14.0).unwrap();
        assert_eq!((a.x, tip.x), (0.0, -280.0));
        // Overlapping: no tail.
        assert!(tail(note, note.translate(vec2(10.0, 10.0)), 14.0).is_none());
    }
}
