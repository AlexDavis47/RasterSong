use std::path::PathBuf;

#[derive(Debug, Clone, thiserror::Error)]
pub enum MediaError {
    #[error("FFmpeg failed to initialize: {0}")]
    Init(String),

    #[error("could not open {}: {reason}", path.display())]
    Open { path: PathBuf, reason: String },

    #[error("{} has no video stream", .0.display())]
    NoVideoStream(PathBuf),

    #[error("{} has no audio stream", .0.display())]
    NoAudioStream(PathBuf),

    #[error("frame {index} is out of range (the video has {count} frames)")]
    FrameOutOfRange { index: usize, count: usize },

    /// The frame is listed in the file but the decoder did not produce it (e.g. corrupt or truncated data).
    #[error("frame {0} could not be decoded")]
    FrameUnavailable(usize),

    #[error("decoding failed: {0}")]
    Decode(String),
}
