//! The serialized graph: what the user edits and what is saved in project files.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::GraphError;

/// Version 2 made a connected parameter with no `modulation` entry unipolar; version 1 meant
/// bipolar. Version 3 made modulation amounts percentages of the parameter's span instead of
/// numbers in its own unit. Version 4 keeps modulated values within the slider's range unless an
/// entry sets `overshoot`; older graphs are rewritten on load, with explicit entries that
/// overshoot as they always did. Version 5 narrowed the usual range of some frequencies. Version
/// 6 made frequency modulation linear like every other parameter (it was in octaves), so those
/// amounts are converted to the equivalent linear amount at the parameter's base value. Version 7
/// made a generator's `layout` a node setting instead of a parameter. Version 8 saves the
/// slider ranges the user sets, which limit modulation.
pub const FORMAT_VERSION: u32 = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphDesc {
    pub version: u32,
    pub nodes: Vec<NodeDesc>,
    #[serde(default)]
    pub connections: Vec<Connection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDesc {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ParamValue>,
    /// How secondary inputs are resampled to the main input's length.
    #[serde(default, skip_serializing_if = "is_default")]
    pub interpolation: Interpolation,
    /// How a mono secondary input is stretched over a multi-channel main input.
    #[serde(default, skip_serializing_if = "is_default")]
    pub grouping: Grouping,
    /// Whether an interleaved signal (RGB, stereo) is processed as one stream or as one stream
    /// per channel.
    #[serde(default, skip_serializing_if = "is_default")]
    pub channels: Channels,
    /// Which host signal a generator takes its shape (resolution or sample count) from. Only
    /// written when it isn't the video.
    #[serde(default, skip_serializing_if = "is_default")]
    pub layout: GeneratorLayout,
    /// Passes the main input straight through to the first output, skipping the node's processing.
    #[serde(default, skip_serializing_if = "is_default")]
    pub bypass: bool,
    /// Name shown in the editor instead of the node type's. Has no effect on rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Where the node sits in the editor. Has no effect on rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 2]>,
    /// How signals connected to parameters (`"node.@param"`) move them, by parameter name.
    /// A connected parameter with no entry uses its default amount, unipolar.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modulation: BTreeMap<String, Modulation>,
    /// Number parameters that are rounded to whole numbers: the value set on the node and, for a
    /// modulated parameter, the value of every sample after modulation (so a signal steps it).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub integer: Vec<String>,
    /// Slider ranges the user set, by parameter name: what the slider shows and, unless a
    /// modulation may overshoot, where a signal can take the value. Parameters without an entry
    /// use the node type's usual range.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ranges: BTreeMap<String, [f64; 2]>,
    /// Parameters whose modulation pins the editor shows, when they differ from the node type's
    /// defaults. Has no effect on rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposed: Option<Vec<String>>,
}

/// How a signal connected to a parameter moves it, clamped to the parameter's limits. The
/// `amount` is a percentage of the parameter's span (its usual range): one way, the value moves `amount` of the span at full signal; both ways,
/// `amount` is the whole swing from the lowest point to the highest, so 100% covers the span.
/// Negative one-way amounts move the value down. The value stays within the parameter's usual
/// range (widened to include its base value) unless `overshoot` allows it up to the limits.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Modulation {
    pub amount: f64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub mode: ModMode,
    /// Lets the modulated value go past the slider's range, up to the parameter's limits.
    #[serde(default, skip_serializing_if = "is_default")]
    pub overshoot: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModMode {
    /// The signal moves the value both ways around its base.
    #[default]
    Bipolar,
    /// The signal's magnitude moves the value one way: up for a positive amount, down for a
    /// negative one.
    Unipolar,
}

impl ModMode {
    /// What a signal sample contributes, before scaling by the amount.
    pub fn shape(self, s: f32) -> f32 {
        match self {
            Self::Bipolar => s,
            Self::Unipolar => s.abs(),
        }
    }
}

/// A connection from an output port to an input port, written `"node.port"`. The port can be left
/// out (`"node"`) to mean the node's first output or its main input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    /// Repeat each source sample. A mono signal stretched over RGB affects R, G and B of a pixel equally.
    #[default]
    Hold,
    /// Ramp linearly between source samples.
    Linear,
}

/// How a node processes a multi-channel (RGB, stereo) main input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channels {
    /// The interleaved stream (R, G, B, R, G, B, … or L, R, L, R, …) as one signal.
    #[default]
    Together,
    /// Each channel through its own copy of the node, with its own state. The copies are
    /// identical: the same as Split, the node once per channel, and Combine.
    Separate,
}

/// How a secondary input with one channel is stretched over a main input with several.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    /// Each source sample covers whole pixels, so a pixel's R, G and B (or L and R) move
    /// together.
    #[default]
    Pixels,
    /// Spread over every sample value, ignoring pixels: a pixel's channels can differ.
    Samples,
}

/// Which host signal a generator takes its layout (resolution or sample count) from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratorLayout {
    /// The video's layout: RGB pixels in rows.
    #[default]
    Video,
    /// The audio track's layout.
    Audio,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

impl GraphDesc {
    pub fn from_json(json: &str) -> Result<Self, GraphError> {
        let desc: Self =
            serde_json::from_str(json).map_err(|e| GraphError::Parse(e.to_string()))?;
        if !(1..=FORMAT_VERSION).contains(&desc.version) {
            return Err(GraphError::Parse(format!(
                "unsupported graph format version {} (expected {FORMAT_VERSION})",
                desc.version
            )));
        }
        let mut desc = desc;
        desc.upgrade();
        Ok(desc)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("graph descriptions always serialize")
    }
}
