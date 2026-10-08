//! A media backend that serves synthetic media registered by path, for testing everything
//! above the media layer without FFmpeg or media files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{
    AudioClip, AudioOptions, MediaBackend, MediaError, Rational, Rotation, StreamInfo, StreamKind,
    VideoFrame, VideoInfo, VideoSource,
};

#[derive(Debug, Default)]
pub struct FakeBackend {
    videos: HashMap<PathBuf, FakeVideo>,
    audio: HashMap<PathBuf, AudioClip>,
    streams: HashMap<PathBuf, Vec<StreamInfo>>,
}

/// A synthetic video. Every pixel of frame `i` at `(x, y)` is [`FakeVideo::pixel`]`(i, x, y)`,
/// so tests can tell exactly which frame they received.
#[derive(Debug, Clone, Copy)]
pub struct FakeVideo {
    pub width: u32,
    pub height: u32,
    pub frame_count: usize,
    pub frame_rate: Rational,
}

impl FakeVideo {
    /// The color of `(x, y)` in frame `index`: red and green hold the frame index
    /// (`index % 256`, `index / 256 % 256`), blue varies across the image.
    pub fn pixel(index: usize, x: u32, y: u32) -> [u8; 3] {
        [index as u8, (index >> 8) as u8, (x + y) as u8]
    }
}

impl FakeBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_video(mut self, path: impl Into<PathBuf>, video: FakeVideo) -> Self {
        self.videos.insert(path.into(), video);
        self
    }

    /// Registers a clip that [`MediaBackend::load_audio`] returns as-is. Resampling and remixing
    /// options are not supported and must be left unset.
    pub fn with_audio(mut self, path: impl Into<PathBuf>, clip: AudioClip) -> Self {
        self.audio.insert(path.into(), clip);
        self
    }

    /// Makes [`MediaBackend::streams`] list `streams` for `path`. Without it a registered video
    /// is stream 0 and registered audio the next. Every video stream opens the registered
    /// video and every audio stream the registered clip.
    pub fn with_streams(mut self, path: impl Into<PathBuf>, streams: Vec<StreamInfo>) -> Self {
        self.streams.insert(path.into(), streams);
        self
    }

    fn not_registered(path: &Path) -> MediaError {
        MediaError::Open {
            path: path.to_owned(),
            reason: "not registered with the fake backend".into(),
        }
    }
}

impl MediaBackend for FakeBackend {
    fn open_video_stream(
        &self,
        path: &Path,
        _stream: Option<usize>,
    ) -> Result<Box<dyn VideoSource>, MediaError> {
        let video = self
            .videos
            .get(path)
            .ok_or_else(|| Self::not_registered(path))?;
        Ok(Box::new(FakeVideoSource {
            info: VideoInfo {
                width: video.width,
                height: video.height,
                frame_count: video.frame_count,
                frame_rate: video.frame_rate,
                rotation: Rotation::None,
            },
            output_size: None,
        }))
    }

    fn streams(&self, path: &Path) -> Result<Vec<StreamInfo>, MediaError> {
        if let Some(streams) = self.streams.get(path) {
            return Ok(streams.clone());
        }
        let video = self.videos.get(path).map(|v| StreamKind::Video {
            width: v.width,
            height: v.height,
            frame_rate: v.frame_rate.as_f64(),
        });
        let audio = self.audio.get(path).map(|a| StreamKind::Audio {
            sample_rate: a.sample_rate,
            channels: a.channels,
        });
        if video.is_none() && audio.is_none() {
            return Err(Self::not_registered(path));
        }
        Ok(video
            .into_iter()
            .chain(audio)
            .enumerate()
            .map(|(index, kind)| StreamInfo {
                index,
                kind,
                codec: "fake".into(),
                title: None,
                language: None,
                default: true,
            })
            .collect())
    }

    fn has_audio(&self, path: &Path) -> bool {
        self.audio.contains_key(path)
    }

    fn load_audio(&self, path: &Path, options: AudioOptions) -> Result<AudioClip, MediaError> {
        assert_eq!(
            AudioOptions {
                stream: None,
                ..options
            },
            AudioOptions::default(),
            "the fake backend does not resample or remix"
        );
        self.audio
            .get(path)
            .cloned()
            .ok_or_else(|| Self::not_registered(path))
    }
}

#[derive(Debug)]
struct FakeVideoSource {
    info: VideoInfo,
    output_size: Option<(u32, u32)>,
}

impl VideoSource for FakeVideoSource {
    fn info(&self) -> &VideoInfo {
        &self.info
    }

    fn frame_time(&self, index: usize) -> f64 {
        index as f64 / self.info.frame_rate.as_f64()
    }

    fn set_output_size(&mut self, size: Option<(u32, u32)>) {
        self.output_size = size;
    }

    fn frame(&mut self, index: usize) -> Result<Arc<VideoFrame>, MediaError> {
        if index >= self.info.frame_count {
            return Err(MediaError::FrameOutOfRange {
                index,
                count: self.info.frame_count,
            });
        }
        let (width, height) = self
            .output_size
            .unwrap_or((self.info.width, self.info.height));
        let mut data = Vec::with_capacity(width as usize * height as usize * 3);
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&FakeVideo::pixel(index, x, y));
            }
        }
        Ok(Arc::new(VideoFrame {
            width,
            height,
            data,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend() -> FakeBackend {
        FakeBackend::new().with_video(
            "clip.mp4",
            FakeVideo {
                width: 8,
                height: 4,
                frame_count: 300,
                frame_rate: Rational::new(30, 1),
            },
        )
    }

    #[test]
    fn frames_encode_their_index() {
        let mut video = backend().open_video(Path::new("clip.mp4")).unwrap();
        for i in [0, 1, 255, 256, 299] {
            let frame = video.frame(i).unwrap();
            assert_eq!(frame.pixel(3, 2), FakeVideo::pixel(i, 3, 2));
        }
        assert!(matches!(
            video.frame(300),
            Err(MediaError::FrameOutOfRange { .. })
        ));
    }

    #[test]
    fn output_size_and_timing() {
        let mut video = backend().open_video(Path::new("clip.mp4")).unwrap();
        video.set_output_size(Some((4, 2)));
        let frame = video.frame(0).unwrap();
        assert_eq!((frame.width, frame.height, frame.data.len()), (4, 2, 24));
        assert_eq!(video.frame_time(45), 1.5);
    }

    #[test]
    fn unknown_paths_fail_to_open() {
        assert!(matches!(
            backend().open_video(Path::new("other.mp4")),
            Err(MediaError::Open { .. })
        ));
    }
}
