use std::f64::consts::TAU;

use crate::nodes::{Category, FreqUnit, GeneratorLayout, NodeKind, NodeSpec, SampleClock};
use crate::{Layout, LayoutContext, Node, OutputSpec, ParamSpec, Params, PrepareContext};
use crate::{ProcessContext, Range, Signal};

choice! {
    /// The shape of one cycle.
    pub enum Wave {
        Sine = "sine",
        Triangle = "triangle",
        /// High for the pulse width, low for the rest.
        Square = "square",
        /// Falls from 1 to -1 each cycle.
        Saw = "saw",
        /// Rises from -1 to 1 each cycle.
        Ramp = "ramp",
    }
}

impl Wave {
    /// The wave at `phase` in `0..1` (a position within the cycle), from -1 to 1.
    pub fn at(self, phase: f64, width: f64) -> f64 {
        match self {
            Self::Sine => (phase * TAU).sin(),
            Self::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
            Self::Square => {
                if phase < width {
                    1.0
                } else {
                    -1.0
                }
            }
            Self::Saw => 1.0 - 2.0 * phase,
            Self::Ramp => 2.0 * phase - 1.0,
        }
    }
}

/// A periodic wave. As video it draws stripes across each row; as audio it is a tone.
///
/// Each pixel gets one value shared by its colour channels. An unmodulated oscillator computes its
/// phase from the sample's position, so seeking is exact. A modulated frequency has to accumulate
/// phase, so the oscillator's phase then depends on where the render started. In cycles per beat
/// or bar, an unmodulated wave starts its cycle on the beat grid of the project tempo.
#[derive(Debug)]
pub struct Oscillator {
    wave: Wave,
    layout: GeneratorLayout,
    freq: f64,
    unit: FreqUnit,
    phase: f64,
    amplitude: f32,
    offset: f32,
    width: f64,
    /// Set in `prepare`: samples per pixel and cycles per pixel for a frequency of one unit.
    group: usize,
    unit_step: f64,
    /// Pixels from the start of the render to the first beat, for beat and bar units, so the
    /// wave starts its cycle on the beat grid.
    origin: f64,
    /// Whether the frequency is modulated, and the accumulated phase when it is.
    modulated: bool,
    accumulated: f64,
    clock: SampleClock,
}

params! { Oscillator {
    WAVE: ParamSpec::choice("wave", "Wave", Wave::OPTIONS, "sine", "The shape of one cycle"),
    LAYOUT: GeneratorLayout::PARAM,
    FREQ: ParamSpec::number(
        "freq",
        "Frequency",
        8.0,
        0.01,
        100.0,
        "Cycles per unit of time or space: how many stripes fit in a row, or how high the tone is",
    )
    .exposed()
    .limits(0.0, 1_000_000.0),
    UNIT: FreqUnit::param("Row", "Unit for the frequency (cycles per unit): Row keeps the look at any resolution"),
    PHASE: ParamSpec::number(
        "phase",
        "Phase",
        0.0,
        0.0,
        1.0,
        "Where in the cycle the wave starts, as a fraction of a cycle",
    )
    .fixed()
    .limits(-1000.0, 1000.0),
    AMPLITUDE: ParamSpec::number(
        "amplitude",
        "Amplitude",
        0.5,
        0.0,
        1.0,
        "Half the peak-to-peak height; with the offset it places the wave in the signal's range",
    )
    .exposed()
    .limits(-10.0, 10.0),
    OFFSET: ParamSpec::number(
        "offset",
        "Offset",
        0.5,
        -1.0,
        1.0,
        "Added to the wave: 0.5 with amplitude 0.5 fills the video range 0 to 1, 0 suits audio",
    )
    .limits(-10.0, 10.0),
    PULSE_WIDTH: ParamSpec::number(
        "pulse_width",
        "Pulse width",
        0.5,
        0.0,
        1.0,
        "For the square wave, the fraction of the cycle it stays high",
    ),
} }

impl NodeKind for Oscillator {
    const KIND: &'static str = "oscillator";
    const SPEC: NodeSpec = NodeSpec::new("Oscillator", Category::Generator)
        .describe("A sine, triangle, square, saw or ramp wave: stripes in video, a tone in audio")
        .params(Self::PARAMS)
        .inputs(&[])
        .outputs(&[OutputSpec::new("out", "The wave")]);
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "wave": "triangle", "freq": 2.5 }"#,
        r#"{ "wave": "square", "pulse_width": 0.25, "phase": 0.3 }"#,
        r#"{ "wave": "saw", "freq": 1, "unit": "Frame" }"#,
        r#"{ "wave": "ramp", "freq": 3000, "unit": "Hertz", "amplitude": 1, "offset": 0 }"#,
        r#"{ "wave": "saw", "freq": 2, "unit": "Beat" }"#,
        r#"{ "wave": "square", "freq": 0.25, "unit": "Bar" }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "wave": "sine", "freq": 12 }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            wave: params.choice_as(Self::WAVE)?,
            layout: params.choice_as(Self::LAYOUT)?,
            freq: params.number_at(Self::FREQ)?,
            unit: params.choice_as(Self::UNIT)?,
            phase: params.number_at(Self::PHASE)?,
            amplitude: params.float_at(Self::AMPLITUDE)?,
            offset: params.float_at(Self::OFFSET)?,
            width: params.number_at(Self::PULSE_WIDTH)?,
            group: 1,
            unit_step: 0.0,
            origin: 0.0,
            modulated: false,
            accumulated: 0.0,
            clock: SampleClock::default(),
        })
    }
}

impl Node for Oscillator {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        self.layout.output_layouts(
            ctx,
            Range::from_bounds(
                f64::from(self.offset - self.amplitude.abs()),
                f64::from(self.offset + self.amplitude.abs()),
            ),
        )
    }

    fn prepare(&mut self, ctx: &PrepareContext) {
        self.group = ctx.main().samples_per_pixel.max(1) as usize;
        self.unit_step = self.unit.per_sample(1.0, ctx) * self.group as f64;
        self.origin = match self.unit {
            FreqUnit::Beat | FreqUnit::Bar => ctx.beat_offset_samples() / self.group as f64,
            _ => 0.0,
        };
        self.modulated = ctx.modulation(Self::FREQ).is_some();
        self.accumulated = self.phase;
    }

    fn process(&mut self, ctx: &ProcessContext, _inputs: &[&Signal], outputs: &mut [Signal]) {
        let data = &mut outputs[0].data;
        let start = self.clock.begin(ctx, data.len());
        let amplitude = ctx.value(Self::AMPLITUDE, f64::from(self.amplitude));
        let offset = ctx.value(Self::OFFSET, f64::from(self.offset));
        let (wave, width, group) = (self.wave, self.width, self.group);
        let emit = |phase: f64, i: usize| {
            let v = wave.at(phase.rem_euclid(1.0), width);
            (v * amplitude.at64(i) + offset.at64(i)) as f32
        };
        match ctx.param(Self::FREQ).filter(|_| self.modulated) {
            Some(freq) => {
                // Phase accumulates once per pixel, at the frequency in force at its first sample.
                for (pixel, chunk) in data.chunks_mut(group).enumerate() {
                    let i = pixel * group;
                    chunk.fill(emit(self.accumulated, i));
                    self.accumulated =
                        (self.accumulated + f64::from(freq[i]) * self.unit_step).rem_euclid(1.0);
                }
            }
            None => {
                let step = self.freq * self.unit_step;
                let first_pixel = start / group as u64;
                for (pixel, chunk) in data.chunks_mut(group).enumerate() {
                    let cycles = ((first_pixel + pixel as u64) as f64 - self.origin) * step;
                    chunk.fill(emit(self.phase + cycles.fract(), pixel * group));
                }
            }
        }
    }

    fn reset(&mut self) {
        self.accumulated = self.phase;
        self.clock.reset();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::testing::{node, node_with_tempo, process_one};
    use crate::{Layout, Node, ProcessContext, Signal, Tempo};

    /// One block of `len` samples that is one row, so cycles per row are cycles per block.
    fn wave(params: &str, len: usize) -> Vec<f32> {
        let mut node = node("oscillator", params, len, len as f64, &[]);
        process_one(node.as_mut(), &[vec![0.0; len]])
    }

    #[test]
    fn a_sine_makes_the_requested_cycles_per_row() {
        let out = wave(r#"{ "freq": 1, "amplitude": 1, "offset": 0 }"#, 8);
        assert!(out[0].abs() < 1e-6);
        assert!((out[2] - 1.0).abs() < 1e-6, "peak at a quarter cycle");
        assert!((out[6] + 1.0).abs() < 1e-6, "trough at three quarters");
    }

    #[test]
    fn defaults_fill_the_video_range() {
        let out = wave(r#"{ "wave": "ramp", "freq": 1 }"#, 4);
        assert_eq!(out, [0.0, 0.25, 0.5, 0.75]);
    }

    #[test]
    fn square_stays_high_for_the_pulse_width() {
        let out = wave(
            r#"{ "wave": "square", "freq": 1, "pulse_width": 0.25, "amplitude": 1, "offset": 0 }"#,
            8,
        );
        assert_eq!(out, [1.0, 1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0]);
    }

    #[test]
    fn triangle_and_saw_hit_their_corners() {
        let tri = wave(
            r#"{ "wave": "triangle", "freq": 1, "amplitude": 1, "offset": 0 }"#,
            4,
        );
        assert_eq!(tri, [-1.0, 0.0, 1.0, 0.0]);
        let saw = wave(
            r#"{ "wave": "saw", "freq": 1, "amplitude": 1, "offset": 0 }"#,
            4,
        );
        assert_eq!(saw, [1.0, 0.5, 0.0, -0.5]);
    }

    #[test]
    fn phase_shifts_the_wave() {
        let out = wave(r#"{ "wave": "ramp", "freq": 1, "phase": 0.25 }"#, 4);
        assert_eq!(out, [0.25, 0.5, 0.75, 0.0]);
    }

    /// One block of a ramp at one cycle per beat. Blocks are 1000 samples at 30 frames a second
    /// (30 kHz), so a beat at 120 bpm is 15000 samples: 15 blocks.
    fn beat_ramp(tempo: Tempo, frame: u64) -> Vec<f32> {
        let mut node = node_with_tempo(
            "oscillator",
            r#"{ "wave": "ramp", "freq": 1, "unit": "Beat", "amplitude": 0.5, "offset": 0.5 }"#,
            1000,
            30_000.0,
            &[],
            tempo,
        );
        let sources: HashMap<String, Signal> = HashMap::new();
        let mut out = [Signal::zeros(Layout::mono(1000, 1))];
        node.process(
            &ProcessContext {
                frame,
                frame_rate: 30.0,
                sources: &sources,
                params: &[],
            },
            &[],
            &mut out,
        );
        out[0].data.clone()
    }

    #[test]
    fn beat_units_start_the_cycle_on_the_beat() {
        for bpm in [90.0, 120.0, 133.0] {
            let tempo = Tempo {
                bpm,
                ..Tempo::default()
            };
            let beat = 30_000.0 * 60.0 / bpm;
            // Sample 0 is on the first beat: the ramp starts at its low point (0 after offset).
            let out = beat_ramp(tempo, 0);
            assert!(out[0].abs() < 1e-6, "{bpm} bpm");
            // Each sample advances 1/beat of a cycle.
            assert!(
                (f64::from(out[500]) - 500.0 / beat).abs() < 1e-4,
                "{bpm} bpm"
            );
        }
    }

    #[test]
    fn the_beat_offset_moves_the_grid() {
        // The first beat is 0.1 s (3000 samples) in, so sample 3000 is the start of a cycle and
        // sample 0 is 3000 samples into the previous one.
        let tempo = Tempo {
            offset_secs: 0.1,
            ..Tempo::default()
        };
        let beat = 15_000.0;
        let out = beat_ramp(tempo, 0);
        assert!((f64::from(out[0]) - (1.0 - 3000.0 / beat)).abs() < 1e-4);
        let out = beat_ramp(tempo, 3);
        assert!(out[0].abs() < 1e-4, "frame 3 starts at sample 3000");
    }

    #[test]
    fn a_seek_continues_where_a_render_from_the_start_would_be() {
        let mut node = node("oscillator", r#"{ "freq": 1.5 }"#, 8, 8.0, &[]);
        let sources: HashMap<String, Signal> = HashMap::new();
        let run = |node: &mut Box<dyn Node>, frame| {
            let mut out = [Signal::zeros(Layout::mono(8, 1))];
            let ctx = ProcessContext {
                frame,
                frame_rate: 1.0,
                sources: &sources,
                params: &[],
            };
            node.process(&ctx, &[], &mut out);
            out[0].data.clone()
        };
        let from_start: Vec<_> = (0..4).map(|frame| run(&mut node, frame)).collect();
        // Reset, then start at frame 2 as a seek would.
        node.reset();
        let seeked = run(&mut node, 2);
        assert_eq!(seeked, from_start[2]);
    }
}
