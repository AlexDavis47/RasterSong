use crate::nodes::{Category, NodeKind, NodeSpec, SampleClock};
use crate::{Layout, LayoutContext, Node, OutputSpec, ParamSpec, Params, PrepareContext};
use crate::{ProcessContext, Range, Signal};

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

/// A signal locked to the project's beat grid: 0 to 1 values that restart every note length,
/// written as a time signature is: `numerator` notes of a `1/denominator` whole note, a quarter
/// note being one beat. 1/4 restarts on every beat, 4/4 every four beats, 16/4 every sixteen and
/// 1/16 four times a beat. Wire it into a parameter to time an effect to the music, for example a
/// decay pumping a distortion's drive on every beat. Unlike an oscillator it also offers pulses
/// and stairs.
///
/// The value depends only on the sample's position, so seeking is exact, and the tempo and beat
/// offset set on the timeline move the grid.
#[derive(Debug)]
pub struct Beat {
    numerator: f64,
    denominator: f64,
    shape: Shape,
    width: f64,
    steps: f64,
    /// Set in `prepare`.
    group: usize,
    /// Pixels per beat, and pixels from the start of the render to the first beat.
    beat_pixels: f64,
    origin: f64,
    clock: SampleClock,
}

params! { Beat {
    NUMERATOR: ParamSpec::number("numerator", 1.0,
        1.0,
        32.0)
    .integer()
    .limits(1.0, 1024.0),
    DENOMINATOR: ParamSpec::number("denominator", 4.0,
        1.0,
        32.0)
    .integer()
    .limits(1.0, 1024.0),
    SHAPE: ParamSpec::choice("shape", Shape::OPTIONS, "decay"),
    WIDTH: ParamSpec::number("width", 0.25,
        0.0,
        1.0)
    .shown_when("shape", &["pulse"]),
    STEPS: ParamSpec::number("steps", 4.0,
        1.0,
        32.0)
    .integer()
    .limits(1.0, 1024.0)
    .shown_when("shape", &["step"]),
} }

impl NodeKind for Beat {
    const KIND: &'static str = "beat";
    const SPEC: NodeSpec = NodeSpec::new(Category::Generator)
        .params(Self::PARAMS)
        .takes_layout()
        .inputs(&[])
        .outputs(&[OutputSpec::new("out")]);
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "shape": "phase" }"#,
        r#"{ "shape": "decay", "numerator": 1, "denominator": 8 }"#,
        r#"{ "shape": "pulse", "width": 0.1 }"#,
        r#"{ "shape": "step", "steps": 3, "numerator": 4, "denominator": 4 }"#,
    ];
    const BENCH: Option<&'static str> = Some("{}");

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            numerator: params.number_at(Self::NUMERATOR)?,
            denominator: params.number_at(Self::DENOMINATOR)?,
            shape: params.choice_as(Self::SHAPE)?,
            width: params.number_at(Self::WIDTH)?,
            steps: params.number_at(Self::STEPS)?.max(1.0),
            group: 1,
            beat_pixels: 1.0,
            origin: 0.0,
            clock: SampleClock::default(),
        })
    }
}

impl Beat {
    /// Beats in `numerator / denominator` of a whole note: a quarter note is a beat.
    fn beats(numerator: f64, denominator: f64) -> f64 {
        4.0 * numerator.max(1e-6) / denominator.max(1e-6)
    }

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
        self.beat_pixels = (ctx.samples_per_beat() / self.group as f64).max(1e-9);
        self.origin = ctx.beat_offset_samples() / self.group as f64;
    }

    fn process(&mut self, ctx: &ProcessContext, _inputs: &[&Signal], outputs: &mut [Signal]) {
        let data = &mut outputs[0].data;
        let start = self.clock.begin(ctx, data.len());
        let first_pixel = (start / self.group as u64) as f64;
        // Each value is read at the pixel's first sample. The shape is a function of position
        // alone, also with a moving length, so seeking stays exact; a changing length moves the
        // shape against the grid rather than restarting it.
        let numerator = ctx.value(Self::NUMERATOR, self.numerator);
        let denominator = ctx.value(Self::DENOMINATOR, self.denominator);
        let width = ctx.value(Self::WIDTH, self.width);
        let steps = ctx.value(Self::STEPS, self.steps);
        for (pixel, chunk) in data.chunks_mut(self.group).enumerate() {
            let i = pixel * self.group;
            let beats = (first_pixel + pixel as f64 - self.origin) / self.beat_pixels;
            let length = Self::beats(numerator.at64(i), denominator.at64(i));
            let phase = (beats / length).rem_euclid(1.0);
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
        let out = run(r#"{ "shape": "decay", "denominator": 8 }"#);
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
    fn four_quarters_are_four_beats_long() {
        let out = run(r#"{ "shape": "phase", "numerator": 4, "denominator": 4 }"#);
        assert_eq!(out[1], 1.0 / 32.0);
    }

    #[test]
    fn lengths_longer_than_a_bar_repeat_that_slowly() {
        // 16/4: sixteen beats, 128 samples.
        let out = run(r#"{ "shape": "phase", "numerator": 16, "denominator": 4 }"#);
        assert_eq!(out[1], 1.0 / 128.0);
    }

    #[test]
    fn a_sixteenth_is_a_quarter_of_a_beat() {
        // 1/16: two samples.
        let out = run(r#"{ "shape": "decay", "numerator": 1, "denominator": 16 }"#);
        assert_eq!(out, [1.0, 0.5, 1.0, 0.5, 1.0, 0.5, 1.0, 0.5]);
    }
}
