//! Built-in nodes and the registry that creates nodes from graph files.

mod effects;
mod io;
mod structural;

use std::collections::BTreeMap;

pub use effects::{Am, Bitcrush, Delay, LengthUnit, Lowpass, ThreeBand};
pub use io::{Output, SourceNode};
pub use structural::{Combine, Interleave, Pack, Split};

use crate::Node;
use crate::desc::Params;

/// The node type name of the graph's output node.
pub const OUTPUT: &str = "output";

type Constructor = Box<dyn Fn(&mut Params) -> Result<Box<dyn Node>, String> + Send + Sync>;

/// Maps node type names (as written in graph files) to constructors.
pub struct Registry {
    constructors: BTreeMap<String, Constructor>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.constructors.keys()).finish()
    }
}

impl Registry {
    pub fn empty() -> Self {
        Self {
            constructors: BTreeMap::new(),
        }
    }

    pub fn register<N: Node + 'static>(
        &mut self,
        kind: &str,
        constructor: impl Fn(&mut Params) -> Result<N, String> + Send + Sync + 'static,
    ) -> &mut Self {
        self.constructors.insert(
            kind.to_owned(),
            Box::new(move |params| Ok(Box::new(constructor(params)?) as Box<dyn Node>)),
        );
        self
    }

    pub fn kinds(&self) -> impl Iterator<Item = &str> {
        self.constructors.keys().map(String::as_str)
    }

    /// Creates a node, or `None` if the type is unknown.
    pub fn create(&self, kind: &str, params: &mut Params) -> Option<Result<Box<dyn Node>, String>> {
        self.constructors.get(kind).map(|c| c(params))
    }
}

impl Default for Registry {
    /// All built-in nodes.
    fn default() -> Self {
        let mut registry = Self::empty();
        registry
            .register("video_input", SourceNode::video)
            .register("audio_input", SourceNode::audio)
            .register(OUTPUT, |_| Ok(Output))
            .register("split", |_| Ok(Split))
            .register("combine", |_| Ok(Combine))
            .register("interleave", |_| Ok(Interleave))
            .register("pack", |_| Ok(Pack))
            .register("three_band", ThreeBand::new)
            .register("am", Am::new)
            .register("delay", Delay::new)
            .register("bitcrush", Bitcrush::new)
            .register("lowpass", Lowpass::new);
        registry
    }
}
