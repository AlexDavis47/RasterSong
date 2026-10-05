//! Preview audio: mixing the tracks, and time-stretching them to follow the playback clock.
//!
//! Playback speed changes as rendering keeps up or falls behind, so the audio is time-stretched
//! (slowed without lowering the pitch) to stay with the picture. This module is pure DSP; the
//! audio device is driven by the app.

use std::ops::Range;
use std::sync::Arc;

use rastersong_graph::Tempo;
use rastersong_media::AudioClip;

use crate::AudioBlock;

/// One track in the mix.
#[derive(Debug, Clone)]
pub struct MixTrack {
    pub clip: Arc<AudioClip>,
    /// Seconds the track starts after the video.
    pub offset: f64,
    /// Linear gain; 0 for muted.
    pub gain: f32,
}

/// Rendered sound, one block per video frame, as the preview has it so far.
pub trait RenderedSource: Send + Sync + std::fmt::Debug {
    /// The rendered audio of each frame in `frames`, where it's ready.
    fn blocks(&self, frames: Range<usize>) -> Vec<Option<Arc<AudioBlock>>>;

    /// Video frames a second, to find the frame a time falls in. Zero when unknown.
    fn frame_rate(&self) -> f64;
}

/// Sums tracks, or the graph's rendered sound, into stereo at any rate and position.
#[derive(Debug, Clone, Default)]
pub struct Mixer {
    tracks: Vec<MixTrack>,
    rendered: Option<(Arc<dyn RenderedSource>, f32)>,
}

impl Mixer {
    pub fn new(tracks: Vec<MixTrack>) -> Self {
        Self {
            tracks,
            rendered: None,
        }
    }

    /// Plays the graph's rendered sound at linear `gain`. Frames not rendered yet are silent.
    pub fn rendered(source: Arc<dyn RenderedSource>, gain: f32) -> Self {
        Self {
            tracks: Vec::new(),
            rendered: Some((source, gain)),
        }
    }

    /// Writes `out.len() / 2` interleaved stereo frames at `rate` Hz, starting at `start` seconds
    /// of video time. Time outside a track is silence; mono tracks play in both channels.
    pub fn render(&self, start: f64, rate: f64, out: &mut [f32]) {
        out.fill(0.0);
        if let Some((source, gain)) = &self.rendered {
            render_blocks(source.as_ref(), *gain, start, rate, out);
        }
        for track in self.tracks.iter().filter(|t| t.gain > 0.0) {
            let clip = &track.clip;
            let channels = clip.channels as usize;
            let frames = clip.frames();
            if frames < 2 || channels == 0 {
                continue;
            }
            let step = f64::from(clip.sample_rate) / rate;
            let mut pos = (start - track.offset) * f64::from(clip.sample_rate);
            for frame in out.as_chunks_mut::<2>().0 {
                if pos >= 0.0 && pos < (frames - 1) as f64 {
                    let i = pos as usize;
                    let frac = (pos - i as f64) as f32;
                    let at = |f: usize, c: usize| clip.samples[f * channels + c.min(channels - 1)];
                    for (c, out) in frame.iter_mut().enumerate() {
                        let a = at(i, c);
                        *out += (a + (at(i + 1, c) - a) * frac) * track.gain;
                    }
                }
                pos += step;
            }
        }
    }
}

/// Adds rendered blocks covering `start` onwards to `out` (stereo at `rate`), interpolating
/// between their samples. Mono blocks play in both channels.
fn render_blocks(source: &dyn RenderedSource, gain: f32, start: f64, rate: f64, out: &mut [f32]) {
    let fps = source.frame_rate();
    if fps <= 0.0 || gain <= 0.0 {
        return;
    }
    let end = start + (out.len() / 2) as f64 / rate;
    let first = ((start * fps).floor() - 1.0).max(0.0) as usize;
    let last = ((end * fps).floor() + 2.0).max(0.0) as usize;
    let blocks: Vec<Arc<AudioBlock>> = source.blocks(first..last).into_iter().flatten().collect();
    let Some(block_rate) = blocks.first().map(|b| f64::from(b.sample_rate)) else {
        return;
    };
    // Sample frame `k` (at the blocks' rate) of channel `c`, or silence where nothing is rendered.
    let sample = |k: i64, c: usize| -> f32 {
        let Ok(k) = u64::try_from(k) else {
            return 0.0;
        };
        blocks
            .iter()
            .find(|b| b.start <= k && k < b.start + b.frames() as u64)
            .map_or(0.0, |b| {
                let channels = b.channels.max(1) as usize;
                b.samples[(k - b.start) as usize * channels + c.min(channels - 1)]
            })
    };
    for (j, frame) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let pos = (start + j as f64 / rate) * block_rate;
        let i = pos.floor() as i64;
        let frac = (pos - i as f64) as f32;
        for (c, out) in frame.iter_mut().enumerate() {
            let a = sample(i, c);
            *out += (a + (sample(i + 1, c) - a) * frac) * gain;
        }
    }
}

/// Grain length in seconds. Long enough to hold a few periods of a bass note.
const GRAIN_SECS: f64 = 0.042;
/// How far a grain may move from its target to line up with the previous one.
const TOLERANCE_SECS: f64 = 0.006;
/// Correlation is computed on every Nth sample; plenty for alignment and much cheaper.
const DECIMATION: usize = 4;
/// Jumps larger than this (a seek) are taken as-is, without alignment.
const SEEK_SECS: f64 = 0.25;

/// WSOLA (waveform-similarity overlap-add) time-stretching.
///
/// Output is produced in hops of half a grain. Each grain is read from the source at the position
/// the playback clock says it should be at, nudged by up to [`TOLERANCE_SECS`] so its waveform
/// lines up with where the previous grain would have continued. At full speed that's an exact
/// copy of the source; slower, the content advances more slowly but keeps its pitch.
#[derive(Debug, Clone)]
pub struct Stretcher {
    rate: f64,
    len: usize,
    tolerance: usize,
    window: Vec<f32>,
    /// The second half of the previous windowed grain, still to be added (stereo).
    overlap: Vec<f32>,
    /// Source position of the previous grain's start.
    previous: Option<f64>,
    grain: Vec<f32>,
    natural: Vec<f32>,
    search: Vec<f32>,
}

impl Stretcher {
    pub fn new(rate: f64) -> Self {
        let len = ((GRAIN_SECS * rate) as usize / 2 * 2).max(64);
        // Periodic Hann: windows half a grain apart sum to exactly one.
        let window = (0..len)
            .map(|i| (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / len as f64).cos()) as f32)
            .collect();
        Self {
            rate,
            len,
            tolerance: (TOLERANCE_SECS * rate) as usize,
            window,
            overlap: vec![0.0; len],
            previous: None,
            grain: vec![0.0; len * 2],
            natural: vec![0.0; len * 2],
            search: Vec::new(),
        }
    }

    /// Stereo frames produced by each call to [`Self::next`].
    pub fn hop(&self) -> usize {
        self.len / 2
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Forgets the previous grain, e.g. after playback stops.
    pub fn reset(&mut self) {
        self.previous = None;
        self.overlap.fill(0.0);
    }

    /// Writes the next [`Self::hop`] stereo frames to `out`, with the source at `position`
    /// seconds. Call with positions advancing at the playback speed: by `hop / rate × speed`.
    pub fn next(&mut self, mixer: &Mixer, position: f64, gain: f32, out: &mut [f32]) {
        let hop = self.hop();
        debug_assert_eq!(out.len(), hop * 2);
        let start = match self.previous {
            Some(previous) => {
                let natural = previous + hop as f64 / self.rate;
                if (position - natural).abs() > SEEK_SECS {
                    position
                } else {
                    self.align(mixer, position, natural)
                }
            }
            None => position,
        };
        mixer.render(start, self.rate, &mut self.grain);
        for (frame, &w) in self
            .grain
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(&self.window)
        {
            frame[0] *= w * gain;
            frame[1] *= w * gain;
        }
        for (i, out) in out.iter_mut().enumerate() {
            *out = self.overlap[i] + self.grain[i];
        }
        self.overlap.copy_from_slice(&self.grain[hop * 2..]);
        self.previous = Some(start);
    }

    /// The start near `target` whose waveform best matches the natural continuation of the
    /// previous grain (normalized cross-correlation on a decimated mono mix).
    fn align(&mut self, mixer: &Mixer, target: f64, natural: f64) -> f64 {
        let (len, tol) = (self.len, self.tolerance);
        mixer.render(natural, self.rate, &mut self.natural);
        self.search.resize((len + 2 * tol) * 2, 0.0);
        mixer.render(target - tol as f64 / self.rate, self.rate, &mut self.search);

        let mono = |buf: &[f32], i: usize| buf[i * 2] + buf[i * 2 + 1];
        let mut best = (f32::NEG_INFINITY, tol);
        for shift in (0..=2 * tol).step_by(DECIMATION / 2) {
            let (mut dot, mut energy) = (0.0f32, 0.0f32);
            for i in (0..len).step_by(DECIMATION) {
                let c = mono(&self.search, shift + i);
                dot += c * mono(&self.natural, i);
                energy += c * c;
            }
            let score = if energy > 1e-12 {
                dot / energy.sqrt()
            } else {
                0.0
            };
            // Prefer staying on target when scores tie (e.g. silence).
            let closer = shift.abs_diff(tol) < best.1.abs_diff(tol);
            if score > best.0 || (score == best.0 && closer) {
                best = (score, shift);
            }
        }
        target + (best.1 as f64 - tol as f64) / self.rate
    }
}

/// Pitch of the click on the first beat of a bar, and on the other beats.
const ACCENT_HZ: f64 = 1_600.0;
const BEAT_HZ: f64 = 1_000.0;
/// How long a click rings, as the time its level takes to fall by 60 dB.
const CLICK_SECS: f64 = 0.03;
const CLICK_LEVEL: f32 = 0.5;

/// A metronome for tuning the project's tempo by ear: a short click on every beat of the grid
/// the beat and bar units follow, higher on the first beat of each bar. It follows the video's
/// time, not the stretched audio's, so clicks stay with the picture at any playback speed.
#[derive(Debug, Clone)]
pub struct Metronome {
    rate: f64,
    /// Index of the beat the previous sample was in, or `None` after a reset or a stop.
    beat: Option<i64>,
    /// The click that is ringing: samples played so far and its pitch.
    click: Option<(usize, f64)>,
}

impl Metronome {
    pub fn new(rate: f64) -> Self {
        Self {
            rate,
            beat: None,
            click: None,
        }
    }

    /// Forgets where the last beat was, e.g. after playback stops, so the next render doesn't
    /// click for a beat it only jumped over.
    pub fn reset(&mut self) {
        self.beat = None;
        self.click = None;
    }

    /// Adds the clicks for the stereo frames `out` to it, where the video is at `position` seconds
    /// at its start and moves `speed` seconds per second.
    pub fn render(&mut self, tempo: Tempo, position: f64, speed: f64, gain: f32, out: &mut [f32]) {
        let tempo = tempo.sanitized();
        let beats_per_second = tempo.bpm / 60.0;
        let step = speed / self.rate;
        for (i, frame) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let time = position + i as f64 * step;
            let beat = ((time - tempo.offset_secs) * beats_per_second).floor() as i64;
            // A click starts when the beat counter moves forward onto a beat of the grid. Moving
            // back (a loop or a seek) and the time before the first beat stay silent.
            if beat >= 0 && self.beat.is_some_and(|previous| beat > previous) {
                let downbeat = beat % i64::from(tempo.beats_per_bar) == 0;
                self.click = Some((0, if downbeat { ACCENT_HZ } else { BEAT_HZ }));
            }
            self.beat = Some(beat);
            if let Some((age, hz)) = self.click {
                let t = age as f64 / self.rate;
                let envelope = 10f64.powf(-3.0 * t / CLICK_SECS);
                let value =
                    ((std::f64::consts::TAU * hz * t).sin() * envelope) as f32 * CLICK_LEVEL * gain;
                frame[0] += value;
                frame[1] += value;
                self.click = (t < CLICK_SECS * 2.0).then_some((age + 1, hz));
            }
        }
    }
}

/// Fades preview audio out at very low playback speeds, where stretching turns into a drone.
pub fn speed_gain(speed: f64) -> f32 {
    ((speed - 0.05) / 0.15).clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    fn tone(frequency: f64, seconds: f64) -> Arc<AudioClip> {
        let n = (RATE * seconds) as usize;
        Arc::new(AudioClip {
            sample_rate: RATE as u32,
            channels: 1,
            samples: (0..n)
                .map(|i| (std::f64::consts::TAU * frequency * i as f64 / RATE).sin() as f32 * 0.5)
                .collect(),
        })
    }

    fn mixer(clip: Arc<AudioClip>) -> Mixer {
        Mixer::new(vec![MixTrack {
            clip,
            offset: 0.0,
            gain: 1.0,
        }])
    }

    /// Runs the stretcher at `speed` for `hops` hops starting at `start`; returns the left channel.
    fn stretch(mixer: &Mixer, speed: f64, start: f64, hops: usize) -> Vec<f32> {
        let mut s = Stretcher::new(RATE);
        let hop = s.hop();
        let mut out = vec![0.0; hop * 2];
        let mut left = Vec::new();
        for k in 0..hops {
            s.next(
                mixer,
                start + k as f64 * hop as f64 / RATE * speed,
                1.0,
                &mut out,
            );
            left.extend(out.iter().step_by(2));
        }
        left
    }

    fn frequency(samples: &[f32]) -> f64 {
        let crossings = samples
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        crossings as f64 / (samples.len() as f64 / RATE)
    }

    #[test]
    fn mixer_applies_offset_gain_and_channels() {
        let clip = Arc::new(AudioClip {
            sample_rate: 4,
            channels: 2,
            samples: vec![1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0],
        });
        let mixer = Mixer::new(vec![
            MixTrack {
                clip: clip.clone(),
                offset: 1.0,
                gain: 0.5,
            },
            MixTrack {
                clip,
                offset: 0.0,
                gain: 0.0,
            },
        ]);
        let mut out = vec![9.0; 8 * 2];
        // 4 Hz output from 0 s: the track starts at 1 s and lasts 1 s.
        mixer.render(0.0, 4.0, &mut out);
        assert_eq!(&out[..8], [0.0; 8], "before the offset: silence");
        assert_eq!(
            &out[8..12],
            [0.5, -0.5, 0.5, -0.5],
            "left and right kept, gain applied"
        );
    }

    #[test]
    fn full_speed_reproduces_the_source() {
        let clip = tone(440.0, 1.0);
        let mixer = mixer(clip.clone());
        let out = stretch(&mixer, 1.0, 0.0, 30);
        let hop = Stretcher::new(RATE).hop();
        // After the first hop (fading in from nothing), output equals the source.
        for (i, (&got, &want)) in out.iter().zip(&clip.samples).enumerate().skip(hop) {
            assert!((got - want).abs() < 1e-4, "sample {i}: {got} vs {want}");
        }
    }

    #[test]
    fn half_speed_keeps_the_pitch() {
        let mixer = mixer(tone(440.0, 2.0));
        let out = stretch(&mixer, 0.5, 0.2, 80);
        let steady = &out[4800..];
        let f = frequency(steady);
        assert!((f - 440.0).abs() < 8.0, "pitch {f} Hz");
        // No dropouts: the level stays close to the tone's RMS (0.5 / √2).
        for chunk in steady.chunks(2400) {
            let rms = (chunk.iter().map(|x| x * x).sum::<f32>() / chunk.len() as f32).sqrt();
            assert!((rms - 0.354).abs() < 0.06, "rms {rms}");
        }
    }

    #[test]
    fn seeks_jump_without_alignment() {
        let mixer = mixer(tone(440.0, 3.0));
        let mut s = Stretcher::new(RATE);
        let mut out = vec![0.0; s.hop() * 2];
        s.next(&mixer, 0.0, 1.0, &mut out);
        s.next(&mixer, 2.0, 1.0, &mut out);
        assert_eq!(s.previous, Some(2.0));
    }

    /// Renders `seconds` of the metronome at full speed, in hops like the audio thread's.
    fn click_track(tempo: Tempo, start: f64, seconds: f64) -> Vec<f32> {
        let mut metronome = Metronome::new(RATE);
        let hop = 1_024;
        let frames = (seconds * RATE) as usize / hop * hop;
        let mut left = Vec::new();
        for h in 0..frames / hop {
            let mut out = vec![0.0; hop * 2];
            let position = start + (h * hop) as f64 / RATE;
            metronome.render(tempo, position, 1.0, 1.0, &mut out);
            left.extend(out.as_chunks::<2>().0.iter().map(|f| f[0]));
        }
        left
    }

    fn loud_between(track: &[f32], from: f64, to: f64) -> bool {
        let range = (from * RATE) as usize..(to * RATE) as usize;
        track[range].iter().any(|x| x.abs() > 0.01)
    }

    #[test]
    fn clicks_on_every_beat_and_nowhere_else() {
        // 120 bpm: a beat every half second. The first sample isn't a click: nothing was before it.
        let track = click_track(Tempo::default(), 0.0, 2.0);
        for beat in [0.5, 1.0, 1.5] {
            assert!(loud_between(&track, beat, beat + 0.01), "click at {beat}");
            assert!(
                !loud_between(&track, beat + 0.12, beat + 0.45),
                "gap after {beat}"
            );
        }
        assert!(!loud_between(&track, 0.0, 0.45));
    }

    #[test]
    fn the_offset_moves_the_grid() {
        let tempo = Tempo {
            offset_secs: 0.2,
            ..Tempo::default()
        };
        let track = click_track(tempo, 0.0, 1.0);
        assert!(!loud_between(&track, 0.0, 0.19));
        assert!(loud_between(&track, 0.7, 0.71));
    }

    #[test]
    fn nothing_clicks_before_the_first_beat() {
        let tempo = Tempo {
            offset_secs: 1.0,
            ..Tempo::default()
        };
        // 0 to 0.9 s lies before the offset, where beat numbers are negative.
        let track = click_track(tempo, 0.0, 0.9);
        assert!(!loud_between(&track, 0.0, 0.89));
    }

    #[test]
    fn the_first_beat_of_a_bar_is_higher() {
        // Beats 4 and 5 of a 4 beat bar start at 1.5 s and 2.0 s; zero crossings tell the pitch.
        let track = click_track(Tempo::default(), 0.0, 2.1);
        let crossings = |at: f64| {
            let from = (at * RATE) as usize;
            track[from..from + 480]
                .windows(2)
                .filter(|w| w[0] * w[1] < 0.0)
                .count()
        };
        assert!(
            crossings(2.0) > crossings(1.5) + 2,
            "{} {}",
            crossings(2.0),
            crossings(1.5)
        );
    }

    #[test]
    fn jumping_back_stays_silent_and_stopping_forgets_the_beat() {
        let mut metronome = Metronome::new(RATE);
        let mut out = vec![0.0; 2_048];
        metronome.render(Tempo::default(), 1.9, 1.0, 1.0, &mut out);
        out.fill(0.0);
        // A loop wraps from 2 s back to 0.1 s: the beat number drops from 4 to 0.
        metronome.render(Tempo::default(), 0.1, 1.0, 1.0, &mut out);
        assert!(out.iter().all(|&x| x == 0.0));
        // Without the reset, 0.9 s (beat 1) after the last render in beat 0 would click.
        metronome.reset();
        metronome.render(Tempo::default(), 0.9, 1.0, 1.0, &mut out);
        assert!(
            out.iter().all(|&x| x == 0.0),
            "no click for a beat only jumped over"
        );
    }

    #[test]
    fn fades_out_when_nearly_stalled() {
        assert_eq!(speed_gain(0.0), 0.0);
        assert_eq!(speed_gain(0.05), 0.0);
        assert_eq!(speed_gain(1.0), 1.0);
        assert!(speed_gain(0.1) > 0.0 && speed_gain(0.1) < 1.0);
    }
}
