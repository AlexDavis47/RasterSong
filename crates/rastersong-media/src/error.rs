use std::path::PathBuf;

use rastersong_lang::tr_args;

/// What can go wrong reading media. The text comes from the lang files (`error.media.*`); the
/// reasons FFmpeg itself gives are passed through as it words them.
#[derive(Debug, Clone)]
pub enum MediaError {
    Init(String),
    Open {
        path: PathBuf,
        reason: String,
    },
    NoVideoStream(PathBuf),
    NoAudioStream(PathBuf),
    FrameOutOfRange {
        index: usize,
        count: usize,
    },
    /// The frame is listed in the file but the decoder did not produce it (e.g. corrupt or truncated data).
    FrameUnavailable(usize),
    Decode(String),
}

impl std::fmt::Display for MediaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Init(message) => tr_args("error.media.init", &[("message", message)]),
            Self::Open { path, reason } => tr_args(
                "error.media.open",
                &[("path", &path.display().to_string()), ("reason", reason)],
            ),
            Self::NoVideoStream(path) => tr_args(
                "error.media.no_video",
                &[("path", &path.display().to_string())],
            ),
            Self::NoAudioStream(path) => tr_args(
                "error.media.no_audio",
                &[("path", &path.display().to_string())],
            ),
            Self::FrameOutOfRange { index, count } => tr_args(
                "error.media.frame_out_of_range",
                &[("index", &index.to_string()), ("count", &count.to_string())],
            ),
            Self::FrameUnavailable(index) => tr_args(
                "error.media.frame_unavailable",
                &[("index", &index.to_string())],
            ),
            Self::Decode(message) => tr_args("error.media.decode", &[("message", message)]),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for MediaError {}
