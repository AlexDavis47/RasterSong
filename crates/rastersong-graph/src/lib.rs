//! The RasterSong processing graph: the [`Signal`] type, the [`Node`] contract, the compiled
//! sequential schedule ([`Graph`]) and the built-in nodes.
//!
//! Pure Rust with no FFmpeg dependency, so everything here is testable without media files.

mod desc;
pub mod dsp;
mod error;
mod graph;
mod node;
pub mod nodes;
mod params;
mod signal;

pub use desc::{
    Channels, Connection, FORMAT_VERSION, GraphDesc, Interpolation, NodeDesc, ParamValue,
};
pub use error::GraphError;
pub use graph::{CompileOptions, Graph, MAX_INPUTS, OutputLevel};
pub use node::{InputSpec, LayoutContext, Node, PrepareContext, ProcessContext, Sources};
pub use nodes::{Category, NodeSpec, NodeType, Registry};
pub use params::{ParamKind, ParamSpec, Params};
pub use signal::{Layout, Signal};
