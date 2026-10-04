//! The serialized graph: what the user edits and what is saved in project files.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::GraphError;

pub const FORMAT_VERSION: u32 = 1;

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
    /// Whether an RGB signal is processed as one stream or as three separate channels.
    #[serde(default, skip_serializing_if = "is_default")]
    pub channels: Channels,
    /// Name shown in the editor instead of the node type's. Has no effect on rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Where the node sits in the editor. Has no effect on rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 2]>,
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

/// How a node processes a multi-channel (RGB) main input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channels {
    /// The interleaved stream R, G, B, R, G, B, … as one signal.
    #[default]
    Together,
    /// R, G and B each through their own copy of the node, with its own state.
    Separate,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

impl GraphDesc {
    pub fn from_json(json: &str) -> Result<Self, GraphError> {
        let desc: Self =
            serde_json::from_str(json).map_err(|e| GraphError::Parse(e.to_string()))?;
        if desc.version != FORMAT_VERSION {
            return Err(GraphError::Parse(format!(
                "unsupported graph format version {} (expected {FORMAT_VERSION})",
                desc.version
            )));
        }
        Ok(desc)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("graph descriptions always serialize")
    }
}
