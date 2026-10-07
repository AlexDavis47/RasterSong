//! Built-in nodes and the registry that creates nodes from graph files.
//!
//! **One file per node** holds everything about it: its [`NodeSpec`] (label, description, ports,
//! parameters with their help text), the implementation, its test configurations and its unit
//! tests. To add a node, create `<category>/<name>.rs` implementing [`NodeKind`] and add one line
//! to the [`nodes!`] list below. Menus, the inspector, the property tests, the benchmarks and the
//! generated reference (`cargo xtask docs`) all read the registry, so nothing else needs editing.

#[macro_use]
mod define;
pub mod support;

use std::collections::BTreeMap;

pub use support::{SampleClock, Unit};

use rastersong_lang::tr;

use crate::graph::{MAX_INPUTS, MAX_PARAMS};
use crate::{InputSpec, Node, OutputSpec, ParamSpec, ParamValue, Params, Range, TagRule};

/// The node type name of the graph's output node.
pub const OUTPUT: &str = "output";
/// The node type name of the graph's optional audio output.
pub const AUDIO_OUTPUT: &str = "audio_output";
/// The node type name of the node that reads the host's video.
pub const VIDEO_INPUT: &str = "video_input";
/// The node type name of the node that reads one of the host's audio tracks.
pub const AUDIO_INPUT: &str = "audio_input";
/// The default signal name of a video input node: the project's video.
pub const DEFAULT_VIDEO: &str = "video";
/// The default signal name of an audio input node.
pub const DEFAULT_AUDIO: &str = "audio";
/// The parameter of both input nodes that names the host signal they read.
pub const SOURCE_PARAM: &str = "source";
/// The node type names of the channel splitter and combiner, whose port counts follow the signal.
pub const SPLIT: &str = "split";
pub const COMBINE: &str = "combine";
/// Most channels Split and Combine handle.
pub const MAX_CHANNELS: usize = 8;
/// The port names of Split's outputs and Combine's inputs, one per channel.
pub const CHANNEL_PORTS: [&str; MAX_CHANNELS] = ["c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8"];

/// Declares every built-in node: `category { file: [Types], … }`, one line per node file under
/// `nodes/<category>/`. Generates the modules, the re-exports and [`Registry::default`].
macro_rules! nodes {
    ($($category:ident { $($file:ident : [$($ty:ident),+ $(,)?]),* $(,)? }),* $(,)?) => {
        $(
            mod $category {
                $(
                    mod $file;
                    pub use self::$file::{$($ty),+};
                )*
            }
            pub use self::$category::*;
        )*

        impl Default for Registry {
            /// All built-in nodes.
            fn default() -> Self {
                let mut registry = Self::empty();
                $($($(registry.register::<$ty>();)+)*)*
                registry
            }
        }
    };
}

nodes! {
    input {
        source: [VideoInput, AudioInput],
    },
    generator {
        beat: [Beat],
        constant: [Constant],
        oscillator: [Oscillator],
        noise: [Noise],
    },
    output {
        video: [Output],
        audio: [AudioOutput],
    },
    structure {
        split: [Split],
        combine: [Combine],
        interleave: [Interleave],
        pack: [Pack],
        flip: [Flip],
        transpose: [Transpose],
        resample: [Resample],
        stretch: [Stretch],
    },
    convert {
        to_audio: [ToAudio],
        to_video: [ToVideo],
        relabel: [Relabel],
    },
    effect {
        three_band: [ThreeBand],
        am: [Am],
        delay: [Delay],
        bitcrush: [Bitcrush],
        compressor: [Compressor],
        gate: [Gate],
        distortion: [Distortion],
        blend: [Blend],
        envelope: [Envelope],
        filter: [Filter],
        equalizer: [Equalizer],
        fm: [Fm],
        reverb: [Reverb],
        gain: [Gain],
        offset: [Offset],
        invert: [Invert],
        clamp: [Clamp],
        remap: [Remap],
        quantize: [Quantize],
        rectify: [Rectify],
        crossfade: [Crossfade],
        sample_hold: [SampleHold],
        slew: [Slew],
        limiter: [Limiter],
        ring_mod: [RingMod],
        chorus: [Chorus],
        flanger: [Flanger],
        phaser: [Phaser],
        frequency_shifter: [FrequencyShifter],
    },
}

/// Groups node types in menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Input,
    Generator,
    Structure,
    Convert,
    Effect,
    Output,
}

impl Category {
    /// Every category, in menu order. `ALL[c.index()] == c`.
    pub const ALL: [Self; 6] = [
        Self::Input,
        Self::Generator,
        Self::Structure,
        Self::Convert,
        Self::Effect,
        Self::Output,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Input => tr("category.input"),
            Self::Generator => tr("category.generator"),
            Self::Structure => tr("category.structure"),
            Self::Convert => tr("category.convert"),
            Self::Effect => tr("category.effect"),
            Self::Output => tr("category.output"),
        }
    }

    /// Whether users add nodes of this category themselves. Inputs and the output come from the
    /// project (one per video, one per audio track, one output).
    pub const fn user_addable(self) -> bool {
        !matches!(self, Self::Input | Self::Output)
    }
}

/// The main input of a node that doesn't say otherwise.
const MAIN_INPUT: &[InputSpec] = &[InputSpec::required("in")];
const MAIN_OUTPUT: &[OutputSpec] = &[OutputSpec::new("out")];

/// How a node type presents itself and what it connects to.
#[derive(Debug, Clone, Copy)]
pub struct NodeSpec {
    /// The label, description and longer documentation are in the node's text (`node.<kind>.label`,
    /// `.description`, `.doc`), not here.
    pub category: Category,
    pub params: &'static [ParamSpec],
    /// Input ports. The first is the main input. Source nodes have none.
    pub inputs: &'static [InputSpec],
    /// Output ports, in order. Each output's tag is set by its [`TagRule`].
    pub outputs: &'static [OutputSpec],
    /// Whether the node can process each channel of an interleaved signal separately
    /// ([`crate::Channels::Separate`]). True for effects, whose output has the same layout as
    /// their main input.
    pub per_channel: bool,
    /// Whether the node is a generator that takes its shape from the video or the audio, which
    /// the node's `layout` setting picks.
    pub takes_layout: bool,
    /// The range the node is designed for on its main input (level thresholds in dB assume
    /// audio's `-1..1`). Another known range only produces a compile warning.
    pub expects: Range,
    /// Whether users add nodes of this type themselves. By default, every category but the
    /// inputs and the output, which come from the project.
    pub addable: bool,
    /// Values the node publishes each frame for the inspector to draw as meters, in the order its
    /// [`Node::meters`] writes them. At most [`MAX_METERS`].
    pub meters: &'static [Meter],
}

/// The most meter values one node publishes.
pub const MAX_METERS: usize = 4;

/// How a meter value is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterKind {
    /// A level, linear (`0` silent, `1` full scale), drawn in decibels.
    Level,
    /// Gain being taken away, in decibels (`0` or more).
    GainReduction,
}

/// One value a node publishes. Its label is `node.<kind>.meter.<id>` in the text.
#[derive(Debug, Clone, Copy)]
pub struct Meter {
    pub id: &'static str,
    pub kind: MeterKind,
}

impl NodeSpec {
    /// A node with one input, `in`, and one output, `out`.
    pub const fn new(category: Category) -> Self {
        Self {
            category,
            params: &[],
            inputs: MAIN_INPUT,
            outputs: MAIN_OUTPUT,
            per_channel: false,
            takes_layout: false,
            expects: Range::Unknown,
            addable: category.user_addable(),
            meters: &[],
        }
    }

    /// Lets users add the node themselves, whatever its category.
    pub const fn addable(mut self) -> Self {
        self.addable = true;
        self
    }

    /// The node is a generator shaped like the video or the audio (the node's `layout`).
    pub const fn takes_layout(mut self) -> Self {
        self.takes_layout = true;
        self
    }

    pub const fn per_channel(mut self) -> Self {
        self.per_channel = true;
        self
    }

    /// The meters the node publishes.
    pub const fn meters(mut self, meters: &'static [Meter]) -> Self {
        self.meters = meters;
        self
    }

    pub const fn expects(mut self, range: Range) -> Self {
        self.expects = range;
        self
    }

    pub const fn params(mut self, params: &'static [ParamSpec]) -> Self {
        self.params = params;
        self
    }

    pub const fn inputs(mut self, inputs: &'static [InputSpec]) -> Self {
        self.inputs = inputs;
        self
    }

    pub const fn outputs(mut self, outputs: &'static [OutputSpec]) -> Self {
        self.outputs = outputs;
        self
    }
}

/// A node type: everything about it lives in its file, and the registry reads it from here.
pub trait NodeKind: Node + Sized + 'static {
    /// The node's type name in graph files.
    const KIND: &'static str;

    /// How the node presents itself, its ports and its parameters.
    const SPEC: NodeSpec;

    /// Parameter sets (JSON objects) the property tests run the node with, on top of the
    /// defaults. Include settings that reach different code paths: each choice, feedback on and
    /// off, and so on.
    const TEST_CONFIGS: &'static [&'static str] = &[];

    /// The parameter set (a JSON object) the benchmarks run the node with, or `None` to skip it.
    const BENCH: Option<&'static str> = None;

    /// Creates the node. Must succeed with every parameter at its default.
    fn new(params: &Params) -> Result<Self, String>;
}

/// An enum for a choice parameter, made with `choice!`.
pub trait Choice: Sized {
    /// The variant for an option name, or `None` if there is no such option.
    fn from_option(option: &str) -> Option<Self>;
}

/// Whether the constant `NAME` is the parameter `"name"` (see `params!`).
pub const fn name_matches(param: &str, constant: &str) -> bool {
    let (a, b) = (param.as_bytes(), constant.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i].to_ascii_uppercase() != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Everything a UI, the tests and the docs need to know about a node type.
#[derive(Debug, Clone)]
pub struct NodeType {
    pub kind: String,
    pub spec: NodeSpec,
    /// Parameter sets for the property tests, from [`NodeKind::TEST_CONFIGS`].
    pub test_configs: &'static [&'static str],
    /// The parameter set for benchmarks, from [`NodeKind::BENCH`].
    pub bench: Option<&'static str>,
}

impl NodeType {
    /// The node's own text key, e.g. `node.delay.label`.
    fn key(&self, rest: &str) -> String {
        format!("node.{}.{rest}", self.kind)
    }

    /// The name shown to the user.
    pub fn label(&self) -> &'static str {
        rastersong_lang::tr(&self.key("label"))
    }

    /// One sentence for menus and tooltips.
    pub fn description(&self) -> &'static str {
        rastersong_lang::tr(&self.key("description"))
    }

    /// Longer explanation for the generated reference; empty when there isn't one.
    pub fn doc(&self) -> &'static str {
        rastersong_lang::try_tr(&self.key("doc")).unwrap_or("")
    }

    /// What input port `name` takes.
    pub fn input_help(&self, name: &str) -> &'static str {
        rastersong_lang::tr(&self.key(&format!("input.{name}")))
    }

    /// What output port `name` carries.
    pub fn output_help(&self, name: &str) -> &'static str {
        rastersong_lang::tr(&self.key(&format!("output.{name}")))
    }

    /// A parameter's text for `field` (`label`, `help` or `locked`): the node's own entry, else
    /// the shared one for that parameter name (like `mix`).
    fn param_text(&self, name: &str, field: &str) -> &'static str {
        let own = self.key(&format!("param.{name}.{field}"));
        rastersong_lang::try_tr(&own)
            .or_else(|| rastersong_lang::try_tr(&format!("param.{name}.{field}")))
            .unwrap_or_else(|| rastersong_lang::tr(&own))
    }

    /// The name shown for parameter `name`.
    pub fn param_label(&self, name: &str) -> &'static str {
        self.param_text(name, "label")
    }

    /// One sentence for parameter `name`'s tooltip.
    pub fn param_help(&self, name: &str) -> &'static str {
        self.param_text(name, "help")
    }

    /// Why parameter `name` can't be modulated; empty when it can.
    pub fn param_locked(&self, spec: &ParamSpec) -> &'static str {
        if spec.locked {
            self.param_text(spec.name, "locked")
        } else {
            ""
        }
    }

    /// Every text key this node needs an entry for (the optional `doc` aside), for the test that
    /// checks the lang files cover the registry.
    pub fn text_keys(&self) -> Vec<String> {
        let mut keys = vec![self.key("label"), self.key("description")];
        keys.extend(
            self.spec
                .inputs
                .iter()
                .map(|i| self.key(&format!("input.{}", i.name))),
        );
        keys.extend(
            self.spec
                .outputs
                .iter()
                .map(|o| self.key(&format!("output.{}", o.name))),
        );
        for p in self.spec.params {
            for field in ["label", "help"] {
                keys.push(self.key(&format!("param.{}.{field}", p.name)));
            }
            if p.locked {
                keys.push(self.key(&format!("param.{}.locked", p.name)));
            }
        }
        keys
    }

    /// How output `index`'s tag is set.
    pub fn output_tag(&self, index: usize) -> TagRule {
        self.spec
            .outputs
            .get(index)
            .map_or(TagRule::INHERIT, |o| o.tag)
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

    /// The registry of all built-in nodes, built once and shared.
    pub fn shared() -> &'static Self {
        static SHARED: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
        SHARED.get_or_init(Self::default)
    }

    /// Adds a node type.
    ///
    /// # Panics
    /// If the node is malformed: a duplicate kind, too many inputs or parameters, a default
    /// outside its range, or a constructor that fails with the defaults. These are programming
    /// errors, so the registry tests catch them.
    pub fn register<N: NodeKind>(&mut self) -> &mut Self {
        self.register_with(N::KIND, N::SPEC, N::TEST_CONFIGS, N::BENCH, N::new)
    }

    /// Adds a node type that isn't a [`NodeKind`], such as a fake node in a test.
    pub fn register_custom<N: Node + 'static>(
        &mut self,
        kind: &str,
        spec: NodeSpec,
        constructor: impl Fn(&Params) -> Result<N, String> + Send + Sync + 'static,
    ) -> &mut Self {
        self.register_with(kind, spec, &[], None, constructor)
    }

    fn register_with<N: Node + 'static>(
        &mut self,
        kind: &str,
        spec: NodeSpec,
        test_configs: &'static [&'static str],
        bench: Option<&'static str>,
        constructor: impl Fn(&Params) -> Result<N, String> + Send + Sync + 'static,
    ) -> &mut Self {
        assert!(
            !self.entries.contains_key(kind),
            "node type `{kind}` is registered twice"
        );
        assert!(
            spec.inputs.len() <= MAX_INPUTS,
            "`{kind}` has more than {MAX_INPUTS} inputs"
        );
        assert!(
            spec.params.len() <= MAX_PARAMS,
            "`{kind}` has more than {MAX_PARAMS} parameters"
        );
        assert!(!spec.outputs.is_empty(), "`{kind}` has no outputs");
        let defaults = Params::new(spec.params, &EMPTY).expect("no values");
        constructor(&defaults)
            .unwrap_or_else(|e| panic!("`{kind}` fails with default parameters: {e}"));
        self.entries.insert(
            kind.to_owned(),
            Entry {
                info: NodeType {
                    kind: kind.to_owned(),
                    spec,
                    test_configs,
                    bench,
                },
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
        types.sort_by_key(|t| (t.spec.category, t.label()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParamKind;

    #[test]
    fn every_default_is_valid_and_every_type_is_described() {
        let registry = Registry::default();
        for t in registry.types() {
            assert!(
                !t.description().is_empty(),
                "`{}` has no description",
                t.kind
            );
            for port in t.spec.inputs {
                assert!(
                    !t.input_help(port.name).is_empty(),
                    "`{}` input `{}` has no help text",
                    t.kind,
                    port.name
                );
            }
            for port in t.spec.outputs {
                assert!(
                    !t.output_help(port.name).is_empty(),
                    "`{}` output `{}` has no help text",
                    t.kind,
                    port.name
                );
            }
            for p in t.spec.params {
                assert!(
                    !t.param_help(p.name).is_empty(),
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
    fn every_node_has_valid_unique_ports_and_parameters() {
        for t in Registry::default().types() {
            let names = |ports: Vec<&str>| {
                let mut unique = ports.clone();
                unique.sort_unstable();
                unique.dedup();
                assert_eq!(
                    unique.len(),
                    ports.len(),
                    "`{}` repeats a port name",
                    t.kind
                );
            };
            names(t.spec.inputs.iter().map(|i| i.name).collect());
            names(t.spec.outputs.iter().map(|o| o.name).collect());
            names(t.spec.params.iter().map(|p| p.name).collect());
            assert!(
                t.spec.inputs.iter().all(|i| !i.name.starts_with('@')),
                "`{}` has an input that looks like a parameter pin",
                t.kind
            );
        }
    }

    #[test]
    fn every_test_config_is_valid_json_the_node_accepts() {
        let registry = Registry::default();
        for t in registry.types() {
            for config in t.test_configs.iter().chain(t.bench.iter()) {
                let values: BTreeMap<String, ParamValue> = serde_json::from_str(config)
                    .unwrap_or_else(|e| panic!("`{}` config {config}: {e}", t.kind));
                registry
                    .create(&t.kind, &values)
                    .unwrap()
                    .unwrap_or_else(|e| panic!("`{}` config {config}: {e}", t.kind));
            }
        }
    }

    #[test]
    fn every_effect_is_property_tested_and_benchmarked() {
        for t in Registry::default().types() {
            if matches!(t.spec.category, Category::Effect | Category::Generator) {
                assert!(
                    !t.test_configs.is_empty(),
                    "effect `{}` has no TEST_CONFIGS, so the property tests skip its settings",
                    t.kind
                );
                assert!(t.bench.is_some(), "effect `{}` has no BENCH", t.kind);
            }
        }
    }

    #[test]
    fn categories_are_listed_in_order() {
        for (i, c) in Category::ALL.into_iter().enumerate() {
            assert_eq!(c.index(), i);
        }
        assert!(Category::Effect.user_addable());
        assert!(!Category::Input.user_addable());
    }

    #[test]
    fn describes_ports() {
        let registry = Registry::default();
        let delay = registry.get("delay").unwrap();
        assert_eq!(registry.get("am").unwrap().spec.inputs.len(), 2);
        assert_eq!(delay.spec.outputs[0].name, "out");
        let split = registry.get("split").unwrap();
        let names: Vec<_> = split.spec.outputs.iter().map(|o| o.name).collect();
        assert_eq!(names, CHANNEL_PORTS);
        assert_eq!(delay.output_tag(0), TagRule::INHERIT);
        assert_eq!(
            registry.get(AUDIO_INPUT).unwrap().output_tag(0),
            TagRule::AUDIO
        );
        assert!(registry.get("nope").is_none());
    }

    #[test]
    #[should_panic(expected = "registered twice")]
    fn duplicate_kinds_are_rejected() {
        let mut registry = Registry::default();
        registry.register::<Split>();
    }
}
