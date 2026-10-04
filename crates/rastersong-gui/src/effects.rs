//! Reusable painting effects for the app's own widgets: glow today, with ghost trails (motion blur
//! for moving controls) intended to sit alongside.
//!
//! Effects are plain data that expand into the layers a painter draws, so they work for anything
//! the app draws itself (wires, rectangles, circles) and can be tested without a UI.

use std::collections::VecDeque;

use eframe::egui::{Color32, CornerRadius, Id, Painter, Pos2, Rect, Ui};

/// How many stacked layers fake the falloff of a glow. Each layer is translucent, so the glow
/// is brightest in the middle and fades outward.
const GLOW_LAYERS: usize = 7;

/// A soft halo of `color` reaching `radius` pixels beyond whatever it surrounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glow {
    pub color: Color32,
    /// How far the halo reaches past the edge, in pixels.
    pub radius: f32,
    /// Brightness at the edge, `0..=1`.
    pub strength: f32,
}

impl Glow {
    pub fn new(color: Color32, radius: f32, strength: f32) -> Self {
        Self {
            color,
            radius: radius.max(0.0),
            strength: strength.clamp(0.0, 1.0),
        }
    }

    /// The layers to draw, outermost first, as `(distance past the edge, colour)`. Draw each at
    /// its distance, so the faint outer layers end up beneath the brighter inner ones.
    pub fn layers(&self) -> Vec<(f32, Color32)> {
        // Together the layers add up to about `strength` where they all overlap.
        let alpha = self.strength * 1.6 / GLOW_LAYERS as f32;
        (0..GLOW_LAYERS)
            .map(|k| {
                let reach = self.radius * (1.0 - k as f32 / GLOW_LAYERS as f32);
                (reach, self.color.gamma_multiply(alpha))
            })
            .collect()
    }

    /// Paints a stroke of `core_width` through `draw(width, colour)`, which strokes the shape at
    /// that width. Only the halo is painted; draw the crisp line on top.
    pub fn stroke(&self, core_width: f32, mut draw: impl FnMut(f32, Color32)) {
        for (reach, color) in self.layers() {
            draw(core_width + 2.0 * reach, color);
        }
    }

    /// Paints the halo around a rectangle.
    pub fn rect(&self, painter: &Painter, rect: Rect, rounding: CornerRadius) {
        for (reach, color) in self.layers() {
            let spread = reach.round().min(f32::from(u8::MAX)) as u8;
            let rounding = CornerRadius {
                nw: rounding.nw.saturating_add(spread),
                ne: rounding.ne.saturating_add(spread),
                sw: rounding.sw.saturating_add(spread),
                se: rounding.se.saturating_add(spread),
            };
            painter.rect_filled(rect.expand(reach), rounding, color);
        }
    }

    /// Paints the halo around a circle.
    pub fn circle(&self, painter: &Painter, center: Pos2, radius: f32) {
        for (reach, color) in self.layers() {
            painter.circle_filled(center, radius + reach, color);
        }
    }
}

/// How long a ghost trail lingers, in seconds.
const TRAIL_SECS: f64 = 0.3;

/// Most interpolated ghosts drawn for one jump.
const MAX_SMEAR: usize = 48;

/// Recent values of something that moves, for drawing fading copies behind it (motion blur).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trail {
    samples: VecDeque<(f64, f64)>,
}

impl Trail {
    /// Records `value` at `time`, dropping samples that have faded out. A value equal to the
    /// last one adds nothing, so a resting control leaves no trail.
    pub fn push(&mut self, time: f64, value: f64) {
        while self
            .samples
            .front()
            .is_some_and(|s| time - s.0 > TRAIL_SECS)
        {
            self.samples.pop_front();
        }
        if self.samples.back().is_none_or(|s| s.1 != value) {
            self.samples.push_back((time, value));
        }
    }

    /// The ghosts to draw at `time`, as `(value, visibility)` with visibility `0..1` (oldest
    /// faintest). Between two recorded values the ghosts are interpolated, one every `gap` of
    /// value, so a value that jumped between frames is drawn as a continuous smear, as if
    /// sampled many times within the frame. The newest value itself is not included.
    pub fn ghosts(&self, time: f64, gap: f64) -> Vec<(f64, f32)> {
        let visibility = |t: f64| (1.0 - (time - t) / TRAIL_SECS).clamp(0.0, 1.0) as f32;
        let gap = gap.max(1e-9);
        let mut out = Vec::new();
        let pairs = self.samples.iter().zip(self.samples.iter().skip(1));
        // The segment into the newest sample is the freshest, so it's visible at full strength
        // in step with its start; older segments fade with their times.
        for (&(t0, v0), &(t1, v1)) in pairs {
            let steps = (((v1 - v0).abs() / gap).ceil() as usize).clamp(1, MAX_SMEAR);
            for k in 0..steps {
                let f = k as f64 / steps as f64;
                let seen = visibility(t0 + (t1 - t0) * f);
                if seen > 0.0 {
                    out.push((v0 + (v1 - v0) * f, seen));
                }
            }
        }
        out
    }

    /// Whether there's anything left to fade.
    pub fn is_moving(&self) -> bool {
        self.samples.len() > 1
    }
}

/// Keeps a [`Trail`] for `id` in the UI's memory, records `value`, and returns the ghosts to
/// draw, one per `gap` of value along the way. Asks for repaints while the trail fades.
pub fn trail(ui: &Ui, id: Id, value: f64, gap: f64) -> Vec<(f64, f32)> {
    let time = ui.input(|i| i.time);
    let mut trail: Trail = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    trail.push(time, value);
    let ghosts = trail.ghosts(time, gap);
    if trail.is_moving() {
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| d.insert_temp(id, trail));
    ghosts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trail_fades_and_a_resting_value_leaves_none() {
        let mut trail = Trail::default();
        trail.push(0.0, 0.0);
        trail.push(0.1, 1.0);
        trail.push(0.2, 2.0);
        let ghosts = trail.ghosts(0.2, 1.0);
        assert_eq!(ghosts.len(), 2);
        assert!(ghosts[0].1 < ghosts[1].1, "older is fainter");
        trail.push(1.0, 2.0);
        assert!(!trail.is_moving());
        assert!(trail.ghosts(1.0, 1.0).is_empty());
    }

    #[test]
    fn a_jump_is_smeared_between_its_ends() {
        let mut trail = Trail::default();
        trail.push(0.0, 0.0);
        trail.push(0.05, 1.0);
        let ghosts = trail.ghosts(0.05, 0.1);
        assert_eq!(ghosts.len(), 10, "one ghost per tenth of the jump");
        assert!(
            ghosts.windows(2).all(|g| g[1].0 > g[0].0),
            "evenly along it"
        );
        assert!(ghosts.iter().all(|g| (0.0..1.0).contains(&g.0)));
        // Never more than the cap, however far it jumps.
        let mut far = Trail::default();
        far.push(0.0, 0.0);
        far.push(0.01, 1e6);
        assert_eq!(far.ghosts(0.01, 1.0).len(), MAX_SMEAR);
    }

    #[test]
    fn glow_layers_run_from_the_widest_and_faintest_inward() {
        let glow = Glow::new(Color32::WHITE, 6.0, 0.8);
        let layers = glow.layers();
        assert_eq!(layers.len(), GLOW_LAYERS);
        assert_eq!(layers[0].0, 6.0, "reaches the full radius");
        assert!(layers.windows(2).all(|pair| pair[0].0 > pair[1].0));
        assert!(layers.last().unwrap().0 > 0.0, "never inside the edge");
    }

    #[test]
    fn a_glow_stroke_is_wider_than_its_core_by_the_radius_each_side() {
        let glow = Glow::new(Color32::RED, 4.0, 1.0);
        let mut widths = Vec::new();
        glow.stroke(2.0, |width, _| widths.push(width));
        assert_eq!(widths[0], 10.0);
        assert!(widths.iter().all(|&w| w > 2.0));
    }

    #[test]
    fn no_strength_means_no_light() {
        let glow = Glow::new(Color32::RED, 4.0, 0.0);
        assert!(glow.layers().iter().all(|(_, c)| c.a() == 0));
    }
}
