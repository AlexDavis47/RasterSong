//! The drag bubble: a rounded pill with an icon and a label that follows the pointer while
//! something is dragged. It pops in with a short "bloop" when the drag starts and again where it
//! is dropped, and shrinks back to where it came from when nothing takes it.
//!
//! A drag source calls [`drag_bubble`] every frame its drag lasts; the app calls [`paint_bubble`]
//! once at the end of each frame, after every drop target has had its chance at the payload. A
//! drop target takes the payload (`DragAndDrop::take_payload`), so a payload still there on the
//! frame the button is released means nothing took it.

use eframe::egui::{
    self, Align2, Context, CornerRadius, FontId, Id, LayerId, Order, Pos2, Rect, Stroke, vec2,
};

use crate::theme::Theme;

/// How long the bloop on pick-up and on drop lasts, in seconds.
pub const BLOOP_SECONDS: f64 = 0.22;
/// How long the bubble takes to shrink back to its source after a cancelled drag.
pub const RETURN_SECONDS: f64 = 0.25;

/// Where a bubble is in its life.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// Following the pointer; `since` is when the drag started.
    Dragging { since: f64 },
    /// Something took the drop at `at`; the bubble blooms and fades there.
    Dropped { at: Pos2, since: f64 },
    /// Nothing took it: the bubble travels from `at` back to its source and shrinks away.
    Cancelled { at: Pos2, since: f64 },
}

#[derive(Debug, Clone)]
struct Bubble {
    icon: String,
    label: String,
    source: Rect,
    phase: Phase,
    /// The pointer when last seen, where a drop or a cancel starts from.
    pointer: Pos2,
    /// The frame `drag_bubble` was last called on.
    seen: u64,
}

fn key() -> Id {
    Id::new("drag-bubble")
}

/// Shows the bubble for a drag from `source` while it lasts: call it every frame the drag
/// lasts, from the drag source.
pub fn drag_bubble(ctx: &Context, source: Rect, icon: &str, label: &str) {
    let (now, pointer) = ctx.input(|i| (i.time, i.pointer.hover_pos()));
    let frame = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let bubble = d.get_temp_mut_or_insert_with(key(), || Bubble {
            icon: String::new(),
            label: String::new(),
            source,
            phase: Phase::Dragging { since: now },
            pointer: source.center(),
            seen: frame,
        });
        if !matches!(bubble.phase, Phase::Dragging { .. }) {
            bubble.phase = Phase::Dragging { since: now };
        }
        icon.clone_into(&mut bubble.icon);
        label.clone_into(&mut bubble.label);
        bubble.source = source;
        bubble.seen = frame;
        if let Some(p) = pointer {
            bubble.pointer = p;
        }
    });
}

/// The bubble's phase, if one is showing.
pub fn bubble_phase(ctx: &Context) -> Option<Phase> {
    ctx.data(|d| d.get_temp::<Bubble>(key())).map(|b| b.phase)
}

/// Settles what happened to a drag that ended this frame and paints the bubble. Call once per
/// frame, after every drop target.
pub fn paint_bubble(ctx: &Context) {
    let Some(mut bubble) = ctx.data(|d| d.get_temp::<Bubble>(key())) else {
        return;
    };
    let now = ctx.input(|i| i.time);
    if let Phase::Dragging { .. } = bubble.phase
        && bubble.seen != ctx.cumulative_pass_nr()
    {
        // The source stopped calling: the drag ended. A payload nobody took means no target
        // wanted it.
        let at = bubble.pointer;
        bubble.phase = if egui::DragAndDrop::has_any_payload(ctx) {
            Phase::Cancelled { at, since: now }
        } else {
            Phase::Dropped { at, since: now }
        };
    }
    let Some((center, scale, opacity)) = placement(&bubble, now) else {
        ctx.data_mut(|d| d.remove::<Bubble>(key()));
        return;
    };
    ctx.data_mut(|d| d.insert_temp(key(), bubble.clone()));
    ctx.request_repaint();
    if scale <= 0.01 || opacity <= 0.01 {
        return;
    }
    let theme = Theme::of(ctx);
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, key()));
    let font = FontId::proportional(13.0 * scale);
    let text = format!("{}  {}", bubble.icon, bubble.label);
    let galley = painter.layout_no_wrap(text, font, theme.bubble_text.gamma_multiply(opacity));
    let size = galley.size() + vec2(20.0, 10.0) * scale;
    let rect = Rect::from_center_size(center, size);
    painter.rect(
        rect,
        CornerRadius::same((size.y / 2.0) as u8),
        theme.bubble.gamma_multiply(opacity),
        Stroke::NONE,
        egui::StrokeKind::Inside,
    );
    let at = Align2::CENTER_CENTER.anchor_size(center, galley.size()).min;
    painter.galley(at, galley, theme.bubble_text);
}

/// Where the bubble is drawn now, how large (1 is its natural size) and how opaque; `None`
/// once it is gone.
fn placement(bubble: &Bubble, now: f64) -> Option<(Pos2, f32, f32)> {
    // Drawn a little below and right of the pointer, so it doesn't hide what is under it.
    let offset = vec2(18.0, 14.0);
    match bubble.phase {
        Phase::Dragging { since } => Some((bubble.pointer + offset, bloop(now - since), 1.0)),
        Phase::Dropped { at, since } => {
            let t = (now - since) / BLOOP_SECONDS;
            (t < 1.0).then(|| (at + offset, drop_scale(t), 1.0 - t as f32))
        }
        Phase::Cancelled { at, since } => {
            let t = (now - since) / RETURN_SECONDS;
            (t < 1.0).then(|| {
                let eased = ease_out(t) as f32;
                let from = at + offset;
                let center = from + (bubble.source.center() - from) * eased;
                (center, 1.0 - 0.7 * eased, 1.0 - eased)
            })
        }
    }
}

/// The scale while a bubble pops in, `elapsed` seconds after the drag started: from half size,
/// past full size, settling on 1.
pub fn bloop(elapsed: f64) -> f32 {
    let t = (elapsed / BLOOP_SECONDS).clamp(0.0, 1.0);
    // Overshoots to about 1.12 and comes back: a damped bounce.
    let overshoot = (std::f64::consts::PI * t).sin() * 0.25 * (1.0 - t);
    (0.5 + 0.5 * ease_out(t) + overshoot) as f32
}

/// The scale `t` (`0..1`) of the way through the bloop on drop: a quick swell, then shrinking
/// away.
pub fn drop_scale(t: f64) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let swell = (std::f64::consts::PI * t.min(0.4) / 0.4).sin() * 0.18;
    ((1.0 - t * t) + swell) as f32
}

fn ease_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pick_up_bloop_overshoots_then_settles() {
        assert!((bloop(0.0) - 0.5).abs() < 1e-6);
        let peak = (1..20)
            .map(|k| bloop(BLOOP_SECONDS * f64::from(k) / 20.0))
            .fold(0.0, f32::max);
        assert!(peak > 1.05, "it overshoots: {peak}");
        assert!((bloop(BLOOP_SECONDS) - 1.0).abs() < 1e-6);
        assert!((bloop(10.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_drop_bloop_swells_then_vanishes() {
        assert!((drop_scale(0.0) - 1.0).abs() < 1e-6);
        assert!(drop_scale(0.2) > 1.1);
        assert!(drop_scale(1.0) < 0.01);
    }

    #[test]
    fn a_cancelled_bubble_ends_at_its_source() {
        let source = Rect::from_center_size(Pos2::new(10.0, 10.0), vec2(40.0, 40.0));
        let bubble = Bubble {
            icon: String::new(),
            label: String::new(),
            source,
            phase: Phase::Cancelled {
                at: Pos2::new(300.0, 200.0),
                since: 0.0,
            },
            pointer: Pos2::new(300.0, 200.0),
            seen: 0,
        };
        let (start, ..) = placement(&bubble, 0.0).unwrap();
        let (near_end, scale, opacity) = placement(&bubble, RETURN_SECONDS * 0.99).unwrap();
        assert!(start.distance(source.center()) > 200.0);
        assert!(near_end.distance(source.center()) < 2.0);
        assert!(scale < 0.4 && opacity < 0.05);
        assert!(placement(&bubble, RETURN_SECONDS).is_none());
    }
}
