//! The RasterSong render service: sequential renderer, warmup on seek, frame cache,
//! cancellation and the playback clock.
//!
//! The CLI and GUI talk only to this crate; they never decode or schedule anything themselves.

pub mod audio;
mod cache;
mod clock;
mod error;
mod offline;
pub mod playback;
mod project;
mod renderer;
mod service;
pub mod sources;
mod tap;
mod thumbnails;
pub mod waveform;

pub use audio::{AudioBlock, AudioSink, DEFAULT_AUDIO_RATE};
pub use cache::{CacheKey, Frame, FrameCache};
pub use clock::PlaybackClock;
pub use error::EngineError;
pub use offline::{FrameSink, RenderSettings, RenderedFrame, render};
pub use project::{
    DEFAULT_MAX_WARMUP_FRAMES, LoopRegion, MAX_WARMUP_FRAMES_LIMIT, PROJECT_EXTENSION,
    PROJECT_VERSION, Project, ProjectTrack, TimelineMode,
};
pub use rastersong_graph::nodes::support::UNBOUNDED_WARMUP;
pub use rastersong_graph::nodes::{
    AUDIO_INPUT, CHANNEL_PORTS, COMBINE, MAX_CHANNELS, OUTPUT, SOURCE_PARAM, SPLIT, VIDEO_INPUT,
};
pub use rastersong_graph::{
    Category, ChannelMap, Channels, CompileOptions, Connection, Diagnostic, FORMAT_VERSION,
    GeneratorLayout, Graph, GraphDesc, GraphError, Grouping, Interpolation, Kind, Layout,
    MODULATION_AMOUNT_LIMITS, Meter, MeterKind, ModMode, Modulation, NodeCost, NodeDesc,
    NodeDiagnostic, NodeMeters, NodeStats, NodeType, OutputLevel, OutputSpec, ParamKind,
    ParamLevel, ParamSpec, ParamValue, Part, Range, Registry, Severity, ShownWhen, Tag, TagRule,
    Tempo, range_span,
};
pub use rastersong_media::{
    AudioClip, AudioOptions, BackendInfo, FakeBackend, FakeVideo, FfmpegBackend, LibraryInfo,
    LosslessWriter, MediaBackend, MediaError, Rational, Version, VideoFrame, VideoInfo,
};
pub use renderer::{
    AudioTrack, DEFAULT_AUDIO_TRACK, OutputSize, RenderInfo, Renderer, VIDEO_SOURCE,
};
pub use service::{
    AudioTrackSpec, Engine, EngineConfig, EngineStatus, Failure, LoadedTrack, PreviewScale,
    RenderProgress,
};
pub use tap::{PICTURE_SIDE, Picture, Tap, TapOutcome, TapRequest, picture_size};
pub use thumbnails::{THUMBNAIL_HEIGHT, Thumbnails};
pub use waveform::Waveform;

/// Initializes the engine and its media backend, and reports what was loaded.
pub fn init() -> Result<BackendInfo, MediaError> {
    rastersong_media::init()?;
    Ok(rastersong_media::backend_info())
}
