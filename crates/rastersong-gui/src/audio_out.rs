//! Preview audio output: plays the mix through the default device, time-stretched to follow the
//! playback clock.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use rastersong_engine::Tempo;
use rastersong_engine::playback::{Metronome, Mixer, Stretcher, speed_gain};

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

#[derive(Default)]
struct Shared {
    transport: Mutex<Option<Transport>>,
    mixer: Mutex<Arc<Mixer>>,
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

    pub fn set_mixer(&self, mixer: Mixer) {
        if let Ok(mut current) = self.shared.mixer.lock() {
            *current = Arc::new(mixer);
        }
    }
}

fn open(shared: Arc<Shared>) -> Result<cpal::Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no output device")?;
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
}

impl Source {
    fn new(rate: f64) -> Self {
        let stretcher = Stretcher::new(rate);
        let hop = vec![0.0; stretcher.hop() * 2];
        Self {
            played: stretcher.hop(),
            stretcher,
            mixer: Arc::new(Mixer::default()),
            metronome: Metronome::new(rate),
            transport: None,
            hop,
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
        match self.transport {
            Some(t) if t.playing && t.speed > 0.0 => {
                // Where the clock is now, extrapolated from the UI's last report.
                let position = t.position + t.at.elapsed().as_secs_f64() * t.speed;
                let gain = t.volume * speed_gain(t.speed);
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
    }
}
