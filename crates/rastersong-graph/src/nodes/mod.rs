//! Built-in nodes and the registry that creates nodes from graph files.
//!
//! One node per file under a folder for its category, holding its spec, parameters,
//! implementation and tests. The registry below lists them all.

mod convert;
mod effect;
mod input;
mod output;
mod structure;

use std::collections::BTreeMap;

pub use convert::{Mapping, ToAudio, ToVideo};
pub use effect::{
    Am, Bitcrush, Compressor, Delay, Distortion, Gate, LengthUnit, Lowpass, Shape, ThreeBand,
};
pub use input::SourceNode;
pub use output::Output;
pub use structure::{Combine, Interleave, Pack, Split};

use crate::{InputSpec, Node, ParamSpec, ParamValue, Params, PortHint};

/// The node type name of the graph's output node.
pub const OUTPUT: &str = "output";

/// Groups node types in menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Input,
    Structure,
    Convert,
    Effect,
    Output,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Self::Input => "Inputs",
            Self::Structure => "Channels",
            Self::Convert => "Conversion",
            Self::Effect => "Effects",
            Self::Output => "Output",
        }
    }
}

/// How a node type presents itself; supplied when registering it.
#[derive(Debug, Clone, Copy)]
pub struct NodeSpec {
    pub label: &'static str,
    pub category: Category,
    pub description: &'static str,
    pub params: &'static [ParamSpec],
    /// Whether the node can process R, G and B separately ([`crate::Channels::Separate`]).
    /// True for effects, whose output has the same layout as their main input.
    pub per_channel: bool,
}

impl NodeSpec {
    pub const fn new(label: &'static str, category: Category) -> Self {
        Self {
            label,
            category,
            description: "",
            params: &[],
            per_channel: false,
        }
    }

    pub const fn per_channel(mut self) -> Self {
        self.per_channel = true;
        self
    }

    pub const fn describe(mut self, description: &'static str) -> Self {
        self.description = description;
        self
    }

    pub const fn params(mut self, params: &'static [ParamSpec]) -> Self {
        self.params = params;
        self
    }
}

/// Everything a UI needs to know about a node type.
#[derive(Debug, Clone)]
pub struct NodeType {
    pub kind: String,
    pub spec: NodeSpec,
    pub inputs: &'static [InputSpec],
    pub outputs: &'static [&'static str],
    pub output_hints: &'static [PortHint],
}

impl NodeType {
    /// What output `index` carries.
    pub fn output_hint(&self, index: usize) -> PortHint {
        self.output_hints
            .get(index)
            .copied()
            .unwrap_or(PortHint::Inherit)
    }
}

type Constructor = Box<dyn Fn(&Params) -> Result<Box<dyn Node>, String> + Send + Sync>;

struct Entry {
    info: NodeType,
    constructor: Constructor,
}

/// Maps node type names (as written in graph files) to constructors and descriptions.
pub struct Registry {
    entries: BTreeMap<String, Entry>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.entries.keys()).finish()
    }
}

impl Registry {
    pub fn empty() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Adds a node type. The constructor must succeed with every parameter at its default, which
    /// is how the registry learns the node's ports.
    pub fn register<N: Node + 'static>(
        &mut self,
        kind: &str,
        spec: NodeSpec,
        constructor: impl Fn(&Params) -> Result<N, String> + Send + Sync + 'static,
    ) -> &mut Self {
        let defaults = Params::new(spec.params, &EMPTY).expect("no values");
        let example = constructor(&defaults)
            .unwrap_or_else(|e| panic!("`{kind}` fails with default parameters: {e}"));
        let info = NodeType {
            kind: kind.to_owned(),
            spec,
            inputs: example.inputs(),
            outputs: example.outputs(),
            output_hints: example.output_hints(),
        };
        self.entries.insert(
            kind.to_owned(),
            Entry {
                info,
                constructor: Box::new(move |params| {
                    Ok(Box::new(constructor(params)?) as Box<dyn Node>)
                }),
            },
        );
        self
    }

    /// Every registered node type, ordered by category and then label.
    pub fn types(&self) -> Vec<&NodeType> {
        let mut types: Vec<&NodeType> = self.entries.values().map(|e| &e.info).collect();
        types.sort_by_key(|t| (t.spec.category, t.spec.label));
        types
    }

    pub fn get(&self, kind: &str) -> Option<&NodeType> {
        self.entries.get(kind).map(|e| &e.info)
    }

    /// Creates a node from its parameter values, or `None` if the type is unknown.
    pub fn create(
        &self,
        kind: &str,
        values: &BTreeMap<String, ParamValue>,
    ) -> Option<Result<Box<dyn Node>, String>> {
        let entry = self.entries.get(kind)?;
        Some(
            Params::new(entry.info.spec.params, values)
                .and_then(|params| (entry.constructor)(&params)),
        )
    }
}

static EMPTY: BTreeMap<String, ParamValue> = BTreeMap::new();

impl Default for Registry {
    /// All built-in nodes.
    fn default() -> Self {
        let mut registry = Self::empty();
        registry
            .register("video_input", SourceNode::VIDEO_SPEC, SourceNode::video)
            .register("audio_input", SourceNode::AUDIO_SPEC, SourceNode::audio)
            .register(OUTPUT, Output::SPEC, |_| Ok(Output))
            .register("split", Split::SPEC, |_| Ok(Split))
            .register("combine", Combine::SPEC, |_| Ok(Combine))
            .register("interleave", Interleave::SPEC, |_| Ok(Interleave))
            .register("pack", Pack::SPEC, |_| Ok(Pack))
            .register("to_audio", ToAudio::SPEC, ToAudio::new)
            .register("to_video", ToVideo::SPEC, ToVideo::new)
            .register("three_band", ThreeBand::SPEC, ThreeBand::new)
            .register("am", Am::SPEC, Am::new)
            .register("delay", Delay::SPEC, Delay::new)
            .register("bitcrush", Bitcrush::SPEC, Bitcrush::new)
            .register("lowpass", Lowpass::SPEC, Lowpass::new)
            .register("compressor", Compressor::SPEC, Compressor::new)
            .register("gate", Gate::SPEC, Gate::new)
            .register("distortion", Distortion::SPEC, Distortion::new);
        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::ParamKind;

    #[test]
    fn every_default_is_valid_and_every_type_is_described() {
        let registry = Registry::default();
        for t in registry.types() {
            assert!(
                !t.spec.description.is_empty(),
                "`{}` has no description",
                t.kind
            );
            for p in t.spec.params {
                assert!(
                    !p.help.is_empty(),
                    "`{}.{}` has no help text",
                    t.kind,
                    p.name
                );
                if let ParamKind::Number {
                    default,
                    min,
                    max,
                    limit_min,
                    limit_max,
                } = p.kind
                {
                    assert!(
                        (min..=max).contains(&default),
                        "`{}.{}` default out of range",
                        t.kind,
                        p.name
                    );
                    assert!(
                        limit_min <= min && max <= limit_max,
                        "`{}.{}` limits narrower than its range",
                        t.kind,
                        p.name
                    );
                }
                // Writing the default explicitly gives the same result as leaving it out.
                let values = BTreeMap::from([(p.name.to_owned(), p.default_value())]);
                assert!(
                    registry.create(&t.kind, &values).unwrap().is_ok(),
                    "`{}.{}`",
                    t.kind,
                    p.name
                );
            }
        }
    }

    #[test]
    fn describes_ports() {
        let registry = Registry::default();
        let delay = registry.get("delay").unwrap();
        assert_eq!(registry.get("am").unwrap().inputs.len(), 2);
        assert_eq!(delay.outputs, ["out"]);
        assert_eq!(registry.get("split").unwrap().outputs, ["r", "g", "b"]);
        let split = registry.get("split").unwrap();
        assert_eq!(split.output_hint(1), PortHint::Green);
        assert_eq!(delay.output_hint(0), PortHint::Inherit);
        assert_eq!(
            registry.get("audio_input").unwrap().output_hint(0),
            PortHint::Audio
        );
        assert!(registry.get("nope").is_none());
    }
}
