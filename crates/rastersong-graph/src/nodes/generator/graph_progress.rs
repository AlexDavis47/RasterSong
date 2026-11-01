use crate::nodes::support::SampleClock;
use crate::nodes::{Category, NodeKind, NodeSpec};
use crate::{Layout, LayoutContext, Node, OutputSpec, Params, PrepareContext};
use crate::{ProcessContext, Range, Signal};

/// How far through its graph item the render is: 0 at the item's left edge, rising evenly to 1
/// at its end. Stretching the item makes it rise more slowly; during pre-roll it is 0. A graph
/// rendered over the whole project goes from 0 to 1 over the project.
///
/// Like the Beat node, the value depends only on the sample's position, so seeking is exact.
#[derive(Debug)]
pub struct GraphProgress {
    /// Set in `prepare`: samples per value, and the item's start and length in pixels of the
    /// graph's own time.
    group: usize,
    start: f64,
    length: f64,
    clock: SampleClock,
}

params! { GraphProgress {} }

impl NodeKind for GraphProgress {
    const KIND: &'static str = "graph_progress";
    const SPEC: NodeSpec = NodeSpec::new(Category::Generator)
        .params(Self::PARAMS)
        .takes_layout()
        .inputs(&[])
        .outputs(&[OutputSpec::new("out")]);
    // No parameters: the defaults are the one setting.
    const TEST_CONFIGS: &'static [&'static str] = &["{}"];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(_params: &Params) -> Result<Self, String> {
        Ok(Self {
            group: 1,
            start: 0.0,
            length: f64::INFINITY,
            clock: SampleClock::default(),
        })
    }
}

impl Node for GraphProgress {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        ctx.layout.output_layouts(ctx, Range::Unipolar)
    }

    fn prepare(&mut self, ctx: &PrepareContext) {
        self.group = ctx.main().samples_per_pixel.max(1) as usize;
        let pixels_per_second = ctx.sample_rate() / self.group as f64;
        self.start = ctx.span.start * pixels_per_second;
        self.length = ctx.span.length * pixels_per_second;
    }

    fn process(&mut self, ctx: &ProcessContext, _inputs: &[&Signal], outputs: &mut [Signal]) {
        let data = &mut outputs[0].data;
        let start = self.clock.begin(ctx, data.len());
        let first_pixel = (start / self.group as u64) as f64;
        for (pixel, chunk) in data.chunks_mut(self.group).enumerate() {
            let progress = if self.length > 0.0 {
                ((first_pixel + pixel as f64 - self.start) / self.length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            chunk.fill(progress as f32);
        }
    }

    fn reset(&mut self) {
        self.clock.reset();
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node_with_span, process_one};
    use crate::{Span, Tempo};

    /// 8 samples per block at 8 samples a second: a block is a second.
    fn run(span: Span) -> Vec<f32> {
        let mut node = node_with_span("graph_progress", "{}", 8, 8.0, Tempo::default(), span);
        process_one(node.as_mut(), &[vec![0.0; 8]])
    }

    #[test]
    fn rises_from_zero_at_the_items_start_to_one_at_its_end() {
        let out = run(Span {
            start: 0.0,
            length: 2.0,
        });
        assert_eq!(
            out,
            [0.0, 0.0625, 0.125, 0.1875, 0.25, 0.3125, 0.375, 0.4375]
        );
    }

    #[test]
    fn is_zero_before_the_item_and_one_after() {
        // The item plays half a second from 0.25 s into the graph's own time.
        let out = run(Span {
            start: 0.25,
            length: 0.5,
        });
        assert_eq!(out, [0.0, 0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0]);
    }

    #[test]
    fn without_an_end_it_stays_at_zero() {
        assert!(run(Span::default()).iter().all(|&x| x == 0.0));
    }
}
