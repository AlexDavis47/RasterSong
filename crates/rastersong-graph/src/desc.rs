//! The serialized graph: what the user edits and what is saved in project files.

use std::collections::{BTreeMap, BTreeSet};

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

/// Reads a node's parameters, applying defaults and rejecting unknown or mistyped values.
#[derive(Debug)]
pub struct Params<'a> {
    values: &'a BTreeMap<String, ParamValue>,
    used: BTreeSet<&'a str>,
}

impl<'a> Params<'a> {
    pub fn new(values: &'a BTreeMap<String, ParamValue>) -> Self {
        Self {
            values,
            used: BTreeSet::new(),
        }
    }

    fn take(&mut self, name: &str) -> Option<&'a ParamValue> {
        let (key, value) = self.values.get_key_value(name)?;
        self.used.insert(key);
        Some(value)
    }

    pub fn number(&mut self, name: &str, default: f64) -> Result<f64, String> {
        match self.take(name) {
            None => Ok(default),
            Some(ParamValue::Number(n)) if n.is_finite() => Ok(*n),
            Some(other) => Err(format!("`{name}` must be a number, got {other:?}")),
        }
    }

    /// A number constrained to `min..=max`.
    pub fn number_in(
        &mut self,
        name: &str,
        default: f64,
        min: f64,
        max: f64,
    ) -> Result<f64, String> {
        let n = self.number(name, default)?;
        if (min..=max).contains(&n) {
            Ok(n)
        } else {
            Err(format!("`{name}` must be between {min} and {max}, got {n}"))
        }
    }

    pub fn text(&mut self, name: &str, default: &str) -> Result<String, String> {
        match self.take(name) {
            None => Ok(default.to_owned()),
            Some(ParamValue::Text(s)) => Ok(s.clone()),
            Some(other) => Err(format!("`{name}` must be text, got {other:?}")),
        }
    }

    /// One of a fixed set of words.
    pub fn choice(
        &mut self,
        name: &str,
        options: &[&'static str],
        default: &'static str,
    ) -> Result<&'static str, String> {
        let value = self.text(name, default)?;
        options
            .iter()
            .find(|&&o| o == value)
            .copied()
            .ok_or_else(|| format!("`{name}` must be one of {options:?}, got {value:?}"))
    }

    /// Fails if any parameter was never read, which catches typos in parameter names.
    pub fn finish(self) -> Result<(), String> {
        match self.values.keys().find(|k| !self.used.contains(k.as_str())) {
            Some(unknown) => Err(format!("unknown parameter `{unknown}`")),
            None => Ok(()),
        }
    }
}
