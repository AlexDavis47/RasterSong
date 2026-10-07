//! The RasterSong processing graph: the [`Signal`] type, the [`Node`] contract, the compiled
//! sequential schedule ([`Graph`]) and the built-in nodes.
//!
//! Pure Rust with no FFmpeg dependency, so everything here is testable without media files.

mod desc;
pub mod dsp;
mod error;
mod graph;
mod migrate;
mod node;
pub mod nodes;
mod params;
mod signal;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use desc::{
    Channels, Connection, FORMAT_VERSION, GeneratorLayout, GraphDesc, Grouping, Interpolation,
    MODULATION_AMOUNT_LIMITS, ModMode, Modulation, NodeDesc, ParamValue,
};
pub use error::GraphError;
pub use graph::{
    CompileOptions, Graph, MAX_INPUTS, MAX_PARAMS, NodeDiagnostic, NodeStats, OutputLevel,
    ParamLevel, render_form,
};
pub use node::{
    Diagnostic, InputSpec, LayoutContext, Node, OutputSpec, PrepareContext, ProcessContext,
    Severity, Sources, Tempo, Value,
};
pub use nodes::{Category, Choice, NodeKind, NodeSpec, NodeType, Registry};
pub use params::{ParamKind, ParamSpec, Params, range_span};
pub use signal::{ChannelMap, Kind, Layout, Part, Range, Signal, Tag, TagRule};
