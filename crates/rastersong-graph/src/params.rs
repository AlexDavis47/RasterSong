//! Node parameters: declared once per node type, used both to read graph files and to build UIs.

use std::collections::{BTreeMap, BTreeSet};

use crate::ParamValue;

/// One parameter of a node type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    /// Name in graph files.
    pub name: &'static str,
    /// Name shown to the user.
    pub label: &'static str,
    /// One sentence for tooltips.
    pub help: &'static str,
    /// Unit shown after the value, e.g. "Hz". Empty for none.
    pub unit: &'static str,
    pub kind: ParamKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamKind {
    /// `min..=max` is the usual range, which sliders show. Values outside it are allowed up to
    /// `limit_min..=limit_max`, the range the node can actually work with; typing one in widens
    /// the slider. Unless a spec says otherwise, the limits are the usual range.
    Number {
        default: f64,
        min: f64,
        max: f64,
        limit_min: f64,
        limit_max: f64,
    },
    Choice {
        options: &'static [&'static str],
        default: &'static str,
    },
    Text {
        default: &'static str,
    },
}

impl ParamSpec {
    pub const fn number(
        name: &'static str,
        label: &'static str,
        default: f64,
        min: f64,
        max: f64,
        help: &'static str,
    ) -> Self {
        Self {
            name,
            label,
            help,
            unit: "",
            kind: ParamKind::Number {
                default,
                min,
                max,
                limit_min: min,
                limit_max: max,
            },
        }
    }

    pub const fn choice(
        name: &'static str,
        label: &'static str,
        options: &'static [&'static str],
        default: &'static str,
        help: &'static str,
    ) -> Self {
        Self {
            name,
            label,
            help,
            unit: "",
            kind: ParamKind::Choice { options, default },
        }
    }

    pub const fn text(
        name: &'static str,
        label: &'static str,
        default: &'static str,
        help: &'static str,
    ) -> Self {
        Self {
            name,
            label,
            help,
            unit: "",
            kind: ParamKind::Text { default },
        }
    }

    /// Allows values from `min` to `max` beyond the slider's range (numbers only).
    pub const fn limits(mut self, min: f64, max: f64) -> Self {
        if let ParamKind::Number {
            ref mut limit_min,
            ref mut limit_max,
            ..
        } = self.kind
        {
            *limit_min = min;
            *limit_max = max;
        }
        self
    }

    /// Allows any finite value beyond the slider's range (numbers only).
    pub const fn unbounded(self) -> Self {
        self.limits(f64::NEG_INFINITY, f64::INFINITY)
    }

    pub const fn unit(mut self, unit: &'static str) -> Self {
        self.unit = unit;
        self
    }

    /// The default as a value that can be stored in a graph file.
    pub fn default_value(&self) -> ParamValue {
        match self.kind {
            ParamKind::Number { default, .. } => ParamValue::Number(default),
            ParamKind::Choice { default, .. } | ParamKind::Text { default } => {
                ParamValue::Text(default.to_owned())
            }
        }
    }
}

/// Reads a node's parameter values against its specs: applies defaults, checks types and ranges,
/// and rejects names the node doesn't have (which catches typos in graph files).
#[derive(Debug)]
pub struct Params<'a> {
    specs: &'static [ParamSpec],
    values: &'a BTreeMap<String, ParamValue>,
}

impl<'a> Params<'a> {
    /// Fails if `values` contains a parameter that isn't in `specs`.
    pub fn new(
        specs: &'static [ParamSpec],
        values: &'a BTreeMap<String, ParamValue>,
    ) -> Result<Self, String> {
        let known: BTreeSet<&str> = specs.iter().map(|s| s.name).collect();
        if let Some(unknown) = values.keys().find(|k| !known.contains(k.as_str())) {
            return Err(format!("unknown parameter `{unknown}`"));
        }
        Ok(Self { specs, values })
    }

    fn spec(&self, name: &str) -> &'static ParamSpec {
        self.specs
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| {
                panic!("node reads parameter `{name}`, which its specs don't declare")
            })
    }

    pub fn number(&self, name: &str) -> Result<f64, String> {
        let ParamKind::Number {
            default,
            limit_min,
            limit_max,
            ..
        } = self.spec(name).kind
        else {
            panic!("parameter `{name}` is not declared as a number");
        };
        match self.values.get(name) {
            None => Ok(default),
            Some(ParamValue::Number(n)) if n.is_finite() && (limit_min..=limit_max).contains(n) => {
                Ok(*n)
            }
            Some(ParamValue::Number(n)) if limit_min.is_finite() || limit_max.is_finite() => Err(
                format!("`{name}` must be between {limit_min} and {limit_max}, got {n}"),
            ),
            Some(ParamValue::Number(n)) => {
                Err(format!("`{name}` must be a finite number, got {n}"))
            }
            Some(other) => Err(format!("`{name}` must be a number, got {other:?}")),
        }
    }

    pub fn choice(&self, name: &str) -> Result<&'static str, String> {
        let ParamKind::Choice { options, default } = self.spec(name).kind else {
            panic!("parameter `{name}` is not declared as a choice");
        };
        match self.values.get(name) {
            None => Ok(default),
            Some(ParamValue::Text(s)) => options
                .iter()
                .find(|&&o| o == s)
                .copied()
                .ok_or_else(|| format!("`{name}` must be one of {options:?}, got {s:?}")),
            Some(other) => Err(format!(
                "`{name}` must be one of {options:?}, got {other:?}"
            )),
        }
    }

    pub fn text(&self, name: &str) -> Result<String, String> {
        let ParamKind::Text { default } = self.spec(name).kind else {
            panic!("parameter `{name}` is not declared as text");
        };
        match self.values.get(name) {
            None => Ok(default.to_owned()),
            Some(ParamValue::Text(s)) => Ok(s.clone()),
            Some(other) => Err(format!("`{name}` must be text, got {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPECS: &[ParamSpec] = &[
        ParamSpec::number("time", "Time", 1.0, 0.0, 10.0, ""),
        ParamSpec::number("gain", "Gain", 0.0, -1.0, 1.0, "").limits(-10.0, 10.0),
        ParamSpec::number("depth", "Depth", 0.0, -1.0, 1.0, "").unbounded(),
        ParamSpec::choice("unit", "Unit", &["rows", "frames"], "rows", ""),
        ParamSpec::text("source", "Source", "video", ""),
    ];

    fn values(json: &str) -> BTreeMap<String, ParamValue> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn defaults_apply() {
        let v = values("{}");
        let p = Params::new(SPECS, &v).unwrap();
        assert_eq!(p.number("time"), Ok(1.0));
        assert_eq!(p.choice("unit"), Ok("rows"));
        assert_eq!(p.text("source"), Ok("video".to_owned()));
    }

    #[test]
    fn values_are_checked() {
        let v = values(r#"{ "time": 11, "unit": "pixels", "source": 3 }"#);
        let p = Params::new(SPECS, &v).unwrap();
        assert!(p.number("time").unwrap_err().contains("between"));
        assert!(p.choice("unit").unwrap_err().contains("one of"));
        assert!(p.text("source").unwrap_err().contains("text"));
    }

    #[test]
    fn values_beyond_the_usual_range_are_allowed_up_to_the_limits() {
        let v = values(r#"{ "gain": 5, "depth": -1e9 }"#);
        let p = Params::new(SPECS, &v).unwrap();
        assert_eq!(p.number("gain"), Ok(5.0));
        assert_eq!(p.number("depth"), Ok(-1e9));
        let v = values(r#"{ "gain": 11 }"#);
        let p = Params::new(SPECS, &v).unwrap();
        assert!(p.number("gain").unwrap_err().contains("between -10 and 10"));
    }

    #[test]
    fn unknown_names_are_rejected() {
        let v = values(r#"{ "tme": 1 }"#);
        assert!(Params::new(SPECS, &v).unwrap_err().contains("tme"));
    }
}
