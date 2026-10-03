//! Media I/O for RasterSong: probing, frame index, video/audio decode and encode.
//!
//! FFmpeg lives entirely inside this crate. No FFmpeg type appears in its public API.

use std::sync::OnceLock;

use ffmpeg_next as ffmpeg;

#[derive(Debug, Clone, thiserror::Error)]
pub enum MediaError {
    #[error("FFmpeg failed to initialize: {0}")]
    Init(String),
}

/// Initializes the media backend. Safe to call more than once; only the first call does any work.
pub fn init() -> Result<(), MediaError> {
    static INIT: OnceLock<Result<(), MediaError>> = OnceLock::new();
    INIT.get_or_init(|| {
        ffmpeg::init().map_err(|e| MediaError::Init(e.to_string()))?;
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

    /// True when every loaded library is LGPL and the build enables no GPL or non-free components.
    pub fn is_lgpl(&self) -> bool {
        self.libraries.iter().all(|l| l.license.starts_with("LGPL"))
            && !self.configuration.contains("--enable-gpl")
            && !self.configuration.contains("--enable-nonfree")
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
