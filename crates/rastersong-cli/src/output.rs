//! Render outputs: a lossless `.mkv` file or a PNG sequence.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use rastersong_engine::playback::{MixTrack, Mixer};
use rastersong_engine::{
    AudioClip, AudioSink, Bus, EngineError, FrameSink, LosslessWriter, RenderInfo, RenderedFrame,
};

/// An audio track the sound of the output can be made of, when the graph renders none.
#[derive(Debug, Clone)]
pub struct SourceTrack {
    pub name: String,
    /// The bus it is summed into in the track mix.
    pub bus: String,
    pub track: MixTrack,
}

pub enum Output {
    /// Waiting for `start` to learn the frame size and rate.
    Mkv {
        path: PathBuf,
        /// The audio tracks, mixed into the sound when the graph doesn't render its own.
        tracks: Vec<SourceTrack>,
        /// The rate the mix is written at.
        audio_rate: u32,
        /// The bus written: the track mix takes its tracks and its channels.
        bus: Bus,
        writer: Option<Box<LosslessWriter>>,
    },
    Png {
        dir: PathBuf,
        size: (u32, u32),
    },
}

impl Output {
    /// A `.mkv` path writes a video file with the sound of `bus` (the graph's, or else the mix
    /// of the `tracks` routed to it, at `audio_rate` with the bus's channels); anything else is a
    /// directory for PNG frames.
    pub fn new(path: &Path, tracks: Vec<SourceTrack>, audio_rate: u32, bus: Bus) -> Self {
        let is_mkv = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mkv"));
        if is_mkv {
            Self::Mkv {
                path: path.to_owned(),
                tracks,
                audio_rate,
                bus,
                writer: None,
            }
        } else {
            Self::Png {
                dir: path.to_owned(),
                size: (0, 0),
            }
        }
    }

    pub fn finish(self) -> Result<(), EngineError> {
        match self {
            Self::Mkv {
                writer: Some(writer),
                ..
            } => Ok(writer.finish()?),
            _ => Ok(()),
        }
    }
}

fn output_error(context: impl std::fmt::Display, e: impl std::fmt::Display) -> EngineError {
    EngineError::Output(format!("{context}: {e}"))
}

impl FrameSink for Output {
    fn start(&mut self, info: &RenderInfo, sink: &AudioSink) -> Result<(), EngineError> {
        match self {
            Self::Mkv {
                path,
                tracks,
                audio_rate,
                bus,
                writer,
            } => {
                *writer = Some(Box::new(match sink {
                    // The graph's own sound arrives with each frame.
                    AudioSink::Rendered {
                        sample_rate,
                        channels,
                    } => LosslessWriter::create_streaming(
                        path,
                        info.width,
                        info.height,
                        info.frame_rate,
                        *sample_rate,
                        *channels,
                    )?,
                    // The tracks as placed on the timeline: the bus's track mix, or the one
                    // passed through.
                    AudioSink::TrackMix | AudioSink::Passthrough(_) => {
                        let only = match sink {
                            AudioSink::Passthrough(name) => Some(name.as_str()),
                            _ => None,
                        };
                        let mixed = mix(tracks, only, bus, info, *audio_rate);
                        LosslessWriter::create(
                            path,
                            info.width,
                            info.height,
                            info.frame_rate,
                            Some(&mixed),
                        )?
                    }
                }));
            }
            Self::Png { dir, size } => {
                fs::create_dir_all(&*dir).map_err(|e| output_error(dir.display(), e))?;
                *size = (info.width, info.height);
            }
        }
        Ok(())
    }

    fn frame(&mut self, frame: RenderedFrame) -> Result<(), EngineError> {
        match self {
            Self::Mkv { writer, .. } => {
                let writer = writer.as_mut().expect("started");
                if let Some(audio) = frame.audio {
                    writer.push_audio(&audio.samples);
                }
                Ok(writer.write_frame(frame.rgb)?)
            }
            Self::Png { dir, size } => {
                let path = dir.join(png_name(frame.index));
                write_png(&path, *size, frame.rgb).map_err(|e| output_error(path.display(), e))
            }
        }
    }
}

/// The project's length of the `tracks` routed to `bus` (or only the one named `only`, at full
/// volume, wherever it is routed) mixed to the bus's channels at `rate`.
fn mix(
    tracks: &[SourceTrack],
    only: Option<&str>,
    bus: &Bus,
    info: &RenderInfo,
    rate: u32,
) -> AudioClip {
    let tracks = tracks
        .iter()
        .filter(|t| match only {
            Some(name) => t.name == name,
            None => t.bus == bus.name,
        })
        .map(|t| MixTrack {
            gain: if only.is_some() { 1.0 } else { t.track.gain },
            ..t.track.clone()
        })
        .collect();
    let seconds = info.frames as f64 / info.frame_rate.as_f64();
    let channels = bus.channels as usize;
    let mut samples = vec![0.0; (seconds * f64::from(rate)).round() as usize * channels];
    Mixer::new(tracks).render_channels(0.0, f64::from(rate), channels, &mut samples);
    AudioClip {
        sample_rate: rate,
        channels: bus.channels,
        samples: samples.into(),
    }
}

/// File name of frame `index` in a PNG sequence.
pub fn png_name(index: usize) -> String {
    format!("frame_{index:05}.png")
}

fn write_png(
    path: &Path,
    (width, height): (u32, u32),
    rgb: &[u8],
) -> Result<(), png::EncodingError> {
    let file = BufWriter::new(File::create(path)?);
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgb)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rastersong_engine::{Item, Rational, Timebase};

    use super::*;

    #[test]
    fn the_mix_follows_the_timeline_and_its_bus_and_passthrough_picks_one_track() {
        let clip = |value| {
            Arc::new(AudioClip {
                sample_rate: 4,
                channels: 1,
                samples: vec![value; 4].into(),
            })
        };
        let tracks = vec![
            SourceTrack {
                name: "a".into(),
                bus: "Main".into(),
                track: MixTrack {
                    clip: clip(0.5),
                    items: vec![Item::whole(0.0)],
                    gain: 1.0,
                },
            },
            SourceTrack {
                name: "b".into(),
                bus: "Stems".into(),
                track: MixTrack {
                    clip: clip(0.25),
                    items: vec![Item::whole(1.0)],
                    gain: 0.5,
                },
            },
        ];
        let timebase = Timebase {
            width: 2,
            height: 2,
            frame_rate: Rational::new(1, 1),
        };
        let info = RenderInfo {
            width: 2,
            height: 2,
            frame_rate: timebase.frame_rate,
            frames: 2,
            timebase,
        };
        // 2 s at 2 Hz on the stereo Main bus: only track `a` is routed to it. Each item's first
        // sample falls on its edge, where its fade in starts from silence ...
        let main = Bus::main();
        let mixed = mix(&tracks, None, &main, &info, 2);
        assert_eq!((mixed.sample_rate, mixed.channels), (2, 2));
        assert_eq!(*mixed.samples, [0.0, 0.0, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0]);
        // ... `b` plays on its own mono bus at its volume, from 1 s ...
        let stems = Bus {
            name: "Stems".into(),
            channels: 1,
        };
        let stem = mix(&tracks, None, &stems, &info, 2);
        assert_eq!(
            (stem.channels, &*stem.samples),
            (1, &[0.0, 0.0, 0.0, 0.125][..])
        );
        // ... and at full volume, on any bus, when passed through on its own.
        let alone = mix(&tracks, Some("b"), &main, &info, 2);
        assert_eq!(alone.samples[..6], [0.0; 6]);
        assert_eq!(alone.samples[6..], [0.25; 2]);
    }
}
