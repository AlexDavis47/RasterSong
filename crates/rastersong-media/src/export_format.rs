//! The file formats RasterSong exports to. Each preset names a container, a video codec and an
//! audio codec; which encoder implements it is decided by the backend when the file is opened.

use rastersong_lang::{tr, tr_args};

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

    /// The video codec's name, the same in every language.
    pub fn codec_name(self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::ProRes => "ProRes",
            Self::Ffv1 => "FFV1",
        }
    }

    /// Whether the frame width and height must be even (formats with subsampled chroma).
    pub fn needs_even_size(self) -> bool {
        !matches!(self, Self::Ffv1)
    }

    /// The size closest to `width` × `height` this format can encode: one pixel more on an odd
    /// side where sizes must be even.
    pub fn nearest_size(self, width: u32, height: u32) -> (u32, u32) {
        if self.needs_even_size() {
            (width.next_multiple_of(2), height.next_multiple_of(2))
        } else {
            (width, height)
        }
    }

    /// Why this format can't encode `width` × `height`, saying which side is odd and the
    /// nearest size that works; `None` when it can.
    pub fn size_problem(self, width: u32, height: u32) -> Option<String> {
        let (even_width, even_height) = self.nearest_size(width, height);
        let side = match (even_width != width, even_height != height) {
            (false, false) => return None,
            (true, false) => tr("export.odd_side.width"),
            (false, true) => tr("export.odd_side.height"),
            (true, true) => tr("export.odd_side.both"),
        };
        Some(tr_args(
            "export.odd_size",
            &[
                ("format", self.codec_name()),
                ("width", &width.to_string()),
                ("height", &height.to_string()),
                ("side", side),
                ("even_width", &even_width.to_string()),
                ("even_height", &even_height.to_string()),
            ],
        ))
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
    fn odd_sizes_name_the_odd_side_and_the_size_that_works() {
        assert_eq!(FormatKind::H264.size_problem(1920, 1080), None);
        assert_eq!(FormatKind::Ffv1.size_problem(1919, 1079), None);
        let message = FormatKind::ProRes.size_problem(1919, 1080).unwrap();
        assert!(message.contains("1919×1080"), "{message}");
        assert!(message.contains("odd width"), "{message}");
        assert!(message.contains("1920×1080"), "{message}");
        assert_eq!(FormatKind::H264.nearest_size(33, 17), (34, 18));
    }

    #[test]
    fn default_formats_keep_their_kind() {
        for kind in FormatKind::ALL {
            assert_eq!(kind.default_format().kind(), kind);
        }
    }
}
