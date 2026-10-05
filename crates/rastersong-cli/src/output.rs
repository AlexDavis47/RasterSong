//! Render outputs: a lossless `.mkv` file or a PNG sequence.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use rastersong_engine::{
    AudioClip, AudioSink, EngineError, FrameSink, LosslessWriter, RenderInfo, RenderedFrame,
};

pub enum Output<'a> {
    /// Waiting for `start` to learn the frame size and rate.
    Mkv {
        path: PathBuf,
        /// The source audio, written when the graph doesn't render its own.
        audio: &'a AudioClip,
        /// Seconds the source audio starts after the video.
        offset: f64,
        writer: Option<LosslessWriter>,
    },
    Png {
        dir: PathBuf,
        size: (u32, u32),
    },
}

impl<'a> Output<'a> {
    /// A `.mkv` path writes a video file with sound (the graph's, or else `audio` starting
    /// `offset` seconds after the video); anything else is a directory for PNG frames.
    pub fn new(path: &Path, audio: &'a AudioClip, offset: f64) -> Self {
        let is_mkv = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mkv"));
        if is_mkv {
            Self::Mkv {
                path: path.to_owned(),
                audio,
                offset,
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

impl FrameSink for Output<'_> {
    fn start(&mut self, info: &RenderInfo, sink: &AudioSink) -> Result<(), EngineError> {
        match self {
            Self::Mkv {
                path,
                audio,
                offset,
                writer,
            } => {
                *writer = Some(match sink {
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
                    // The source track, untouched. The CLI has one track, so a track passed
                    // through is that one.
                    AudioSink::Source | AudioSink::Passthrough(_) => LosslessWriter::create(
                        path,
                        info.width,
                        info.height,
                        info.frame_rate,
                        Some(&shifted(audio, *offset)),
                    )?,
                });
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

/// `clip` starting `offset` seconds later: silence first for a positive offset, the start cut
/// off for a negative one.
fn shifted(clip: &AudioClip, offset: f64) -> AudioClip {
    let channels = clip.channels.max(1) as usize;
    let frames = (offset.abs() * f64::from(clip.sample_rate)).round() as usize;
    let samples = if offset >= 0.0 {
        let mut samples = vec![0.0; frames * channels];
        samples.extend_from_slice(&clip.samples);
        samples
    } else {
        clip.samples
            .get(frames * channels..)
            .unwrap_or_default()
            .to_vec()
    };
    AudioClip {
        samples,
        ..clip.clone()
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
    use super::*;

    #[test]
    fn shifting_pads_or_trims_the_start() {
        let clip = AudioClip {
            sample_rate: 4,
            channels: 2,
            samples: vec![1.0, -1.0, 2.0, -2.0, 3.0, -3.0],
        };
        assert_eq!(
            shifted(&clip, 0.5).samples,
            [0.0, 0.0, 0.0, 0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0]
        );
        assert_eq!(shifted(&clip, -0.25).samples, [2.0, -2.0, 3.0, -3.0]);
        assert!(shifted(&clip, -10.0).samples.is_empty());
        assert_eq!(shifted(&clip, 0.0), clip);
    }
}
