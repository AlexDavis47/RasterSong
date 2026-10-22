//! The PNG-sequence output. Video files go through [`rastersong_engine::export`].

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use rastersong_engine::{AudioSink, EngineError, FrameSink, RenderInfo, RenderedFrame};

/// Writes each frame as a PNG into a directory.
pub struct PngSequence {
    dir: PathBuf,
    size: (u32, u32),
}

impl PngSequence {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_owned(),
            size: (0, 0),
        }
    }
}

fn output_error(context: impl std::fmt::Display, e: impl std::fmt::Display) -> EngineError {
    EngineError::Output(format!("{context}: {e}"))
}

impl FrameSink for PngSequence {
    fn start(&mut self, info: &RenderInfo, _: &AudioSink) -> Result<(), EngineError> {
        fs::create_dir_all(&self.dir).map_err(|e| output_error(self.dir.display(), e))?;
        self.size = (info.width, info.height);
        Ok(())
    }

    fn frame(&mut self, frame: RenderedFrame) -> Result<(), EngineError> {
        let path = self.dir.join(png_name(frame.index));
        write_png(&path, self.size, frame.rgb).map_err(|e| output_error(path.display(), e))
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
