use crate::nodes::support::settle_frames;
use crate::nodes::{Category, GeneratorLayout, NodeKind, NodeSpec, SampleClock};
use crate::{Layout, LayoutContext, Node, OutputSpec, ParamSpec, Params, PrepareContext};
use crate::{ProcessContext, Range, Signal};

choice! {
    /// How the noise's energy is spread across frequencies.
    pub enum Color {
        /// Equal energy at every frequency: sharp grain.
        White = "white",
        /// Energy falls 3 dB per octave: soft, natural grain.
        Pink = "pink",
        /// Energy falls 6 dB per octave: slow, wandering drift.
        Brown = "brown",
        /// Energy rises 3 dB per octave: fine grain.
        Blue = "blue",
        /// Energy rises 6 dB per octave: the finest, most pixel-to-pixel grain.
        Violet = "violet",
    }
}

/// The `index`th white noise value for `seed`, uniform in `-1..1`. A counter-based hash
/// (SplitMix64), so any sample can be computed without computing those before it.
fn white(seed: u64, index: u64) -> f32 {
    let mut z = (seed << 32 ^ index).wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u32 << 23) as f32 - 1.0
}

/// Random values. As video it is grain; as audio it is hiss.
///
/// The noise is a function of the seed and the sample's position, so it is the same however the
/// stream is cut into blocks. White, blue and violet need no history, so seeking is exact; pink
/// and brown filter white noise and need a short warmup.
#[derive(Debug)]
pub struct Noise {
    color: Color,
    layout: GeneratorLayout,
    seed: u64,
    amplitude: f32,
    offset: f32,
    /// Pink noise filter state (Paul Kellet's economy filter) and the brown noise integrator.
    pink: [f32; 3],
    brown: f32,
    clock: SampleClock,
}

params! { Noise {
    COLOR: ParamSpec::choice(
        "color",
        "Color",
        Color::OPTIONS,
        "white",
        "How the noise is spread over frequencies: white is sharp grain, brown is slow drift, violet is the finest grain",
    ),
    LAYOUT: GeneratorLayout::PARAM,
    SEED: ParamSpec::number("seed", "Seed", 0.0, 0.0, 999.0, "Picks which noise; the same seed always gives the same noise")
        .fixed()
        .limits(0.0, 4_000_000_000.0),
    AMPLITUDE: ParamSpec::number(
        "amplitude",
        "Amplitude",
        0.5,
        0.0,
        1.0,
        "Scales the noise, which spans -1 to 1 before the offset is added",
    )
    .exposed()
    .limits(-10.0, 10.0),
    OFFSET: ParamSpec::number(
        "offset",
        "Offset",
        0.5,
        -1.0,
        1.0,
        "Added to the noise: 0.5 with amplitude 0.5 fills the video range 0 to 1, 0 suits audio",
    )
    .limits(-10.0, 10.0),
} }

impl NodeKind for Noise {
    const KIND: &'static str = "noise";
    const SPEC: NodeSpec = NodeSpec::new("Noise", Category::Generator)
        .describe("Random values in a chosen colour: grain in video, hiss in audio")
        .params(Self::PARAMS)
        .inputs(&[])
        .outputs(&[OutputSpec::new("out", "The noise")]);
    const TEST_CONFIGS: &'static [&'static str] = &[
        r#"{ "color": "pink", "seed": 3 }"#,
        r#"{ "color": "brown" }"#,
        r#"{ "color": "blue", "amplitude": 1, "offset": 0 }"#,
        r#"{ "color": "violet" }"#,
    ];
    const BENCH: Option<&'static str> = Some(r#"{ "color": "pink" }"#);

    fn new(params: &Params) -> Result<Self, String> {
        Ok(Self {
            color: params.choice_as(Self::COLOR)?,
            layout: params.choice_as(Self::LAYOUT)?,
            seed: params.number_at(Self::SEED)? as u64,
            amplitude: params.float_at(Self::AMPLITUDE)?,
            offset: params.float_at(Self::OFFSET)?,
            pink: [0.0; 3],
            brown: 0.0,
            clock: SampleClock::default(),
        })
    }
}

impl Noise {
    /// The next noise value in `-1..1` for sample `index`, advancing any filter state.
    fn next(&mut self, index: u64) -> f32 {
        let w = white(self.seed, index);
        match self.color {
            Color::White => w,
            Color::Pink => {
                let [b0, b1, b2] = &mut self.pink;
                *b0 = 0.99765 * *b0 + w * 0.099_046;
                *b1 = 0.963 * *b1 + w * 0.296_516_4;
                *b2 = 0.57 * *b2 + w * 1.052_691_3;
                ((*b0 + *b1 + *b2 + w * 0.1848) * 0.25).clamp(-1.0, 1.0)
            }
            Color::Brown => {
                self.brown = (self.brown + 0.02 * w) / 1.02;
                (self.brown * 3.5).clamp(-1.0, 1.0)
            }
            Color::Blue => (w - white(self.seed, index.wrapping_sub(1))) * 0.5,
            Color::Violet => {
                let (a, b) = (white(self.seed, index.wrapping_sub(1)), white(self.seed, index.wrapping_sub(2)));
                (w - 2.0 * a + b) * 0.25
            }
        }
    }
}

impl Node for Noise {
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String> {
        self.layout
            .output_layouts(ctx, Range::from_bounds(f64::from(self.offset - self.amplitude.abs()), f64::from(self.offset + self.amplitude.abs())))
    }

    fn process(&mut self, ctx: &ProcessContext, _inputs: &[&Signal], outputs: &mut [Signal]) {
        let data = &mut outputs[0].data;
        let start = self.clock.begin(ctx, data.len());
        let amplitude = ctx.value(Self::AMPLITUDE, f64::from(self.amplitude));
        let offset = ctx.value(Self::OFFSET, f64::from(self.offset));
        for (i, out) in data.iter_mut().enumerate() {
            let noise = self.next(start + i as u64);
            *out = noise * amplitude.at(i) + offset.at(i);
        }
    }

    fn reset(&mut self) {
        self.pink = [0.0; 3];
        self.brown = 0.0;
        self.clock.reset();
    }

    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 {
        match self.color {
            // The slowest filter pole (0.99765 for pink, 1/1.02 for brown) settles in ~2000 samples.
            Color::Pink => settle_frames(2000.0, ctx),
            Color::Brown => settle_frames(500.0, ctx),
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{node, process_one};

    fn noise(params: &str, len: usize) -> Vec<f32> {
        let mut node = node("noise", params, len, len as f64, &[]);
        process_one(node.as_mut(), &[vec![0.0; len]])
    }

    #[test]
    fn white_noise_fills_the_range_with_a_zero_mean() {
        let out = noise(r#"{ "amplitude": 1, "offset": 0 }"#, 20_000);
        let mean = out.iter().sum::<f32>() / out.len() as f32;
        assert!(mean.abs() < 0.03, "mean {mean}");
        assert!(out.iter().all(|&x| (-1.0..1.0).contains(&x)));
        assert!(out.iter().any(|&x| x > 0.95) && out.iter().any(|&x| x < -0.95));
    }

    #[test]
    fn the_seed_picks_the_noise() {
        let a = noise(r#"{ "seed": 1 }"#, 64);
        assert_eq!(a, noise(r#"{ "seed": 1 }"#, 64));
        assert_ne!(a, noise(r#"{ "seed": 2 }"#, 64));
    }

    /// Mean absolute change between neighbouring samples relative to the signal's own spread:
    /// small for slow noise, large for fast noise.
    fn roughness(color: &str) -> f32 {
        let out = noise(&format!(r#"{{ "color": "{color}", "amplitude": 1, "offset": 0 }}"#), 20_000);
        let rms = (out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32).sqrt();
        let step = out.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / out.len() as f32;
        step / rms
    }

    #[test]
    fn colours_run_from_slow_to_fast() {
        let order = ["brown", "pink", "white", "blue", "violet"].map(roughness);
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    }
}
