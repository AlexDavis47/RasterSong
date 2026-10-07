//! Node parameters: declared once per node type, used both to read graph files and to build UIs.

use rastersong_lang::tr_args;
use std::collections::{BTreeMap, BTreeSet};

use crate::nodes::Choice;
use crate::{MODULATION_AMOUNT_LIMITS, ModMode, Modulation, ParamValue};

/// One parameter of a node type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    /// Name in graph files.
    pub name: &'static str,
    /// Unit shown after the value, e.g. "Hz". Empty for none.
    pub unit: &'static str,
    pub kind: ParamKind,
    /// Whether a signal can be connected to modulate it (numbers only).
    pub modulatable: bool,
    /// Whether the editor shows its modulation pin on the node until the user hides it.
    pub exposed: bool,
    /// Whether only whole numbers make sense (counts, divisions, steps): the slider and value box
    /// snap to them, loaded values are rounded, and a modulated value is rounded at every sample.
    pub integer: bool,
    /// Whether a signal is barred from modulating it (set by [`Self::fixed`]); the reason is in the
    /// node's text, under `locked`.
    pub locked: bool,
    /// When the parameter means something: the editor hides it (and the docs say so) while
    /// the rule fails. A hidden parameter keeps its value and still saves.
    pub when: Option<ShownWhen>,
}

/// A parameter that only matters while another, a choice, has one of some values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShownWhen {
    /// Name of the choice parameter that decides.
    pub param: &'static str,
    /// The choices of that parameter for which this one is used.
    pub values: &'static [&'static str],
}

/// The size of a range, zero for one that isn't finite.
pub fn range_span(range: (f64, f64)) -> f64 {
    let span = range.1 - range.0;
    if span.is_finite() { span.max(0.0) } else { 0.0 }
}

/// How much a newly connected signal moves a parameter, in percent of its span.
pub const DEFAULT_MODULATION_PERCENT: f64 = 25.0;

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
    pub const fn number(name: &'static str, default: f64, min: f64, max: f64) -> Self {
        Self {
            name,
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
            integer: false,
            locked: false,
            when: None,
        }
    }

    /// The dry/wet blend every effect with a `mix` shares: 0 is the node's input untouched, 1 is
    /// only what the node made of it, and in between crossfades the two ([`crate::dsp::mix`]).
    /// Always starts at 1, fully processed.
    pub const fn mix() -> Self {
        Self::number("mix", 1.0, 0.0, 1.0)
    }

    pub const fn choice(
        name: &'static str,
        options: &'static [&'static str],
        default: &'static str,
    ) -> Self {
        Self {
            name,
            unit: "",
            kind: ParamKind::Choice { options, default },
            modulatable: false,
            exposed: false,
            integer: false,
            locked: false,
            when: None,
        }
    }

    pub const fn text(name: &'static str, default: &'static str) -> Self {
        Self {
            name,
            unit: "",
            kind: ParamKind::Text { default },
            modulatable: false,
            exposed: false,
            integer: false,
            locked: false,
            when: None,
        }
    }

    /// Shows the parameter's modulation pin on new nodes.
    pub const fn exposed(mut self) -> Self {
        self.exposed = true;
        self
    }

    /// A number that can only be whole. The usual range and limits should be whole too.
    pub const fn integer(mut self) -> Self {
        self.integer = true;
        self
    }

    /// Only used while the choice parameter `param` is one of `values`. The editor hides it the
    /// rest of the time, unless a signal is connected to it, and says why it is unused.
    pub const fn shown_when(
        mut self,
        param: &'static str,
        values: &'static [&'static str],
    ) -> Self {
        self.when = Some(ShownWhen { param, values });
        self
    }

    /// Whether the parameter is used, given the node's parameters (`value_of` gives a choice
    /// parameter's current value by name). Always true for one without a rule.
    pub fn is_used(&self, value_of: impl Fn(&str) -> Option<String>) -> bool {
        self.when
            .is_none_or(|w| value_of(w.param).is_some_and(|v| w.values.contains(&v.as_str())))
    }

    /// Can't be modulated, for a reason the node's text states (`locked`), which the editor shows on
    /// the parameter. Modulation is the default and a lock needs a real reason: the value changes
    /// the shape of what the graph is compiled for, say, not "nobody wrote it".
    pub const fn fixed(mut self) -> Self {
        self.modulatable = false;
        self.exposed = false;
        self.locked = true;
        self
    }

    /// The usual range, which the slider shows unless the user has set another; zero-sized for
    /// anything that isn't a number.
    pub fn usual_range(&self) -> (f64, f64) {
        match self.kind {
            ParamKind::Number { min, max, .. } => (min, max),
            _ => (0.0, 0.0),
        }
    }

    /// The size of the usual range, in the parameter's own unit.
    pub fn modulation_span(&self) -> f64 {
        range_span(self.usual_range())
    }

    /// The modulation amount a newly connected signal starts with, in percent of the span.
    pub fn default_modulation_amount(&self) -> f64 {
        DEFAULT_MODULATION_PERCENT
    }

    /// The modulation a newly connected signal gets: the default amount, one way.
    pub fn default_modulation(&self) -> Modulation {
        Modulation {
            amount: self.default_modulation_amount(),
            mode: ModMode::Unipolar,
        }
    }

    /// How far a full-scale signal moves the value, in the parameter's unit, for a slider showing
    /// `range`: the amount, a percentage of that range's size. Both ways it is the distance
    /// either side of the value.
    pub fn modulation_sweep(&self, modulation: Modulation, range: (f64, f64)) -> f64 {
        modulation.amount / 100.0 * range_span(range)
    }

    /// The amount (percent of the range) whose sweep is `sweep`, the inverse of
    /// [`Self::modulation_sweep`], held to what an amount can be: for typing a distance in the
    /// parameter's own unit.
    pub fn modulation_amount_for_sweep(&self, sweep: f64, range: (f64, f64)) -> f64 {
        let span = range_span(range);
        let amount = if span > 0.0 {
            sweep * 100.0 / span
        } else {
            0.0
        };
        amount.clamp(MODULATION_AMOUNT_LIMITS.0, MODULATION_AMOUNT_LIMITS.1)
    }

    /// Where a modulated value is kept: the slider's range, widened to include `base` (which
    /// the editor keeps inside it anyway), and never past the parameter's limits.
    pub fn modulation_bounds(&self, base: f64, range: (f64, f64)) -> (f64, f64) {
        let ParamKind::Number {
            limit_min,
            limit_max,
            ..
        } = self.kind
        else {
            return (f64::MIN, f64::MAX);
        };
        (
            range.0.min(base).max(limit_min),
            range.1.max(base).min(limit_max),
        )
    }

    /// The value for one sample `signal` of a modulating signal, before clamping to the bounds.
    pub fn modulated(
        &self,
        base: f64,
        modulation: Modulation,
        signal: f64,
        range: (f64, f64),
    ) -> f64 {
        let shaped = f64::from(modulation.mode.shape(signal as f32));
        base + self.modulation_sweep(modulation, range) * shaped
    }

    /// The range a modulated value moves over for a signal within `-1..=1`, within the bounds.
    pub fn modulated_range(
        &self,
        base: f64,
        modulation: Modulation,
        range: (f64, f64),
    ) -> (f64, f64) {
        let ends = match modulation.mode {
            ModMode::Bipolar => [-1.0, 1.0],
            ModMode::Unipolar => [0.0, 1.0],
        };
        let [a, b] = ends.map(|s| self.modulated(base, modulation, s, range));
        let (lo, hi) = self.modulation_bounds(base, range);
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
            return Err(tr_args("error.param.unknown", &[("name", unknown)]));
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
            Some(ParamValue::Number(n)) if limit_min.is_finite() || limit_max.is_finite() => {
                Err(tr_args(
                    "error.param.between",
                    &[
                        ("name", name),
                        ("min", &limit_min.to_string()),
                        ("max", &limit_max.to_string()),
                        ("got", &n.to_string()),
                    ],
                ))
            }
            Some(ParamValue::Number(n)) => Err(tr_args(
                "error.param.finite",
                &[("name", name), ("got", &n.to_string())],
            )),
            Some(other) => Err(tr_args(
                "error.param.number",
                &[("name", name), ("got", &format!("{other:?}"))],
            )),
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
            Some(ParamValue::Text(s)) => {
                options.iter().find(|&&o| o == s).copied().ok_or_else(|| {
                    tr_args(
                        "error.param.choice",
                        &[
                            ("name", name),
                            ("options", &format!("{options:?}")),
                            ("got", &format!("{s:?}")),
                        ],
                    )
                })
            }
            Some(other) => Err(tr_args(
                "error.param.choice",
                &[
                    ("name", name),
                    ("options", &format!("{options:?}")),
                    ("got", &format!("{other:?}")),
                ],
            )),
        }
    }

    /// The choice at `index` as the enum made with `choice!`. There is no fallback: an option the
    /// enum doesn't know is an error, which means the spec and the enum disagree.
    pub fn choice_as<E: Choice>(&self, index: usize) -> Result<E, String> {
        let option = self.choice_at(index)?;
        E::from_option(option).ok_or_else(|| {
            tr_args(
                "error.param.no_variant",
                &[("name", self.specs[index].name), ("option", option)],
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
            Some(other) => Err(tr_args(
                "error.param.text",
                &[("name", name), ("got", &format!("{other:?}"))],
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPECS: &[ParamSpec] = &[
        ParamSpec::number("time", 1.0, 0.0, 10.0),
        ParamSpec::number("gain", 0.0, -1.0, 1.0).limits(-10.0, 10.0),
        ParamSpec::number("depth", 0.0, -1.0, 1.0).unbounded(),
        ParamSpec::choice("unit", &["rows", "frames"], "rows"),
        ParamSpec::text("source", "video"),
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
    fn modulation_is_a_percentage_of_the_slider_range() {
        let spec = ParamSpec::number("time", 1.0, 0.0, 10.0);
        let range = spec.usual_range();
        let both = |amount| Modulation {
            amount,
            mode: ModMode::Bipolar,
        };
        let one_way = |amount| Modulation {
            amount,
            mode: ModMode::Unipolar,
        };
        // One rule in both modes: a full signal moves the value amount% of the range from where it is.
        assert_eq!(spec.modulation_sweep(both(40.0), range), 4.0);
        assert_eq!(spec.modulation_sweep(one_way(40.0), range), 4.0);
        assert_eq!(spec.modulated(5.0, both(40.0), 1.0, range), 9.0);
        assert_eq!(spec.modulated(5.0, both(40.0), -1.0, range), 1.0);
        assert_eq!(
            spec.modulated(5.0, one_way(40.0), -0.5, range),
            7.0,
            "one way uses |signal|"
        );
        assert_eq!(
            spec.modulated(5.0, one_way(-20.0), 1.0, range),
            3.0,
            "negative turns it down"
        );
        // The percentage is of the slider's range, so a narrower slider makes the same amount smaller.
        assert_eq!(spec.modulation_sweep(both(50.0), (0.0, 4.0)), 2.0);
        // The value stays inside the range, widened to include the base.
        assert_eq!(spec.modulated_range(5.0, both(40.0), range), (1.0, 9.0));
        assert_eq!(spec.modulated_range(9.0, both(40.0), range), (5.0, 10.0));
        assert_eq!(spec.modulated_range(5.0, one_way(-20.0), range), (3.0, 5.0));
        assert_eq!(spec.modulated_range(5.0, both(100.0), range), (0.0, 10.0));
        assert_eq!(
            spec.modulated_range(5.0, both(100.0), (2.0, 6.0)),
            (2.0, 6.0)
        );
        let wide = ParamSpec::number("t", 1.0, 0.0, 10.0).limits(-100.0, 100.0);
        assert_eq!(
            wide.modulated_range(50.0, both(40.0), range),
            (46.0, 50.0),
            "a base past the range keeps it"
        );
        assert_eq!(
            wide.modulation_bounds(5.0, (-500.0, 500.0)),
            (-100.0, 100.0),
            "never past the limits"
        );
        // Typing a distance gives the percentage that makes it, held to what an amount can be.
        let amount = spec.modulation_amount_for_sweep(2.0, range);
        assert_eq!(spec.modulation_sweep(both(amount), range), 2.0);
        assert_eq!(spec.modulation_amount_for_sweep(-20.0, range), -100.0);
        assert_eq!(spec.modulation_amount_for_sweep(50.0, range), 100.0);
        assert_eq!(spec.modulation_amount_for_sweep(5.0, (3.0, 3.0)), 0.0);
        assert_eq!(spec.default_modulation_amount(), 25.0);
        assert_eq!(spec.default_modulation().mode, ModMode::Unipolar);
    }

    #[test]
    fn unknown_names_are_rejected() {
        let v = values(r#"{ "tme": 1 }"#);
        assert!(Params::new(SPECS, &v).unwrap_err().contains("tme"));
    }
}
