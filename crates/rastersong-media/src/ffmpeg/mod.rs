//! The FFmpeg backend. All FFmpeg types stay inside this module.

mod audio;
mod index;
mod scale;
mod video;
mod writer;

use std::path::Path;
use std::sync::OnceLock;

use ffmpeg_next as ffmpeg;

pub use writer::LosslessWriter;

use crate::{
    AudioClip, AudioOptions, MediaBackend, MediaError, StreamInfo, StreamKind, VideoSource,
};

#[derive(Debug)]
pub struct FfmpegBackend {
    _initialized: (),
}

impl FfmpegBackend {
    pub fn new() -> Result<Self, MediaError> {
        init()?;
        Ok(Self { _initialized: () })
    }
}

impl MediaBackend for FfmpegBackend {
    fn open_video_stream(
        &self,
        path: &Path,
        stream: Option<usize>,
    ) -> Result<Box<dyn VideoSource>, MediaError> {
        Ok(Box::new(video::FfmpegVideoSource::open(path, stream)?))
    }

    fn streams(&self, path: &Path) -> Result<Vec<StreamInfo>, MediaError> {
        let input = open_input(path)?;
        Ok(input.streams().filter_map(|s| stream_info(&s)).collect())
    }

    fn load_audio(&self, path: &Path, options: AudioOptions) -> Result<AudioClip, MediaError> {
        audio::load_audio(path, options)
    }

    fn decode_audio(
        &self,
        path: &Path,
        options: AudioOptions,
        sink: &mut dyn FnMut(&[f32]) -> Result<(), MediaError>,
    ) -> Result<(u32, u32), MediaError> {
        audio::decode_audio(path, options, sink)
    }

    fn has_audio(&self, path: &Path) -> bool {
        open_input(path)
            .is_ok_and(|input| input.streams().best(ffmpeg::media::Type::Audio).is_some())
    }
}

/// Initializes FFmpeg. Safe to call more than once; only the first call does any work.
pub fn init() -> Result<(), MediaError> {
    static INIT: OnceLock<Result<(), MediaError>> = OnceLock::new();
    INIT.get_or_init(|| {
        ffmpeg::init().map_err(|e| MediaError::Init(e.to_string()))?;
        // FFmpeg logs to stderr directly. Expected situations (e.g. undecodable leading frames
        // after a seek) produce warnings, so only show errors.
        ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Error);
        let info = backend_info();
        tracing::info!(
            avcodec = %info.library("avcodec").version,
            license = info.license(),
            "FFmpeg initialized"
        );
        Ok(())
    })
    .clone()
}

fn open_input(path: &Path) -> Result<ffmpeg::format::context::Input, MediaError> {
    ffmpeg::format::input(path).map_err(|e| MediaError::Open {
        path: path.to_owned(),
        reason: e.to_string(),
    })
}

/// Stream `index` of `input` if it is of `kind`, or the best stream of `kind` for `None`.
fn find_stream(
    input: &ffmpeg::format::context::Input,
    kind: ffmpeg::media::Type,
    index: Option<usize>,
) -> Option<ffmpeg::format::stream::Stream<'_>> {
    match index {
        None => input.streams().best(kind),
        Some(index) => input
            .stream(index)
            .filter(|s| s.parameters().medium() == kind),
    }
}

/// What the import dialog lists about `stream`: `None` for anything but a video or audio
/// stream, and for cover pictures.
fn stream_info(stream: &ffmpeg::format::stream::Stream<'_>) -> Option<StreamInfo> {
    use ffmpeg::format::stream::Disposition;
    let disposition = stream.disposition();
    if disposition.contains(Disposition::ATTACHED_PIC) {
        return None;
    }
    let parameters = stream.parameters();
    let context = ffmpeg::codec::context::Context::from_parameters(parameters.clone()).ok()?;
    let kind = match parameters.medium() {
        ffmpeg::media::Type::Video => {
            let video = context.decoder().video().ok()?;
            let rate = stream.avg_frame_rate();
            StreamKind::Video {
                width: video.width(),
                height: video.height(),
                frame_rate: if rate.denominator() == 0 {
                    0.0
                } else {
                    f64::from(rate)
                },
            }
        }
        ffmpeg::media::Type::Audio => {
            let audio = context.decoder().audio().ok()?;
            StreamKind::Audio {
                sample_rate: audio.rate(),
                channels: u32::from(audio.channels()),
            }
        }
        _ => return None,
    };
    let metadata = stream.metadata();
    let tag = |key: &str| {
        metadata
            .get(key)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    Some(StreamInfo {
        index: stream.index(),
        kind,
        codec: parameters.id().name().to_owned(),
        title: tag("title"),
        language: tag("language").filter(|l| l != "und"),
        default: disposition.contains(Disposition::DEFAULT),
    })
}

fn decode_error(e: ffmpeg::Error) -> MediaError {
    MediaError::Decode(e.to_string())
}

/// Versions and licenses of the FFmpeg libraries loaded at runtime.
#[derive(Debug, Clone)]
pub struct BackendInfo {
    pub libraries: Vec<LibraryInfo>,
    /// The `./configure` line FFmpeg was built with.
    pub configuration: &'static str,
}

#[derive(Debug, Clone)]
pub struct LibraryInfo {
    pub name: &'static str,
    pub version: Version,
    /// Major version the bindings were compiled against. A different major version at runtime
    /// means the wrong shared library was loaded.
    pub compiled_major: u32,
    pub license: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub micro: u32,
}

impl Version {
    /// Decodes FFmpeg's `AV_VERSION_INT` packing.
    fn from_av(v: u32) -> Self {
        Self {
            major: v >> 16,
            minor: (v >> 8) & 0xff,
            micro: v & 0xff,
        }
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.micro)
    }
}

impl BackendInfo {
    /// Looks up one of the libraries listed in [`backend_info`] by name, e.g. `"avcodec"`.
    pub fn library(&self, name: &str) -> &LibraryInfo {
        self.libraries
            .iter()
            .find(|l| l.name == name)
            .expect("unknown FFmpeg library")
    }

    /// The license of the loaded build, as reported by libavutil.
    pub fn license(&self) -> &'static str {
        self.library("avutil").license
    }

    /// Where the text of the loaded build's LGPL version is published.
    pub fn license_url(&self) -> &'static str {
        lgpl_url(self.license())
    }

    /// True when every loaded library is LGPL and the build enables no GPL or non-free components.
    pub fn is_lgpl(&self) -> bool {
        self.libraries.iter().all(|l| l.license.starts_with("LGPL"))
            && !self.configuration.contains("--enable-gpl")
            && !self.configuration.contains("--enable-nonfree")
    }
}

/// Where the text of the LGPL version named in an FFmpeg license string (as libavutil reports it,
/// e.g. `"LGPL version 2.1 or later"`) is published. Builds configured with `--enable-version3`
/// are LGPL 3; every other LGPL build, the release FFmpeg among them, is LGPL 2.1 or later.
pub fn lgpl_url(license: &str) -> &'static str {
    if license.contains("version 3") {
        "https://www.gnu.org/licenses/lgpl-3.0.html"
    } else {
        "https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html"
    }
}

pub fn backend_info() -> BackendInfo {
    use ffmpeg::ffi;
    use ffmpeg::software::{resampling, scaling};

    // bindgen exposes the `LIB*_VERSION_MAJOR` macros as i32.
    let lib = |name, version, compiled_major: i32, license| LibraryInfo {
        name,
        version: Version::from_av(version),
        compiled_major: compiled_major as u32,
        license,
    };
    BackendInfo {
        libraries: vec![
            lib(
                "avutil",
                ffmpeg::util::version(),
                ffi::LIBAVUTIL_VERSION_MAJOR,
                ffmpeg::util::license(),
            ),
            lib(
                "avcodec",
                ffmpeg::codec::version(),
                ffi::LIBAVCODEC_VERSION_MAJOR,
                ffmpeg::codec::license(),
            ),
            lib(
                "avformat",
                ffmpeg::format::version(),
                ffi::LIBAVFORMAT_VERSION_MAJOR,
                ffmpeg::format::license(),
            ),
            lib(
                "swscale",
                scaling::version(),
                ffi::LIBSWSCALE_VERSION_MAJOR,
                scaling::license(),
            ),
            lib(
                "swresample",
                resampling::version(),
                ffi::LIBSWRESAMPLE_VERSION_MAJOR,
                resampling::license(),
            ),
        ],
        configuration: ffmpeg::util::configuration(),
    }
}
