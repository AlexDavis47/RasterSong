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
    Channels, Connection, FORMAT_VERSION, GraphDesc, Interpolation, ModMode, Modulation, NodeDesc,
    ParamValue,
};
pub use error::GraphError;
pub use graph::{
    CompileOptions, Graph, MAX_INPUTS, MAX_PARAMS, NodeStats, OutputLevel, ParamLevel, render_form,
};
pub use node::{
    InputSpec, LayoutContext, Node, OutputSpec, PortHint, PrepareContext, ProcessContext, Sources,
    Tempo, Value,
};
pub use nodes::{Category, Choice, NodeKind, NodeSpec, NodeType, Registry};
pub use params::{ModScale, ParamKind, ParamSpec, Params};
pub use signal::{Layout, Signal};
