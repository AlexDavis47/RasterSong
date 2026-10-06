use crate::nodes::{Category, NodeKind, NodeSpec, SampleClock};
use crate::{Layout, LayoutContext, Node, OutputSpec, ParamSpec, Params, PrepareContext};
use crate::{ProcessContext, Range, Signal};

choice! {
    /// The span the shape repeats over.
    pub enum Period {
        Beat = "beat",
        Bar = "bar",
    }
}

choice! {
    /// What the node outputs over each period.
    pub enum Shape {
        /// Rises from 0 to 1 over the period.
        Phase = "phase",
        /// Falls from 1 to 0 over the period: a hit that fades out on each beat.
        Decay = "decay",
        /// 1 for the first `width` of the period, then 0.
        Pulse = "pulse",
        /// Climbs in `steps` equal stairs from 0 towards 1: which part of the period it is in.
        Step = "step",
    }
}

/// A signal locked to the project's beat grid: 0 to 1 values that restart on every beat or bar.
/// Wire it into a parameter to time an effect to the music, for example a decay pumping a
/// distortion's drive on every beat. Unlike an oscillator it also offers pulses and stairs.
///
/// The value depends only on the sample's position, so seeking is exact, and the tempo and beat
/// offset set on the timeline move the grid.
#[derive(Debug)]
pub struct Beat {
    period: Period,
    shape: Shape,
    /// Cycles of the shape per period.
    division: f64,
    width: f64,
    steps: f64,
    /// Set in `prepare`.
    group: usize,
    /// Pixels per period, and pixels from the start of the render to the first beat.
    period_pixels: f64,
    origin: f64,
    clock: SampleClock,
}

params! { Beat {
    PERIOD: ParamSpec::choice(
        "period",
        "Period",
        Period::OPTIONS,
        "beat",
        "beat restarts the shape on every beat, bar on every bar",
    ),
    DIVISION: ParamSpec::number(
        "division",
        "Division",
        1.0,
        1.0,
        16.0,
        "Cycles per period: 2 restarts twice as often (half beats). Use the bar period for slower",
    )
    .integer()
    .limits(1.0, 1000.0),
    SHAPE: ParamSpec::choice(
        "shape",
        "Shape",
        Shape::OPTIONS,
        "decay",
        "phase rises 0 to 1, decay falls 1 to 0, pulse is on for the width, step climbs in stairs",
    ),
    WIDTH: ParamSpec::number(
        "width",
        "Width",
        0.25,
        0.0,
        1.0,
        "For the pulse shape, the fraction of each cycle it stays on",
    ),
    STEPS: ParamSpec::number(
        "steps",
        "Steps",
        4.0,
        1.0,
        32.0,
        "For the step shape, how many stairs each cycle climbs",
    ).integer()
    .limits(1.0, 1024.0),
} }

impl NodeKind for Beat {
    const KIND: &'static str = "beat";
    const SPEC: NodeSpec = NodeSpec::new("Beat", Category::Generator)
        .describe(
            "A 0 to 1 signal locked to the project's beats or bars: phase, decay, pulse or steps",
        )
        .params(Self::PARAMS)
        .takes_layout()
        .inputs(&[])
        .outputs(&[OutputSpec::new("out", "The beat-locked signal")]);
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "shape": "phase" }"#,
        r#"{ "shape": "decay", "period": "bar", "division": 2 }"#,
        r#"{ "shape": "pulse", "width": 0.1 }"#,
        r#"{ "shape": "step", "steps": 3, "period": "bar" }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            period: params.choice_as(Self::PERIOD)?,
            shape: params.choice_as(Self::SHAPE)?,
            division: params.number_at(Self::DIVISION)?.max(1e-6),
            width: params.number_at(Self::WIDTH)?,
            steps: params.number_at(Self::STEPS)?.max(1.0),
            group: 1,
            period_pixels: 1.0,
            origin: 0.0,
            clock: SampleClock::default(),
        })
    }
}

impl Beat {
    /// The shape at `phase` in `0..1`, for a pulse of `width` or a stair of `steps`.
    fn at(&self, phase: f64, width: f64, steps: f64) -> f32 {
        let v = match self.shape {
            Shape::Phase => phase,
            Shape::Decay => 1.0 - phase,
            Shape::Pulse => f64::from(phase < width),
            Shape::Step => {
                let steps = steps.round().max(1.0);
                (phase * steps).floor() / steps
            }
        };
        v as f32
    }
}

impl Node for Beat {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        ctx.layout.output_layouts(ctx, Range::Unipolar)
    }

    fn prepare(&mut self, ctx: &PrepareContext) {
        self.group = ctx.main().samples_per_pixel.max(1) as usize;
        let period_samples = match self.period {
            Period::Beat => ctx.samples_per_beat(),
            Period::Bar => ctx.samples_per_bar(),
        };
        self.period_pixels = (period_samples / self.group as f64).max(1e-9);
        self.origin = ctx.beat_offset_samples() / self.group as f64;
    }

    fn process(&mut self, ctx: &ProcessContext, _inputs: &[&Signal], outputs: &mut [Signal]) {
        let data = &mut outputs[0].data;
        let start = self.clock.begin(ctx, data.len());
        let first_pixel = (start / self.group as u64) as f64;
        // Each value is read at the pixel's first sample. The shape is a function of position
        // alone, also with a moving division, so seeking stays exact; a changing division moves
        // the shape against the grid rather than restarting it.
        let division = ctx.value(Self::DIVISION, self.division);
        let width = ctx.value(Self::WIDTH, self.width);
        let steps = ctx.value(Self::STEPS, self.steps);
        for (pixel, chunk) in data.chunks_mut(self.group).enumerate() {
            let i = pixel * self.group;
            let periods = (first_pixel + pixel as f64 - self.origin) / self.period_pixels;
            let phase = (periods * division.at64(i).max(1e-6)).rem_euclid(1.0);
            chunk.fill(self.at(phase, width.at64(i), steps.at64(i)));
        }
    }

    fn reset(&mut self) {
        self.clock.reset();
    }
}

#[cfg(test)]
mod tests {
    use crate::Tempo;
    use crate::testing::{node_with_tempo, process_one};

    /// 8 samples per block at 8 samples a second with 60 bpm: a beat is 8 samples, a bar 32.
    fn run(params: &str) -> Vec<f32> {
        let tempo = Tempo {
            bpm: 60.0,
            beats_per_bar: 4,
            offset_secs: 0.0,
        };
        let mut node = node_with_tempo("beat", params, 8, 8.0, &[], tempo);
        process_one(node.as_mut(), &[vec![0.0; 8]])
    }

    #[test]
    fn phase_rises_over_each_beat() {
        let out = run(r#"{ "shape": "phase" }"#);
        assert_eq!(out, [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875]);
    }

    #[test]
    fn decay_starts_at_one_on_the_beat() {
        let out = run(r#"{ "shape": "decay", "division": 2 }"#);
        assert_eq!(out, [1.0, 0.75, 0.5, 0.25, 1.0, 0.75, 0.5, 0.25]);
    }

    #[test]
    fn pulse_is_on_for_the_width() {
        let out = run(r#"{ "shape": "pulse", "width": 0.25 }"#);
        assert_eq!(out, [1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn steps_climb_in_stairs() {
        let out = run(r#"{ "shape": "step", "steps": 4 }"#);
        assert_eq!(out, [0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75]);
    }

    #[test]
    fn a_bar_is_four_beats_long() {
        let out = run(r#"{ "shape": "phase", "period": "bar" }"#);
        assert_eq!(out[1], 1.0 / 32.0);
    }
}
