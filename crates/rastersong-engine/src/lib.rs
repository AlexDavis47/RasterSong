//! The RasterSong render service: sequential renderer, warmup on seek, frame cache,
//! cancellation and the playback clock.
//!
//! The CLI and GUI talk only to this crate; they never decode or schedule anything themselves.

pub mod audio;
mod cache;
mod clock;
mod compose;
mod composition;
mod error;
pub mod export;
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
pub use composition::{
    ComposedResource, ComposedTrack, Composition, InputKind, InputPort, input_ports, port_of,
};
pub use error::EngineError;
pub use listen::ListenTarget;
pub use offline::{FrameSink, RenderSettings, RenderedFrame, render};
pub use project::{
    DEFAULT_GRAPH_NAME, DEFAULT_INSPECT_RATE, DEFAULT_MAX_WARMUP_FRAMES, Edge, INSPECT_RATE_RANGE,
    ItemLengths, ItemRef, LoopRegion, MAX_WARMUP_FRAMES_LIMIT, MIN_ITEM_LENGTH, MediaFile,
    PASSTHROUGH_GRAPH, PROJECT_EXTENSION, PROJECT_VERSION, Project, ProjectTrack, RATE_RANGE,
    Resource, ResourceContent, ResourceId, TimelineMode, resource_name_for, snap_offset,
};
pub use rastersong_graph::dsp::Fft;
pub use rastersong_graph::nodes::support::UNBOUNDED_WARMUP;
pub use rastersong_graph::nodes::{
    AUDIO_BUS_OUTPUT, AUDIO_INPUT, AUDIO_OUTPUT, BELOW, BUS_PARAM, CHANNEL_PORTS, COMBINE,
    DEFAULT_AUDIO, DEFAULT_BUS, DEFAULT_VIDEO, MAX_CHANNELS, NAME_PARAM, NAMED_INPUT, NAMED_OUTPUT,
    OUTPUT, SOURCE_PARAM, SPLIT, TRACK_PARAM, VIDEO_INPUT,
};
pub use rastersong_graph::{
    Category, ChannelMap, Channels, CompileOptions, Connection, Diagnostic, FORMAT_VERSION,
    GeneratorLayout, Graph, GraphDesc, GraphError, Grouping, Interpolation, Kind, Layout,
    MODULATION_AMOUNT_LIMITS, Meter, MeterKind, ModMode, Modulation, NodeCost, NodeDesc,
    NodeDiagnostic, NodeMeters, NodeStats, NodeType, OutputLevel, OutputSpec, ParamKind,
    ParamLevel, ParamSpec, ParamValue, Part, Range, Reading, Registry, Severity, ShownWhen, Tag,
    TagRule, Tempo, Timing, Unit, audio_output_bus, range_span, render_form,
};
pub use rastersong_media::{
    AudioCache, AudioClip, AudioOptions, AvailableFormat, BackendInfo, ExportFormat, FakeBackend,
    FakeVideo, FfmpegBackend, FormatKind, LibraryInfo, MediaBackend, MediaError, MediaWriter,
    ProResProfile, Quality, Rational, Samples, StreamInfo, StreamKind, Version, VideoFrame,
    VideoInfo, export_formats, lgpl_url,
};
pub use renderer::{
    AudioResources, DEFAULT_AUDIO_TRACK, OutputSize, RenderInfo, Renderer, VIDEO_SOURCE,
};
pub use service::{
    CacheUse, Engine, EngineConfig, EngineStatus, Failure, LoadedAudio, PreviewScale, RenderDelays,
    RenderProgress,
};
pub use tap::{PICTURE_SIDE, Picture, Tap, TapOutcome, TapRequest, picture_size};
pub use thumbnails::{THUMBNAIL_HEIGHT, Thumbnails, VideoKey};
pub use timeline::{Bus, Item, LabelColor, MAX_BUS_CHANNELS, Source, Stack, Timebase, TrackKind};
pub use waveform::Waveform;

/// Initializes the engine and its media backend, and reports what was loaded.
pub fn init() -> Result<BackendInfo, MediaError> {
    rastersong_media::init()?;
    Ok(rastersong_media::backend_info())
}
