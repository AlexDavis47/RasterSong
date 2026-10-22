//! The file formats RasterSong exports to. Each preset names a container, a video codec and an
//! audio codec; which encoder implements it is decided by the backend when the file is opened.

/// A format to write a render to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportFormat {
    /// H.264 in MP4 with AAC sound, through an OS or hardware encoder.
    H264 { quality: Quality },
    /// Apple ProRes in QuickTime with 24-bit PCM sound.
    ProRes { profile: ProResProfile },
    /// Lossless FFV1 in Matroska with 16-bit PCM sound.
    Ffv1,
}

/// How much data a lossy format may spend per pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Quality {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProResProfile {
    Proxy,
    Lt,
    Standard,
    #[default]
    Hq,
}

/// A family of [`ExportFormat`]s, without their settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormatKind {
    H264,
    ProRes,
    Ffv1,
}

impl FormatKind {
    pub const ALL: [Self; 3] = [Self::H264, Self::ProRes, Self::Ffv1];

    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::H264 => "mp4",
            Self::ProRes => "mov",
            Self::Ffv1 => "mkv",
        }
    }

    /// The format whose extension `ext` is (any case), with default settings.
    pub fn from_extension(ext: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|k| k.extension().eq_ignore_ascii_case(ext))
    }

    /// The format with default settings.
    pub fn default_format(self) -> ExportFormat {
        match self {
            Self::H264 => ExportFormat::H264 {
                quality: Quality::default(),
            },
            Self::ProRes => ExportFormat::ProRes {
                profile: ProResProfile::default(),
            },
            Self::Ffv1 => ExportFormat::Ffv1,
        }
    }

    /// Whether the frame width and height must be even (formats with subsampled chroma).
    pub fn needs_even_size(self) -> bool {
        !matches!(self, Self::Ffv1)
    }
}

impl ExportFormat {
    pub fn kind(self) -> FormatKind {
        match self {
            Self::H264 { .. } => FormatKind::H264,
            Self::ProRes { .. } => FormatKind::ProRes,
            Self::Ffv1 => FormatKind::Ffv1,
        }
    }
}

impl Quality {
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];

    /// Bits per pixel per frame, which sets the bit rate for the frame size and rate.
    pub fn bits_per_pixel(self) -> f64 {
        match self {
            Self::Low => 0.05,
            Self::Medium => 0.1,
            Self::High => 0.2,
        }
    }
}

impl ProResProfile {
    pub const ALL: [Self; 4] = [Self::Proxy, Self::Lt, Self::Standard, Self::Hq];
}

/// A format the loaded FFmpeg can write, with the encoder that writes its video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableFormat {
    pub kind: FormatKind,
    /// FFmpeg's name for the video encoder, e.g. `h264_nvenc`.
    pub video_encoder: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_map_back_to_their_format() {
        for kind in FormatKind::ALL {
            assert_eq!(FormatKind::from_extension(kind.extension()), Some(kind));
        }
        assert_eq!(FormatKind::from_extension("MP4"), Some(FormatKind::H264));
        assert_eq!(FormatKind::from_extension("png"), None);
    }

    #[test]
    fn default_formats_keep_their_kind() {
        for kind in FormatKind::ALL {
            assert_eq!(kind.default_format().kind(), kind);
        }
    }
}
