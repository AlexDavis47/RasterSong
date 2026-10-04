//! Node parameters: declared once per node type, used both to read graph files and to build UIs.

use std::collections::{BTreeMap, BTreeSet};

use crate::nodes::Choice;
use crate::{ModMode, Modulation, ParamValue};

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
    /// Whether a signal can be connected to modulate it (numbers only).
    pub modulatable: bool,
    /// Whether the editor shows its modulation pin on the node until the user hides it.
    pub exposed: bool,
    /// How a modulation amount applies to the value.
    pub scale: ModScale,
}

/// How a modulation amount applies to a parameter's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModScale {
    /// The amount is in the parameter's own unit: `base + amount × signal`.
    #[default]
    Linear,
    /// The amount is in octaves: `base × 2^(amount × signal)`. For frequencies and other
    /// parameters heard or seen on a logarithmic scale.
    Octaves,
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
            modulatable: true,
            exposed: false,
            scale: ModScale::Linear,
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
            modulatable: false,
            exposed: false,
            scale: ModScale::Linear,
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
            modulatable: false,
            exposed: false,
            scale: ModScale::Linear,
        }
    }

    /// Shows the parameter's modulation pin on new nodes.
    pub const fn exposed(mut self) -> Self {
        self.exposed = true;
        self
    }

    /// Can't be modulated: for parameters that are too costly or meaningless to change while
    /// rendering, such as filter crossovers.
    pub const fn fixed(mut self) -> Self {
        self.modulatable = false;
        self.exposed = false;
        self
    }

    /// Modulation amounts are in octaves.
    pub const fn octaves(mut self) -> Self {
        self.scale = ModScale::Octaves;
        self
    }

    /// The modulation amount a newly connected signal starts with, around a base value of
    /// `base`: one octave, or half the base, or a tenth of the usual range when the base is zero.
    pub fn default_modulation_amount(&self, base: f64) -> f64 {
        match self.kind {
            ParamKind::Number { .. } if self.scale == ModScale::Octaves => 1.0,
            ParamKind::Number { min, max, .. } if base == 0.0 => (max - min) / 10.0,
            ParamKind::Number { .. } => base.abs() / 2.0,
            _ => 0.0,
        }
    }

    /// The modulation a newly connected signal gets: the default amount, both ways.
    pub fn default_modulation(&self, base: f64) -> Modulation {
        Modulation {
            amount: self.default_modulation_amount(base),
            mode: ModMode::Bipolar,
        }
    }

    /// The value for one sample `signal` of a modulating signal, before clamping to the limits.
    pub fn modulated(&self, base: f64, modulation: Modulation, signal: f64) -> f64 {
        let offset = modulation.amount * f64::from(modulation.mode.shape(signal as f32));
        match self.scale {
            ModScale::Linear => base + offset,
            ModScale::Octaves => base * offset.exp2(),
        }
    }

    /// The range a modulated value moves over for a signal within `-1..=1`, within the limits.
    pub fn modulated_range(&self, base: f64, modulation: Modulation) -> (f64, f64) {
        let ends = match modulation.mode {
            ModMode::Bipolar => [-1.0, 1.0],
            ModMode::Unipolar => [0.0, 1.0],
        };
        let [a, b] = ends.map(|s| self.modulated(base, modulation, s));
        let (lo, hi) = self.number_limits().unwrap_or((f64::MIN, f64::MAX));
        (a.min(b).clamp(lo, hi), a.max(b).clamp(lo, hi))
    }

    /// The number's base value, from the node's values or the default.
    pub fn number_value(&self, values: &BTreeMap<String, ParamValue>) -> Option<f64> {
        match (self.kind, values.get(self.name)) {
            (_, Some(ParamValue::Number(n))) => Some(*n),
            (ParamKind::Number { default, .. }, None) => Some(default),
            _ => None,
        }
    }

    /// The hard limits of a number, or `None` for other kinds.
    pub fn number_limits(&self) -> Option<(f64, f64)> {
        match self.kind {
            ParamKind::Number {
                limit_min,
                limit_max,
                ..
            } => Some((limit_min, limit_max)),
            _ => None,
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

    fn index_of(&self, name: &str) -> usize {
        self.specs
            .iter()
            .position(|s| s.name == name)
            .unwrap_or_else(|| {
                panic!("node reads parameter `{name}`, which its specs don't declare")
            })
    }

    /// A number, by name. Nodes read by index (`number_at`) so a name can't drift from `PARAMS`.
    pub fn number(&self, name: &str) -> Result<f64, String> {
        self.number_at(self.index_of(name))
    }

    /// The choice named `name`, as one of its option strings.
    pub fn choice(&self, name: &str) -> Result<&'static str, String> {
        self.choice_at(self.index_of(name))
    }

    pub fn text(&self, name: &str) -> Result<String, String> {
        self.text_at(self.index_of(name))
    }

    /// The number at `index` in the node's specs (the constants `params!` declares).
    pub fn number_at(&self, index: usize) -> Result<f64, String> {
        let spec = &self.specs[index];
        let name = spec.name;
        let ParamKind::Number {
            default,
            limit_min,
            limit_max,
            ..
        } = spec.kind
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

    /// The number at `index` as `f32`, which is what nodes compute with.
    pub fn float_at(&self, index: usize) -> Result<f32, String> {
        self.number_at(index).map(|n| n as f32)
    }

    pub fn choice_at(&self, index: usize) -> Result<&'static str, String> {
        let spec = &self.specs[index];
        let name = spec.name;
        let ParamKind::Choice { options, default } = spec.kind else {
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

    /// The choice at `index` as the enum made with `choice!`. There is no fallback: an option the
    /// enum doesn't know is an error, which means the spec and the enum disagree.
    pub fn choice_as<E: Choice>(&self, index: usize) -> Result<E, String> {
        let option = self.choice_at(index)?;
        E::from_option(option).ok_or_else(|| {
            format!(
                "`{}` option `{option}` has no matching variant",
                self.specs[index].name
            )
        })
    }

    pub fn text_at(&self, index: usize) -> Result<String, String> {
        let spec = &self.specs[index];
        let name = spec.name;
        let ParamKind::Text { default } = spec.kind else {
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
    fn modulation_moves_values_linearly_or_in_octaves() {
        let linear = ParamSpec::number("time", "Time", 1.0, 0.0, 10.0, "");
        let octaves = ParamSpec::number("cutoff", "Cutoff", 40.0, 1.0, 1000.0, "").octaves();
        let both = |amount| Modulation {
            amount,
            mode: ModMode::Bipolar,
        };
        let one_way = |amount| Modulation {
            amount,
            mode: ModMode::Unipolar,
        };
        assert_eq!(linear.modulated(5.0, both(2.0), -0.5), 4.0);
        assert_eq!(
            linear.modulated(5.0, one_way(-2.0), -0.5),
            4.0,
            "one way uses |signal|"
        );
        assert_eq!(octaves.modulated(40.0, both(1.0), 1.0), 80.0);
        assert_eq!(octaves.modulated(40.0, both(1.0), -1.0), 20.0);
        // Ranges are clamped to the limits (here the usual range, 0..10).
        assert_eq!(linear.modulated_range(5.0, both(2.0)), (3.0, 7.0));
        assert_eq!(linear.modulated_range(9.0, both(2.0)), (7.0, 10.0));
        assert_eq!(linear.modulated_range(5.0, one_way(-2.0)), (3.0, 5.0));
        assert_eq!(octaves.modulated_range(40.0, both(2.0)), (10.0, 160.0));
        assert_eq!(octaves.default_modulation_amount(40.0), 1.0);
        assert_eq!(linear.default_modulation_amount(4.0), 2.0);
        assert_eq!(linear.default_modulation_amount(0.0), 1.0);
    }

    #[test]
    fn unknown_names_are_rejected() {
        let v = values(r#"{ "tme": 1 }"#);
        assert!(Params::new(SPECS, &v).unwrap_err().contains("tme"));
    }
}
