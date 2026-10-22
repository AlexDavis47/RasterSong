//! Preview audio output: plays the mix through the default device, time-stretched to follow the
//! playback clock.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use rastersong_engine::Tempo;
use rastersong_engine::playback::{Metronome, Mixer, Stretcher, speed_gain};
use rastersong_lang::tr;

/// Where playback is, as last reported by the UI. The audio thread extrapolates from it.
#[derive(Debug, Clone, Copy)]
struct Transport {
    /// Video time in seconds at `at`.
    position: f64,
    speed: f64,
    playing: bool,
    volume: f32,
    /// The tempo to click along with, or `None` for no metronome.
    metronome: Option<Tempo>,
    at: Instant,
}

/// A connection being listened to: its sound, mixed over the playback while it plays.
#[derive(Clone)]
struct ListenPlay {
    mixer: Arc<Mixer>,
    /// Changes with every listen, so the audio thread notices a new one.
    id: u64,
}

/// How long the listened sound takes to fade in and out, in seconds.
const LISTEN_FADE_SECS: f64 = 0.03;
/// How much of the playback is turned down while listening, `0..=1`.
const LISTEN_DUCK: f32 = 0.9;

#[derive(Default)]
struct Shared {
    transport: Mutex<Option<Transport>>,
    mixer: Mutex<Arc<Mixer>>,
    listen: Mutex<Option<ListenPlay>>,
    listens: std::sync::atomic::AtomicU64,
}

/// The audio device. If no device is available the app keeps working, silently.
pub struct AudioOut {
    shared: Arc<Shared>,
    _stream: Option<cpal::Stream>,
    pub error: Option<String>,
}

impl std::fmt::Debug for AudioOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioOut")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl AudioOut {
    /// Opens the default output device.
    pub fn start() -> Self {
        let shared = Arc::new(Shared::default());
        match open(shared.clone()) {
            Ok(stream) => Self {
                shared,
                _stream: Some(stream),
                error: None,
            },
            Err(e) => {
                tracing::warn!("no audio output: {e}");
                Self::silent(Some(e))
            }
        }
    }

    /// No device: for tests, or when none is available.
    pub fn silent(error: Option<String>) -> Self {
        Self {
            shared: Arc::new(Shared::default()),
            _stream: None,
            error,
        }
    }

    /// Reports the playback position (video seconds), speed and volume, and the tempo of the
    /// metronome if it is on. Call every UI frame.
    pub fn update(
        &self,
        position: f64,
        speed: f64,
        playing: bool,
        volume: f32,
        metronome: Option<Tempo>,
    ) {
        if let Ok(mut transport) = self.shared.transport.lock() {
            *transport = Some(Transport {
                position,
                speed,
                playing,
                volume,
                metronome,
                at: Instant::now(),
            });
        }
    }

    /// Mixes `mixer` over the playback, turning the playback down, until [`Self::stop_listening`].
    /// It follows the playback's position, so it is silent while playback is stopped: what is
    /// heard always matches the picture. Fades in.
    pub fn listen(&self, mixer: Mixer) {
        let id = self
            .shared
            .listens
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        if let Ok(mut listen) = self.shared.listen.lock() {
            *listen = Some(ListenPlay {
                mixer: Arc::new(mixer),
                id,
            });
        }
    }

    /// Fades the listened sound out and gives the playback its volume back.
    pub fn stop_listening(&self) {
        if let Ok(mut listen) = self.shared.listen.lock() {
            *listen = None;
        }
    }

    pub fn set_mixer(&self, mixer: Mixer) {
        if let Ok(mut current) = self.shared.mixer.lock() {
            *current = Arc::new(mixer);
        }
    }
}

fn open(shared: Arc<Shared>) -> Result<cpal::Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| tr("error.no_output_device").to_owned())?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, shared),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, shared),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, shared),
        _ => build::<f32>(&device, &config, shared),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: Arc<Shared>,
) -> Result<cpal::Stream, String> {
    let channels = config.channels as usize;
    let mut source = Source::new(f64::from(config.sample_rate));
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], _| {
                let transport = shared.transport.try_lock().ok().and_then(|t| *t);
                let mixer = shared.mixer.try_lock().ok().map(|m| m.clone());
                if let (Some(transport), Some(mixer)) = (transport, mixer) {
                    source.transport = Some(transport);
                    source.mixer = mixer;
                }
                if let Ok(listen) = shared.listen.try_lock() {
                    source.listen(listen.clone());
                }
                for frame in data.chunks_mut(channels) {
                    let [left, right] = source.next_frame();
                    for (c, out) in frame.iter_mut().enumerate() {
                        *out = T::from_sample(if c % 2 == 0 { left } else { right });
                    }
                }
            },
            |e| tracing::warn!("audio output error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

/// The audio thread's side: stretched stereo frames, one at a time.
struct Source {
    stretcher: Stretcher,
    mixer: Arc<Mixer>,
    metronome: Metronome,
    transport: Option<Transport>,
    hop: Vec<f32>,
    /// Frames of `hop` already played.
    played: usize,
    listened: Listened,
}

/// The audio thread's side of listening to a connection.
struct Listened {
    stretcher: Stretcher,
    /// The sound to mix in, kept while it fades out after listening stops.
    mixer: Option<Arc<Mixer>>,
    wanted: bool,
    id: u64,
    /// `0..=1`.
    gain: f32,
    hop: Vec<f32>,
}

impl Source {
    fn new(rate: f64) -> Self {
        let stretcher = Stretcher::new(rate);
        let stretcher_hop = stretcher.hop();
        let hop = vec![0.0; stretcher_hop * 2];
        Self {
            played: stretcher.hop(),
            stretcher,
            mixer: Arc::new(Mixer::default()),
            metronome: Metronome::new(rate),
            transport: None,
            hop,
            listened: Listened {
                stretcher: Stretcher::new(rate),
                mixer: None,
                wanted: false,
                id: 0,
                gain: 0.0,
                hop: vec![0.0; stretcher_hop * 2],
            },
        }
    }

    /// Takes what the UI wants listened to, or `None` to fade it out.
    fn listen(&mut self, play: Option<ListenPlay>) {
        let listened = &mut self.listened;
        listened.wanted = play.is_some();
        if let Some(play) = play {
            if play.id != listened.id {
                listened.id = play.id;
                listened.stretcher.reset();
            }
            listened.mixer = Some(play.mixer);
        }
    }

    fn next_frame(&mut self) -> [f32; 2] {
        if self.played == self.stretcher.hop() {
            self.next_hop();
        }
        let i = self.played * 2;
        self.played += 1;
        [self.hop[i], self.hop[i + 1]]
    }

    fn next_hop(&mut self) {
        self.played = 0;
        let hop_secs = self.stretcher.hop() as f64 / self.stretcher.rate();
        let fade = (hop_secs / LISTEN_FADE_SECS) as f32;
        let listened = &mut self.listened;
        listened.gain = if listened.wanted {
            (listened.gain + fade).min(1.0)
        } else {
            (listened.gain - fade).max(0.0)
        };
        let duck = 1.0 - LISTEN_DUCK * listened.gain;
        match self.transport {
            Some(t) if t.playing && t.speed > 0.0 => {
                // Where the clock is now, extrapolated from the UI's last report.
                let position = t.position + t.at.elapsed().as_secs_f64() * t.speed;
                let gain = t.volume * speed_gain(t.speed) * duck;
                self.stretcher
                    .next(&self.mixer, position, gain, &mut self.hop);
                // Clicks follow the video's time, not the stretched audio, and ignore the speed
                // fade so they stay audible while tuning at a crawl.
                match t.metronome {
                    Some(tempo) => {
                        self.metronome
                            .render(tempo, position, t.speed, t.volume, &mut self.hop);
                    }
                    None => self.metronome.reset(),
                }
            }
            _ => {
                self.stretcher.reset();
                self.metronome.reset();
                self.hop.fill(0.0);
            }
        }
        self.mix_listened();
    }

    /// Adds the listened sound to the hop just rendered.
    fn mix_listened(&mut self) {
        let listened = &mut self.listened;
        if listened.gain <= 0.0 && !listened.wanted {
            listened.mixer = None;
            return;
        }
        let Some(mixer) = listened.mixer.clone() else {
            return;
        };
        // Like the playback, the listened sound only plays while the transport does.
        let Some(t) = self.transport.filter(|t| t.playing && t.speed > 0.0) else {
            listened.stretcher.reset();
            return;
        };
        let position = t.position + t.at.elapsed().as_secs_f64() * t.speed;
        let gain = t.volume * speed_gain(t.speed);
        listened
            .stretcher
            .next(&mixer, position, gain * listened.gain, &mut listened.hop);
        for (out, heard) in self.hop.iter_mut().zip(&listened.hop) {
            *out += heard;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rastersong_engine::AudioBlock;
    use rastersong_engine::playback::RenderedSource;

    /// A steady 0.5 at 48 kHz in every frame of video at 30 fps.
    #[derive(Debug)]
    struct Steady;

    impl RenderedSource for Steady {
        fn blocks(&self, frames: std::ops::Range<usize>) -> Vec<Option<Arc<AudioBlock>>> {
            frames
                .map(|f| {
                    Some(Arc::new(AudioBlock {
                        start: f as u64 * 1600,
                        sample_rate: 48_000,
                        channels: 1,
                        samples: vec![0.5; 1600],
                    }))
                })
                .collect()
        }

        fn frame_rate(&self) -> f64 {
            30.0
        }
    }

    fn pull(source: &mut Source, seconds: f64) -> f32 {
        let mut last = 0.0;
        for _ in 0..(seconds * 48_000.0) as usize {
            last = source.next_frame()[0];
        }
        last
    }

    #[test]
    fn listened_sound_follows_the_transport_and_fades_out_when_listening_stops() {
        let mut source = Source::new(48_000.0);
        let transport = |playing| Transport {
            position: 0.5,
            speed: 1.0,
            playing,
            volume: 1.0,
            metronome: None,
            at: Instant::now(),
        };
        source.listen(Some(ListenPlay {
            mixer: Arc::new(Mixer::rendered(Arc::new(Steady), 1.0)),
            id: 1,
        }));
        // Paused: nothing is heard, whether or not a transport has been reported.
        assert_eq!(pull(&mut source, 0.3), 0.0);
        source.transport = Some(transport(false));
        assert_eq!(pull(&mut source, 0.3), 0.0);

        source.transport = Some(transport(true));
        let heard = pull(&mut source, 0.3);
        assert!((heard - 0.5).abs() < 0.05, "{heard}");

        source.listen(None);
        let after = pull(&mut source, 0.3);
        assert!(after.abs() < 0.01, "{after}");
        assert!(source.listened.mixer.is_none(), "released once silent");
    }
}
