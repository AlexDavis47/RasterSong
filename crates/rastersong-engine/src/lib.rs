//! The RasterSong render service: sequential renderer, warmup on seek, frame cache,
//! cancellation and the playback clock.
//!
//! The CLI and GUI talk only to this crate; they never decode or schedule anything themselves.

pub mod audio;
mod cache;
mod clock;
mod error;
mod listen;
mod offline;
pub mod playback;
mod project;
mod renderer;
mod service;
pub mod sources;
mod tap;
mod thumbnails;
pub mod timeline;
pub mod waveform;

pub use audio::{AudioBlock, AudioSink, DEFAULT_AUDIO_RATE};
pub use cache::{CacheKey, Frame, FrameCache};
pub use clock::PlaybackClock;
pub use error::EngineError;
pub use listen::ListenTarget;
pub use offline::{FrameSink, RenderSettings, RenderedFrame, render};
pub use project::{
    Binding, DEFAULT_INSPECT_RATE, DEFAULT_MAX_WARMUP_FRAMES, Edge, GraphEntry, GraphItem,
    GraphLayer, INSPECT_RATE_RANGE, InputKind, InputPort, ItemRef, LayerSet, LoopRegion,
    MAX_WARMUP_FRAMES_LIMIT, MIN_ITEM_LENGTH, PASSTHROUGH_GRAPH, PROJECT_EXTENSION,
    PROJECT_VERSION, Project, ProjectTrack, RATE_RANGE, RenderItem, Resource, ResourceId,
    ResourceKind, StoredGraph, TimelineMode, input_ports, resource_name_for, snap_offset,
};
pub use rastersong_graph::dsp::Fft;
pub use rastersong_graph::nodes::support::UNBOUNDED_WARMUP;
pub use rastersong_graph::nodes::{
    AUDIO_INPUT, AUDIO_OUTPUT, BUS_PARAM, CHANNEL_PORTS, COMBINE, DEFAULT_BUS, LAYER_BELOW_SOURCE,
    MAX_CHANNELS, NO_SOURCE, OUTPUT, SOURCE_PARAM, SPLIT, VIDEO_INPUT,
};
pub use rastersong_graph::{
    Category, ChannelMap, Channels, CompileOptions, Connection, Diagnostic, FORMAT_VERSION,
    GeneratorLayout, Graph, GraphDesc, GraphError, Grouping, Interpolation, Kind, Layout,
    MODULATION_AMOUNT_LIMITS, Meter, MeterKind, ModMode, Modulation, NodeCost, NodeDesc,
    NodeDiagnostic, NodeMeters, NodeStats, NodeType, OutputLevel, OutputSpec, ParamKind,
    ParamLevel, ParamSpec, ParamValue, Part, Range, Registry, Severity, ShownWhen, Tag, TagRule,
    Tempo, range_span, render_form,
};
pub use rastersong_media::{
    AudioCache, AudioClip, AudioOptions, BackendInfo, FakeBackend, FakeVideo, FfmpegBackend,
    LibraryInfo, LosslessWriter, MediaBackend, MediaError, Rational, Samples, StreamInfo,
    StreamKind, Version, VideoFrame, VideoInfo,
};
pub use renderer::{
    DEFAULT_AUDIO_TRACK, OutputSize, RenderInfo, RenderTrack, Renderer, TrackMedia, VIDEO_SOURCE,
};
pub use service::{
    Engine, EngineConfig, EngineStatus, Failure, LoadedTrack, PreviewScale, RenderProgress,
};
pub use tap::{PICTURE_SIDE, Picture, Tap, TapOutcome, TapRequest, picture_size};
pub use thumbnails::{THUMBNAIL_HEIGHT, Thumbnails, VideoKey};
pub use timeline::{Bus, Item, MAX_BUS_CHANNELS, Timebase, Timeline, TrackKind, TrackSpec};
pub use waveform::Waveform;

/// Initializes the engine and its media backend, and reports what was loaded.
pub fn init() -> Result<BackendInfo, MediaError> {
    rastersong_media::init()?;
    Ok(rastersong_media::backend_info())
}
