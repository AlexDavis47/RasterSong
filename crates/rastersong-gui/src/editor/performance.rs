//! Per-node processing time: smoothing, and the badge and tint that point out slow nodes.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, vec2};
use rastersong_engine::NodeCost;
use rastersong_lang::tr_args;

use super::GraphEditor;
use super::canvas::{Geometry, NODE_TEXT_LINE};
use crate::theme::Theme;

/// How much of each new frame's time goes into the smoothed value.
const SMOOTHING: f32 = 0.1;

impl GraphEditor {
    /// Folds the last frame's costs into the smoothed ones. Nodes that are gone are forgotten.
    pub(super) fn smooth_costs(&mut self, costs: &[NodeCost]) {
        self.costs
            .retain(|id, _| costs.iter().any(|c| *c.node == **id));
        for cost in costs {
            self.costs
                .entry(cost.node.to_string())
                .and_modify(|v| *v += (cost.micros - *v) * SMOOTHING)
                .or_insert(cost.micros);
        }
    }

    /// Tints a node's body by how slow it is against the slowest node, and writes its time and
    /// its share of the graph's total under it: at the right, or on the line under its latency
    /// and warmup when `below_stats`, so the two never overlap. Returns whether it wrote any.
    pub(super) fn draw_performance(
        &self,
        painter: &egui::Painter,
        theme: &Theme,
        g: &Geometry,
        to_screen: impl Fn(Pos2) -> Pos2,
        text_color: Color32,
        below_stats: bool,
    ) -> bool {
        let Some(node) = self.node(g.key) else {
            return false;
        };
        let Some(&micros) = self.costs.get(node.id.as_str()) else {
            return false;
        };
        let total: f32 = self.costs.values().sum();
        let slowest = self.costs.values().copied().fold(0.0, f32::max);
        let heat = if slowest > 0.0 { micros / slowest } else { 0.0 };
        let rect = Rect::from_min_max(to_screen(g.rect.min), to_screen(g.rect.max));
        let rounding =
            egui::CornerRadius::same((6.0 * self.view.zoom).round().clamp(1.0, 12.0) as u8);
        painter.rect_filled(
            rect,
            rounding,
            theme.warning.gamma_multiply(0.35 * heat * heat),
        );
        let zoom = self.view.zoom;
        if zoom <= 0.45 {
            return false;
        }
        let share = if total > 0.0 {
            micros / total * 100.0
        } else {
            0.0
        };
        let text = tr_args(
            "editor.node.cost",
            &[
                ("ms", &format!("{:.2}", micros / 1000.0)),
                ("share", &format!("{share:.0}")),
            ],
        );
        let (pos, align) = if below_stats {
            (
                rect.left_bottom() + vec2(5.0 * zoom, (5.0 + NODE_TEXT_LINE) * zoom),
                Align2::LEFT_TOP,
            )
        } else {
            (
                rect.right_bottom() + vec2(-5.0 * zoom, 5.0 * zoom),
                Align2::RIGHT_TOP,
            )
        };
        painter.text(
            pos,
            align,
            text,
            FontId::proportional(11.5 * zoom),
            text_color,
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn cost(node: &str, micros: f32) -> NodeCost {
        NodeCost {
            node: Arc::from(node),
            micros,
        }
    }

    #[test]
    fn costs_are_smoothed_and_forgotten_with_their_node() {
        let mut editor = GraphEditor::new(
            &rastersong_engine::GraphDesc::from_json(include_str!(
                "../../../../examples/graphs/am_bands.json"
            ))
            .unwrap(),
        );
        editor.smooth_costs(&[cost("a", 100.0), cost("b", 10.0)]);
        editor.smooth_costs(&[cost("a", 200.0)]);
        assert!((editor.costs["a"] - 110.0).abs() < 1e-3);
        assert!(!editor.costs.contains_key("b"));
    }
}
