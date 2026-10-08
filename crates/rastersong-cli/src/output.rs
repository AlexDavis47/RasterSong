//! Render outputs: a lossless `.mkv` file or a PNG sequence.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use rastersong_engine::playback::{MixTrack, Mixer};
use rastersong_engine::{
    AudioClip, AudioSink, EngineError, FrameSink, LosslessWriter, RenderInfo, RenderedFrame,
};

/// An audio track the sound of the output can be made of, when the graph renders none.
#[derive(Debug, Clone)]
pub struct SourceTrack {
    pub name: String,
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
        writer: Option<Box<LosslessWriter>>,
    },
    Png {
        dir: PathBuf,
        size: (u32, u32),
    },
}

impl Output {
    /// A `.mkv` path writes a video file with sound (the graph's, or else the mix of `tracks`,
    /// stereo at `audio_rate`); anything else is a directory for PNG frames.
    pub fn new(path: &Path, tracks: Vec<SourceTrack>, audio_rate: u32) -> Self {
        let is_mkv = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mkv"));
        if is_mkv {
            Self::Mkv {
                path: path.to_owned(),
                tracks,
                audio_rate,
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
                    // The tracks as placed on the timeline: all of them mixed, or the one
                    // passed through.
                    AudioSink::Source | AudioSink::Passthrough(_) => {
                        let only = match sink {
                            AudioSink::Passthrough(name) => Some(name.as_str()),
                            _ => None,
                        };
                        let mixed = mix(tracks, only, info, *audio_rate);
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

/// The project's length of `tracks` (or only the one named `only`, at full volume) mixed to
/// stereo at `rate`.
fn mix(tracks: &[SourceTrack], only: Option<&str>, info: &RenderInfo, rate: u32) -> AudioClip {
    let tracks = tracks
        .iter()
        .filter(|t| only.is_none_or(|name| t.name == name))
        .map(|t| MixTrack {
            gain: if only.is_some() { 1.0 } else { t.track.gain },
            ..t.track.clone()
        })
        .collect();
    let seconds = info.frames as f64 / info.frame_rate.as_f64();
    let mut samples = vec![0.0; (seconds * f64::from(rate)).round() as usize * 2];
    Mixer::new(tracks).render(0.0, f64::from(rate), &mut samples);
    AudioClip {
        sample_rate: rate,
        channels: 2,
        samples,
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
    fn the_mix_follows_the_timeline_and_passthrough_picks_one_track() {
        let clip = |value| {
            Arc::new(AudioClip {
                sample_rate: 4,
                channels: 1,
                samples: vec![value; 4],
            })
        };
        let tracks = vec![
            SourceTrack {
                name: "a".into(),
                track: MixTrack {
                    clip: clip(0.5),
                    items: vec![Item::whole(0.0)],
                    gain: 1.0,
                },
            },
            SourceTrack {
                name: "b".into(),
                track: MixTrack {
                    clip: clip(0.25),
                    items: vec![Item::whole(1.0)],
                    gain: 0.0,
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
        // 2 s at 2 Hz, stereo. Track `b` is silent in the mix (gain 0) ...
        let mixed = mix(&tracks, None, &info, 2);
        assert_eq!((mixed.sample_rate, mixed.channels), (2, 2));
        assert_eq!(mixed.samples[..4], [0.5; 4]);
        assert_eq!(mixed.samples[4..], [0.0; 4]);
        // ... and at full volume, from 1 s, when passed through on its own.
        let alone = mix(&tracks, Some("b"), &info, 2);
        assert_eq!(alone.samples[..4], [0.0; 4]);
        assert_eq!(alone.samples[4..6], [0.25; 2]);
    }
}
