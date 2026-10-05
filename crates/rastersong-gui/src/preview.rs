//! The preview pane's view state: pan and zoom, which video is shown, and the comparison split.
//! Pure logic, so it can be tested without a window.

use eframe::egui::{Pos2, Rect, Vec2};

/// Zoom limits, as multiples of the size that fits the pane.
pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 32.0;

/// One of the two parallel videos: the graph's result, or the original.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Feed {
    #[default]
    Processed,
    Unprocessed,
}

impl Feed {
    pub fn other(self) -> Self {
        match self {
            Self::Processed => Self::Unprocessed,
            Self::Unprocessed => Self::Processed,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Processed => "Processed",
            Self::Unprocessed => "Unprocessed",
        }
    }
}

/// The feeds on the (left, right) of the split when `showing` is the selected one: the selected
/// feed is on the right, the other on the left. Switching the selection flips the sides.
pub fn split_sides(showing: Feed) -> (Feed, Feed) {
    (showing.other(), showing)
}

/// Pan and zoom of the frame in the preview pane. At zoom 1 with no offset the frame is as large as
/// fits the pane, centred.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewView {
    pub zoom: f32,
    /// How far the frame's centre is from the pane's centre, in pixels.
    pub offset: Vec2,
}

impl Default for PreviewView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            offset: Vec2::ZERO,
        }
    }
}

impl PreviewView {
    /// Fits the frame in the pane again.
    pub fn fit(&mut self) {
        *self = Self::default();
    }

    /// Where a frame of `frame` pixels is drawn in `pane`.
    pub fn frame_rect(&self, pane: Rect, frame: Vec2) -> Rect {
        let fit = (pane.width() / frame.x).min(pane.height() / frame.y);
        Rect::from_center_size(pane.center() + self.offset, frame * fit * self.zoom)
    }

    /// Multiplies the zoom by `factor`, keeping the point under `pointer` where it is.
    pub fn zoom_at(&mut self, pane: Rect, pointer: Pos2, factor: f32) {
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let ratio = zoom / self.zoom;
        let centre = pane.center() + self.offset;
        let new_centre = pointer - (pointer - centre) * ratio;
        self.offset = new_centre - pane.center();
        self.zoom = zoom;
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.offset += delta;
    }
}

/// The split's position as a fraction of the pane's width, kept off the very edges so the handle
/// can always be grabbed.
pub fn clamp_split(position: f32) -> f32 {
    position.clamp(0.02, 0.98)
}

/// The pane's left and right halves around a split at `position` (0..1).
pub fn split_rects(pane: Rect, position: f32) -> (Rect, Rect) {
    let x = pane.left() + pane.width() * clamp_split(position);
    (
        Rect::from_min_max(pane.min, Pos2::new(x, pane.bottom())),
        Rect::from_min_max(Pos2::new(x, pane.top()), pane.max),
    )
}

#[cfg(test)]
mod tests {
    use eframe::egui::vec2;

    use super::*;

    fn pane() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 20.0), vec2(400.0, 200.0))
    }

    #[test]
    fn the_frame_fits_the_pane_at_zoom_one() {
        let view = PreviewView::default();
        let rect = view.frame_rect(pane(), vec2(160.0, 90.0));
        // Limited by height: 200 / 90 = 2.22.
        assert!((rect.height() - 200.0).abs() < 1e-3);
        assert!((rect.width() - 160.0 * 200.0 / 90.0).abs() < 1e-3);
        assert_eq!(rect.center(), pane().center());
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let mut view = PreviewView::default();
        let frame = vec2(160.0, 90.0);
        let pointer = Pos2::new(300.0, 60.0);
        let before = view.frame_rect(pane(), frame);
        // The point's position within the frame, 0..1.
        let uv = ((pointer - before.min) / before.size()).to_pos2();
        view.zoom_at(pane(), pointer, 3.0);
        let after = view.frame_rect(pane(), frame);
        let moved = after.min + uv.to_vec2() * after.size();
        assert!(
            (moved - pointer).length() < 1e-3,
            "{moved:?} vs {pointer:?}"
        );
        assert!((view.zoom - 3.0).abs() < 1e-6);
    }

    #[test]
    fn zoom_is_limited_and_fit_resets() {
        let mut view = PreviewView::default();
        view.zoom_at(pane(), pane().center(), 1e6);
        assert_eq!(view.zoom, MAX_ZOOM);
        view.zoom_at(pane(), pane().center(), 1e-9);
        assert_eq!(view.zoom, MIN_ZOOM);
        view.pan(vec2(5.0, 6.0));
        view.fit();
        assert_eq!(view, PreviewView::default());
    }

    #[test]
    fn the_selected_feed_is_on_the_right_and_switching_flips_the_sides() {
        assert_eq!(
            split_sides(Feed::Processed),
            (Feed::Unprocessed, Feed::Processed)
        );
        assert_eq!(
            split_sides(Feed::Unprocessed),
            (Feed::Processed, Feed::Unprocessed)
        );
    }

    #[test]
    fn the_split_divides_the_pane() {
        let (left, right) = split_rects(pane(), 0.25);
        assert_eq!(left.right(), right.left());
        assert!((left.width() - 100.0).abs() < 1e-3);
        assert_eq!(left.union(right), pane());
        // Dragged past the edge, the handle stays reachable.
        let (left, _) = split_rects(pane(), -3.0);
        assert!(left.width() > 0.0);
    }
}
