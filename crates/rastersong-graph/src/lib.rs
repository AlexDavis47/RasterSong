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
mod signal;

pub use desc::{
    Connection, FORMAT_VERSION, GraphDesc, Interpolation, NodeDesc, ParamValue, Params,
};
pub use error::GraphError;
pub use graph::{CompileOptions, Graph, MAX_INPUTS};
pub use node::{InputSpec, LayoutContext, Node, PrepareContext, ProcessContext, Sources};
pub use nodes::Registry;
pub use signal::{Layout, Signal};
