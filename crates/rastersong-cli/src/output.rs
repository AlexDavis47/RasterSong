//! Render outputs: a lossless `.mkv` file or a PNG sequence.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use rastersong_engine::{
    AudioClip, EngineError, FrameSink, LosslessWriter, RenderInfo, RenderedFrame,
};

pub enum Output<'a> {
    /// Waiting for `start` to learn the frame size and rate.
    Mkv {
        path: PathBuf,
        audio: &'a AudioClip,
        writer: Option<LosslessWriter>,
    },
    Png {
        dir: PathBuf,
        size: (u32, u32),
    },
}

impl<'a> Output<'a> {
    /// A `.mkv` path writes a video file with `audio` as its soundtrack; anything else is a
    /// directory for PNG frames.
    pub fn new(path: &Path, audio: &'a AudioClip) -> Self {
        let is_mkv = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mkv"));
        if is_mkv {
            Self::Mkv {
                path: path.to_owned(),
                audio,
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
    fn start(&mut self, info: &RenderInfo) -> Result<(), EngineError> {
        match self {
            Self::Mkv {
                path,
                audio,
                writer,
            } => {
                *writer = Some(LosslessWriter::create(
                    path,
                    info.width,
                    info.height,
                    info.frame_rate,
                    Some(audio),
                )?);
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
                Ok(writer.as_mut().expect("started").write_frame(frame.rgb)?)
            }
            Self::Png { dir, size } => {
                let path = dir.join(png_name(frame.index));
                write_png(&path, *size, frame.rgb).map_err(|e| output_error(path.display(), e))
            }
        }
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
