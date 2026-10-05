//! The graph's audio output on its way out: sanitized, resampled to the project's audio rate, and
//! cut into one block per video frame.
//!
//! The graph works in fixed blocks of one frame each, whose length depends on the signal (about
//! 1602 samples for 48 kHz audio at 29.97 fps, millions for a picture). The sink treats the blocks
//! as one continuous stream at `block length × frame rate` samples a second and resamples it to
//! a fixed rate. History carries across blocks, so block edges don't click.

/// The project's audio rate when nothing else is set.
pub const DEFAULT_AUDIO_RATE: u32 = 48_000;

/// What the audio of a render is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioSink {
    /// The graph has no audio output: the source audio is used as it is.
    Source,
    /// An audio track is wired straight to the audio output: that track, as it is.
    Passthrough(String),
    /// The graph's audio output, rendered.
    Rendered { sample_rate: u32, channels: u32 },
}

/// One video frame's rendered audio.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBlock {
    /// Index of the first sample frame, counting from the start of the video at `sample_rate`.
    pub start: u64,
    pub sample_rate: u32,
    /// 1 (mono) or 2 (stereo).
    pub channels: u32,
    /// Interleaved samples in `-1..=1`.
    pub samples: Vec<f32>,
}

impl AudioBlock {
    /// Sample frames (samples per channel) in the block.
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }
}

/// The first output sample of video frame `frame`: sample `k` belongs to the frame its time `k / rate`
/// falls in.
pub fn frame_start(frame: i64, rate: f64, fps: f64) -> i64 {
    (frame as f64 * rate / fps - 1e-6).ceil() as i64
}

/// Half-width of the resampling kernel at full bandwidth, in input samples.
const TAPS: f64 = 12.0;
/// The widest kernel, for heavy downsampling where block averaging couldn't help.
const MAX_HALF_WIDTH: usize = 512;

/// Turns the audio output's blocks into audio at a fixed rate: NaN and infinity become silence,
/// the stream is resampled with a windowed-sinc kernel, and the result is clipped to `-1..=1`.
///
/// Causal: each output sample uses only input up to its own time, so a frame's audio is complete
/// once that frame's block is in. The cost is a delay of the kernel's half-width in input samples
/// (a fraction of a millisecond for audio-rate blocks). Picture-sized blocks are first averaged
/// down in groups that divide the block evenly.
#[derive(Debug, Clone)]
pub struct SinkResampler {
    rate: f64,
    fps: f64,
    /// Output channels, and samples per pixel of the input.
    channels: usize,
    /// Sample frames per input block, before and after averaging.
    frames_in: usize,
    group: usize,
    /// Input samples per output sample, after averaging.
    step: f64,
    half_width: usize,
    /// Kernel bandwidth relative to the input's Nyquist frequency.
    cutoff: f64,
    /// Averaged input per channel, from absolute index `base`.
    history: Vec<Vec<f32>>,
    base: i64,
    /// The frame the next block should be, and its first output sample, once started.
    next: Option<(i64, i64)>,
    weights: Vec<f64>,
}

impl SinkResampler {
    /// For blocks of `block_len` samples at `fps` blocks a second, with `samples_per_pixel`
    /// interleaved: 1 is mono, 2 stereo, anything else is read as interleaved stereo.
    pub fn new(block_len: usize, samples_per_pixel: u32, fps: f64, rate: u32) -> Self {
        let channels = if samples_per_pixel == 1 { 1 } else { 2 };
        let frames_in = block_len / channels;
        let rate = f64::from(rate);
        let ratio = frames_in as f64 * fps / rate;
        // Average groups that divide the block evenly, leaving at most about twice the output rate.
        let target = (ratio / 2.0).floor().max(1.0) as usize;
        let group = (1..=target)
            .rev()
            .find(|g| frames_in.is_multiple_of(*g))
            .unwrap_or(1);
        let step = ratio / group as f64;
        let cutoff = (1.0 / step).min(1.0);
        let half_width = ((TAPS / cutoff).ceil() as usize).clamp(TAPS as usize, MAX_HALF_WIDTH);
        Self {
            rate,
            fps,
            channels,
            frames_in,
            group,
            step,
            half_width,
            cutoff,
            history: vec![Vec::new(); channels],
            base: 0,
            next: None,
            weights: Vec::with_capacity(2 * half_width + 1),
        }
    }

    pub fn channels(&self) -> u32 {
        self.channels as u32
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate as u32
    }

    /// Forgets the stream, e.g. after a seek. The next block starts a new one.
    pub fn reset(&mut self) {
        self.next = None;
        for h in &mut self.history {
            h.clear();
        }
    }

    /// Takes the audio output's block for video frame `frame` and returns that frame's audio.
    /// Blocks are expected in order; any other frame starts a new stream (as after a seek), whose
    /// first samples have no history before them.
    pub fn push(&mut self, frame: i64, data: &[f32]) -> AudioBlock {
        let averaged = (self.frames_in / self.group) as i64;
        if self.next.is_none_or(|(expected, _)| expected != frame) {
            self.reset();
            self.base = frame * averaged;
            self.next = Some((frame, frame_start(frame, self.rate, self.fps)));
        }
        let (_, first) = self.next.expect("set above");

        // Sanitized, split into channels and averaged in groups.
        let scale = 1.0 / self.group as f32;
        for (c, history) in self.history.iter_mut().enumerate() {
            let samples = data
                .iter()
                .skip(c)
                .step_by(self.channels)
                .take(self.frames_in);
            let mut sum = 0.0f32;
            for (j, &x) in samples.enumerate() {
                sum += if x.is_finite() { x } else { 0.0 };
                if (j + 1) % self.group == 0 {
                    history.push(sum * scale);
                    sum = 0.0;
                }
            }
        }

        let end = frame_start(frame + 1, self.rate, self.fps);
        let count = (end - first).max(0) as usize;
        let mut samples = vec![0.0; count * self.channels];
        for (n, out) in samples.chunks_exact_mut(self.channels).enumerate() {
            self.sample(first + n as i64, out);
        }
        self.next = Some((frame + 1, end));
        self.trim(end);
        AudioBlock {
            start: first.max(0) as u64,
            sample_rate: self.rate as u32,
            channels: self.channels as u32,
            samples: if first < 0 { Vec::new() } else { samples },
        }
    }

    /// Output sample `k` of every channel: the input around `half_width` samples before `k`'s
    /// time, so only input up to `k`'s time is needed.
    fn sample(&mut self, k: i64, out: &mut [f32]) {
        let w = self.half_width as f64;
        let center = k as f64 * self.step - w;
        let lo = (center - w).floor() as i64 + 1;
        let hi = (center + w).ceil() as i64 - 1;
        self.weights.clear();
        let mut total = 0.0;
        for s in lo..=hi {
            let x = center - s as f64;
            let weight = kernel(x, w, self.cutoff);
            self.weights.push(weight);
            total += weight;
        }
        let norm = if total.abs() > 1e-12 {
            1.0 / total
        } else {
            0.0
        };
        for (history, out) in self.history.iter().zip(out) {
            let mut acc = 0.0;
            for (s, &weight) in (lo..=hi).zip(&self.weights) {
                let i = s - self.base;
                if (0..history.len() as i64).contains(&i) {
                    acc += weight * f64::from(history[i as usize]);
                }
            }
            *out = ((acc * norm) as f32).clamp(-1.0, 1.0);
        }
    }

    /// Drops input no output from sample `next` on will read.
    fn trim(&mut self, next: i64) {
        let needed = (next as f64 * self.step - 2.0 * self.half_width as f64).floor() as i64 - 1;
        let drop = (needed - self.base).clamp(0, self.history[0].len() as i64) as usize;
        if drop > 0 {
            for h in &mut self.history {
                h.drain(..drop);
            }
            self.base += drop as i64;
        }
    }
}

/// A Blackman-windowed sinc with bandwidth `cutoff` (of the input's Nyquist frequency), reaching
/// zero at `±half_width`.
fn kernel(x: f64, half_width: f64, cutoff: f64) -> f64 {
    use std::f64::consts::PI;
    if x.abs() >= half_width {
        return 0.0;
    }
    let sinc = if x.abs() < 1e-12 {
        1.0
    } else {
        (PI * cutoff * x).sin() / (PI * cutoff * x)
    };
    let t = (x / half_width + 1.0) / 2.0;
    let window = 0.42 - 0.5 * (2.0 * PI * t).cos() + 0.08 * (4.0 * PI * t).cos();
    cutoff * sinc * window
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds frames `frames` of a stream whose sample `s` (per channel, absolute) is `f(s)`.
    fn run(
        resampler: &mut SinkResampler,
        frames: std::ops::Range<i64>,
        block_len: usize,
        channels: usize,
        f: impl Fn(i64, usize) -> f32,
    ) -> Vec<AudioBlock> {
        let per = (block_len / channels) as i64;
        frames
            .map(|frame| {
                let data: Vec<f32> = (0..block_len)
                    .map(|i| f(frame * per + (i / channels) as i64, i % channels))
                    .collect();
                resampler.push(frame, &data)
            })
            .collect()
    }

    #[test]
    fn blocks_tile_the_timeline_exactly() {
        // 48 kHz out at 29.97 fps: 1601.6 samples a frame, as 1601 or 1602, without gaps.
        let fps = 30_000.0 / 1001.0;
        let mut r = SinkResampler::new(1470, 1, fps, 48_000);
        let blocks = run(&mut r, 0..10, 1470, 1, |_, _| 0.0);
        let mut next = 0;
        for b in &blocks {
            assert_eq!(b.start, next);
            assert!(matches!(b.frames(), 1601 | 1602), "{}", b.frames());
            next += b.frames() as u64;
        }
        assert_eq!(next, frame_start(10, 48_000.0, fps) as u64);
    }

    #[test]
    fn a_steady_tone_crosses_block_edges_without_clicks() {
        // A 440 Hz sine at 44.1 kHz in 1470-sample blocks (30 fps), resampled to 48 kHz. Away
        // from the start, every output sample matches the sine, delayed by the kernel.
        let (fps, rate_in) = (30.0, 44_100.0);
        let tone = |s: i64| (std::f64::consts::TAU * 440.0 * s as f64 / rate_in).sin();
        let mut r = SinkResampler::new(1470, 1, fps, 48_000);
        let delay = r.half_width as f64 / rate_in;
        let blocks = run(&mut r, 0..6, 1470, 1, |s, _| tone(s) as f32 * 0.5);
        let mut worst = 0.0f64;
        for b in &blocks[1..] {
            for (n, &y) in b.samples.iter().enumerate() {
                let t = (b.start + n as u64) as f64 / 48_000.0 - delay;
                let expected = 0.5 * (std::f64::consts::TAU * 440.0 * t).sin();
                worst = worst.max((f64::from(y) - expected).abs());
            }
        }
        assert!(worst < 2e-3, "worst error {worst}");
    }

    #[test]
    fn seeking_gives_the_same_audio_after_one_frame_of_history() {
        let f = |s: i64, c: usize| ((s * 7 + c as i64 * 3) % 23) as f32 / 23.0 - 0.5;
        let mut sequential = SinkResampler::new(1000, 2, 25.0, 48_000);
        let all = run(&mut sequential, 0..8, 1000, 2, f);
        let mut seeking = SinkResampler::new(1000, 2, 25.0, 48_000);
        let resumed = run(&mut seeking, 4..8, 1000, 2, f);
        assert_eq!(resumed[1..], all[5..]);
        assert_eq!(resumed[0].channels, 2);
    }

    #[test]
    fn sanitizes_and_clips() {
        let mut r = SinkResampler::new(480, 1, 100.0, 48_000);
        let blocks = run(&mut r, 0..3, 480, 1, |s, _| match s % 3 {
            0 => f32::NAN,
            1 => f32::INFINITY,
            _ => 5.0,
        });
        for b in &blocks {
            assert!(b.samples.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
        }
    }

    #[test]
    fn pictures_are_averaged_down_to_audio() {
        // A 1920×1080 RGB frame read as stereo at 30 fps is ~93 MHz: averaged in groups first.
        let len = 1920 * 1080 * 3;
        let r = SinkResampler::new(len, 3, 30.0, 48_000);
        assert_eq!(r.channels(), 2);
        assert!(r.group > 1 && (len / 2).is_multiple_of(r.group));
        assert!(r.step <= 2.5, "step {}", r.step);
    }
}
