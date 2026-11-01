//! The graph's totals in a card on the canvas's corner: how big it is, how many samples it
//! processes a block (one frame), how long that takes and how fast, and how late and how slow to
//! settle the render is.

use eframe::egui::{self, Align2, FontId, Rect, Stroke, StrokeKind, Ui, pos2, vec2};
use rastersong_engine::RenderDelays;
use rastersong_lang::{tr, tr_args};

use super::GraphEditor;
use crate::theme::{self, Theme};

/// What the statistics need from outside the editor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatsContext {
    /// The project's frame rate, for the share of a frame's time the graph takes.
    pub frame_rate: f64,
    /// The render's latency and warmup, once it has compiled.
    pub delays: Option<RenderDelays>,
}

/// The graph's totals, as the card shows them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct GraphTotals {
    pub nodes: usize,
    pub connections: usize,
    /// Samples every node's outputs hold in a block, summed: what the graph produces per frame.
    pub samples_per_block: u64,
    /// The smoothed time the nodes took for the frame at the playhead, in microseconds; none
    /// before a frame has rendered.
    pub micros: Option<f32>,
}

impl GraphTotals {
    /// Samples processed a second, once there is a time to divide by.
    pub fn samples_per_second(&self) -> Option<f64> {
        let micros = self.micros.filter(|m| *m > 0.0)?;
        Some(self.samples_per_block as f64 / (f64::from(micros) / 1_000_000.0))
    }
}

/// Space inside the card.
const PAD: f32 = 10.0;
/// The card's width.
const WIDTH: f32 = 184.0;
/// Height of one line.
const LINE: f32 = 17.0;
/// Space between the card and the canvas's edges.
const INSET: f32 = 10.0;

impl GraphEditor {
    /// The graph's totals from the last compile and the smoothed node times.
    pub(super) fn graph_totals(&self) -> GraphTotals {
        let samples_per_block = self
            .compiled
            .iter()
            .flat_map(|s| &s.outputs)
            .map(|layout| layout.len() as u64)
            .sum();
        GraphTotals {
            nodes: self.nodes.len(),
            connections: self.wires.len(),
            samples_per_block,
            micros: (!self.costs.is_empty()).then(|| self.costs.values().sum()),
        }
    }

    /// Draws the card in the canvas's bottom-left corner, `bottom` above its edge (clear of the
    /// error bar).
    pub(super) fn draw_graph_stats(
        &self,
        ui: &Ui,
        canvas: Rect,
        bottom: f32,
        theme: &Theme,
        ctx: StatsContext,
    ) {
        let totals = self.graph_totals();
        let none = || tr("editor.stats.none").to_owned();
        let frames = |n: usize| tr_args("editor.stats.frames", &[("frames", &n.to_string())]);
        let render = totals.micros.map_or_else(none, |micros| {
            tr_args(
                "editor.stats.ms",
                &[("ms", &format!("{:.2}", micros / 1000.0))],
            )
        });
        let share = totals.micros.map_or_else(none, |micros| {
            let budget = 1e6 / ctx.frame_rate.max(1e-6);
            let percent = f64::from(micros) / budget * 100.0;
            tr_args("editor.stats.percent", &[("n", &format!("{percent:.0}"))])
        });
        let rows = [
            (tr("editor.stats.nodes"), totals.nodes.to_string()),
            (
                tr("editor.stats.connections"),
                totals.connections.to_string(),
            ),
            (
                tr("editor.stats.samples_block"),
                compact_count(totals.samples_per_block as f64),
            ),
            (tr("editor.stats.render"), render),
            (tr("editor.stats.share"), share),
            (
                tr("editor.stats.samples_second"),
                totals.samples_per_second().map_or_else(none, compact_count),
            ),
            (
                tr("editor.stats.latency"),
                ctx.delays.map_or_else(none, |d| frames(d.latency_frames)),
            ),
            (
                tr("editor.stats.warmup"),
                ctx.delays.map_or_else(none, |d| frames(d.warmup_frames)),
            ),
        ];
        let height = 2.0 * PAD + LINE * (rows.len() as f32 + 1.0);
        let card = Rect::from_min_size(
            pos2(
                canvas.left() + INSET,
                canvas.bottom() - bottom - INSET - height,
            ),
            vec2(WIDTH, height),
        );
        if card.top() < canvas.top() {
            return;
        }
        let painter = ui.painter_at(canvas);
        painter.rect(
            card,
            8,
            theme.surfaces.panel.gamma_multiply(0.94),
            Stroke::new(1.0, theme.surfaces.card_border),
            StrokeKind::Inside,
        );
        let left = card.left() + PAD;
        let right = card.right() - PAD;
        let mut y = card.top() + PAD;
        painter.text(
            pos2(left, y + LINE / 2.0),
            Align2::LEFT_CENTER,
            tr("editor.stats.title").to_uppercase(),
            theme::semibold(theme::SMALL_SIZE),
            theme.surfaces.text_faint,
        );
        for (label, value) in rows {
            y += LINE;
            painter.text(
                pos2(left, y + LINE / 2.0),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(12.0),
                theme.surfaces.text_soft,
            );
            painter.text(
                pos2(right, y + LINE / 2.0),
                Align2::RIGHT_CENTER,
                value,
                FontId::monospace(theme::SMALL_SIZE),
                theme.surfaces.text,
            );
        }
        // Named for screen readers (and tests).
        let response = ui.interact(card, ui.id().with("graph-stats"), egui::Sense::hover());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, tr("editor.stats.title"))
        });
        response.on_hover_text(tr("editor.stats.help"));
    }
}

/// `n` in at most four digits with a thousands prefix: "512", "48.0 k", "6.22 M".
pub(super) fn compact_count(n: f64) -> String {
    let (value, key) = if n < 1e3 {
        return format!("{n:.0}");
    } else if n < 1e6 {
        (n / 1e3, "editor.stats.kilo")
    } else if n < 1e9 {
        (n / 1e6, "editor.stats.mega")
    } else {
        (n / 1e9, "editor.stats.giga")
    };
    let digits = if value < 10.0 {
        2
    } else if value < 100.0 {
        1
    } else {
        0
    };
    tr_args(key, &[("n", &format!("{value:.digits$}"))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rastersong_engine::{GraphDesc, NodeCost, NodeStats};
    use std::sync::Arc;

    #[test]
    fn counts_are_compact() {
        assert_eq!(compact_count(512.0), "512");
        assert_eq!(compact_count(48_000.0), "48.0 k");
        assert_eq!(compact_count(6_220_800.0), "6.22 M");
        assert_eq!(compact_count(312_000_000.0), "312 M");
        assert_eq!(compact_count(1.5e9), "1.50 G");
    }

    #[test]
    fn totals_add_up_outputs_and_times() {
        let mut editor = GraphEditor::new(
            &GraphDesc::from_json(include_str!("../../../../examples/graphs/am_bands.json"))
                .unwrap(),
        );
        let totals = editor.graph_totals();
        assert_eq!(totals.micros, None);
        assert_eq!(totals.samples_per_second(), None);
        assert!(totals.nodes > 0 && totals.connections > 0);

        let layout = |samples_per_pixel| rastersong_engine::Layout {
            width: 100,
            height: 10,
            samples_per_pixel,
            tag: Default::default(),
        };
        editor.set_compiled(vec![
            NodeStats {
                node: "a".into(),
                latency_frames: 0.0,
                warmup_frames: 0,
                inputs: Vec::new(),
                outputs: vec![layout(3), layout(1)],
                diagnostics: Vec::new(),
            },
            NodeStats {
                node: "b".into(),
                latency_frames: 0.0,
                warmup_frames: 0,
                inputs: Vec::new(),
                outputs: vec![layout(2)],
                diagnostics: Vec::new(),
            },
        ]);
        let cost = |node: &str, micros| NodeCost {
            node: Arc::from(node),
            micros,
        };
        editor.smooth_costs(&[cost("a", 1500.0), cost("b", 500.0)]);
        let totals = editor.graph_totals();
        assert_eq!(totals.samples_per_block, 6000);
        assert_eq!(totals.micros, Some(2000.0));
        assert_eq!(totals.samples_per_second(), Some(3_000_000.0));
    }
}
