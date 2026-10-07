//! Bringing graphs written for older versions of the nodes up to date.

use crate::desc::{
    FORMAT_VERSION, GeneratorLayout, GraphDesc, MODULATION_AMOUNT_LIMITS, ModMode, Modulation,
};
use crate::{ParamKind, ParamSpec, ParamValue, Registry};

// Migrations are idempotent rewrites that recognise old graphs by their shape, so a graph that
// is already up to date passes through unchanged. Bump `FORMAT_VERSION` only for a change to the
// file format itself (not to nodes), and keep the old shape readable. Version 2 changed what a
// connected parameter without a `modulation` entry means (bipolar -> unipolar); `upgrade_modulation`
// writes the old meaning out for version 1 graphs.
//
// To rename or move something, add a row to one of the tables below and a test; never edit or
// remove an existing row, since files written long ago may still use it.

/// What a newly connected signal's amount used to be, in the parameter's own unit: one octave, or
/// half the base value, or a tenth of the usual range when the base is zero.
fn legacy_default_amount(spec: &ParamSpec, octaves: bool, base: f64) -> f64 {
    match spec.kind {
        ParamKind::Number { .. } if octaves => 1.0,
        ParamKind::Number { min, max, .. } if base == 0.0 => (max - min) / 10.0,
        ParamKind::Number { .. } => base.abs() / 2.0,
        _ => 0.0,
    }
}

/// An amount given in the parameter's own unit (octaves for frequencies), as the percentage of
/// its span that moves the value just as far.
fn percent_of_span(spec: &ParamSpec, modulation: Modulation) -> f64 {
    let reach = match modulation.mode {
        ModMode::Bipolar => 0.5,
        ModMode::Unipolar => 1.0,
    };
    let span = spec.modulation_span();
    if span > 0.0 {
        modulation.amount * 100.0 / (span * reach)
    } else {
        modulation.amount
    }
}

/// A node that used to have a `modulation` input and a `depth` parameter. Parameter modulation
/// does the same job now: a signal connected to `node.@param` with the old depth as its amount.
struct ModulationInput {
    kind: &'static str,
    /// The parameter the old input moved.
    param: &'static str,
}

const MODULATION_INPUTS: &[ModulationInput] = &[
    ModulationInput {
        kind: "delay",
        param: "time",
    },
    ModulationInput {
        kind: "bitcrush",
        param: "bits",
    },
    ModulationInput {
        kind: "lowpass",
        param: "cutoff",
    },
];

/// A frequency that modulated in octaves until version 6. Its amount was a percentage of its
/// usual range measured in octaves, which was `old` (the range before version 5) in graphs before
/// version 5.
struct OctaveParam {
    kind: &'static str,
    param: &'static str,
    old: (f64, f64),
}

const OCTAVE_PARAMS: &[OctaveParam] = &[
    OctaveParam {
        kind: "oscillator",
        param: "freq",
        old: (0.01, 100.0),
    },
    OctaveParam {
        kind: "filter",
        param: "cutoff",
        old: (0.01, 1000.0),
    },
    OctaveParam {
        kind: "lowpass",
        param: "cutoff",
        old: (0.01, 100_000.0),
    },
    OctaveParam {
        kind: "equalizer",
        param: "low_freq",
        old: (0.01, 1000.0),
    },
    OctaveParam {
        kind: "equalizer",
        param: "mid_freq",
        old: (0.01, 1000.0),
    },
    OctaveParam {
        kind: "equalizer",
        param: "high_freq",
        old: (0.01, 1000.0),
    },
    OctaveParam {
        kind: "phaser",
        param: "freq",
        old: (20.0, 20_000.0),
    },
];

/// The linear amount (percent of the span) that moves a value from `base` as far as an octave
/// modulation did: a sweep of `octaves` each way at full signal. Both ways the value moved up by
/// `base × 2^o` and down by `base / 2^o`, which are averaged. Exact at the base, an
/// approximation elsewhere, since an octave sweep is not even in the parameter's own unit.
fn octaves_to_percent(spec: &ParamSpec, mode: ModMode, base: f64, octaves: f64) -> f64 {
    let (up, down) = (octaves.exp2(), (-octaves).exp2());
    let (distance, reach) = match mode {
        ModMode::Bipolar => (base * (up - down) / 2.0, 0.5),
        ModMode::Unipolar => (base * (up - 1.0), 1.0),
    };
    let span = spec.modulation_span();
    if span > 0.0 {
        distance * 100.0 / (span * reach)
    } else {
        0.0
    }
}

/// A node type that changed its name.
struct RenamedKind {
    old: &'static str,
    new: &'static str,
}

const RENAMED_KINDS: &[RenamedKind] = &[];

/// A parameter that changed its name, on a node type that keeps its name.
struct RenamedParam {
    kind: &'static str,
    old: &'static str,
    new: &'static str,
}

const RENAMED_PARAMS: &[RenamedParam] = &[];

/// An input or output port that changed its name, on a node type that keeps its name.
struct RenamedPort {
    kind: &'static str,
    old: &'static str,
    new: &'static str,
}

/// Split and Combine were RGB only, with ports `r`, `g` and `b`; they now take any number of
/// channels, numbered.
const RENAMED_PORTS: &[RenamedPort] = &[
    RenamedPort {
        kind: "split",
        old: "r",
        new: "c1",
    },
    RenamedPort {
        kind: "split",
        old: "g",
        new: "c2",
    },
    RenamedPort {
        kind: "split",
        old: "b",
        new: "c3",
    },
    RenamedPort {
        kind: "combine",
        old: "r",
        new: "c1",
    },
    RenamedPort {
        kind: "combine",
        old: "g",
        new: "c2",
    },
    RenamedPort {
        kind: "combine",
        old: "b",
        new: "c3",
    },
];

/// A choice parameter whose option was renamed, on a node type that keeps its name.
struct RenamedChoice {
    kind: &'static str,
    param: &'static str,
    old: &'static str,
    new: &'static str,
}

/// Frequency units were once written out ("cycles/row"); the parameter's meaning already says
/// cycles.
const RENAMED_CHOICES: &[RenamedChoice] = &[
    RenamedChoice {
        kind: "filter",
        param: "unit",
        old: "cycles/row",
        new: "Row",
    },
    RenamedChoice {
        kind: "filter",
        param: "unit",
        old: "cycles/frame",
        new: "Frame",
    },
    RenamedChoice {
        kind: "filter",
        param: "unit",
        old: "Hz",
        new: "Hertz",
    },
    RenamedChoice {
        kind: "equalizer",
        param: "unit",
        old: "cycles/row",
        new: "Row",
    },
    RenamedChoice {
        kind: "equalizer",
        param: "unit",
        old: "cycles/frame",
        new: "Frame",
    },
    RenamedChoice {
        kind: "equalizer",
        param: "unit",
        old: "Hz",
        new: "Hertz",
    },
    RenamedChoice {
        kind: "oscillator",
        param: "unit",
        old: "cycles/row",
        new: "Row",
    },
    RenamedChoice {
        kind: "oscillator",
        param: "unit",
        old: "cycles/frame",
        new: "Frame",
    },
    RenamedChoice {
        kind: "oscillator",
        param: "unit",
        old: "Hz",
        new: "Hertz",
    },
];

/// Time units and frequency units were two lists with their own names; they are one now, singular
/// and lowercase. Every node with a `unit` parameter takes these, whichever list it used.
const RENAMED_UNITS: &[(&str, &str)] = &[
    ("rows", "row"),
    ("frames", "frame"),
    ("seconds", "second"),
    ("beats", "beat"),
    ("bars", "bar"),
    ("Row", "row"),
    ("Frame", "frame"),
    ("Hertz", "second"),
    ("Beat", "beat"),
    ("Bar", "bar"),
];

impl GraphDesc {
    /// Rewrites anything written for older node versions. Graphs already up to date are left
    /// as they are. [`GraphDesc::from_json`] calls it; call it on graphs deserialized any other
    /// way (e.g. inside a project file).
    pub fn upgrade(&mut self) {
        self.rename_kinds(RENAMED_KINDS);
        self.rename_params(RENAMED_PARAMS);
        self.rename_ports(RENAMED_PORTS);
        self.rename_choices(RENAMED_CHOICES);
        self.rename_units(Registry::shared());
        self.round_whole_parameters(Registry::shared());
        self.move_layout_to_settings(Registry::shared());
        self.convert_modulation_inputs(MODULATION_INPUTS);
        self.upgrade_modulation(Registry::shared());
        self.modulate_by_one_rule(Registry::shared());
        self.version = FORMAT_VERSION;
    }

    /// Graphs before version 9 measured a both-ways amount peak to peak and could let a value go
    /// past the slider's range. Modulation is one rule now: the amount is a percentage of the
    /// slider range, a full-scale signal moves the value that far (either way for both ways), and
    /// the value stays inside the range. An old amount becomes the percentage that moves the value
    /// as far, and an entry that overshot gets the slider range widened to where it used to reach
    /// (within the parameter's limits), so the graph does what it did.
    fn modulate_by_one_rule(&mut self, registry: &Registry) {
        if self.version >= 9 {
            return;
        }
        for node in &mut self.nodes {
            let Some(kind) = registry.get(&node.kind) else {
                continue;
            };
            for (name, modulation) in &mut node.modulation {
                let Some(spec) = kind
                    .spec
                    .params
                    .iter()
                    .find(|s| s.name == *name && s.number_limits().is_some())
                else {
                    continue;
                };
                let Some(base) = spec.number_value(&node.params) else {
                    continue;
                };
                let (limit_min, limit_max) = spec.number_limits().unwrap_or((f64::MIN, f64::MAX));
                let half = match modulation.mode {
                    ModMode::Bipolar => 0.5,
                    ModMode::Unipolar => 1.0,
                };
                // How far a full-scale signal moved the value, in the parameter's unit.
                let sweep = modulation.amount / 100.0 * spec.modulation_span() * half;
                let slider = node.ranges.get(name.as_str()).map(|r| (r[0], r[1]));
                let mut range = slider.unwrap_or_else(|| spec.usual_range());
                if modulation.overshoot {
                    let (a, b) = match modulation.mode {
                        ModMode::Bipolar => (base - sweep.abs(), base + sweep.abs()),
                        ModMode::Unipolar => (base.min(base + sweep), base.max(base + sweep)),
                    };
                    let widened = (range.0.min(a).max(limit_min), range.1.max(b).min(limit_max));
                    if widened != range {
                        range = widened;
                        node.ranges.insert(name.clone(), [range.0, range.1]);
                    }
                }
                let span = crate::range_span(range);
                modulation.amount = if span > 0.0 {
                    (sweep * 100.0 / span)
                        .clamp(MODULATION_AMOUNT_LIMITS.0, MODULATION_AMOUNT_LIMITS.1)
                } else {
                    0.0
                };
                modulation.overshoot = false;
            }
        }
    }

    /// Graphs before version 4 let modulation carry values past the slider's range, so their
    /// entries (written out for connected parameters that had none) allow it.
    ///
    /// Graphs before version 3 gave modulation amounts in the parameter's own unit (octaves for
    /// frequencies); they are now percentages of its span. A connected parameter with no
    /// `modulation` entry used to get a default amount that depended on its base value (bipolar in
    /// version 1, unipolar in version 2); that is written out first so it keeps its meaning.
    fn upgrade_modulation(&mut self, registry: &Registry) {
        // Everything from version 6 on keeps its amounts as they are here.
        if self.version >= 6 {
            return;
        }
        let legacy_amounts = self.version < 3;
        let version = self.version;
        let octave = |kind: &str, param: &str| {
            OCTAVE_PARAMS
                .iter()
                .find(|o| o.kind == kind && o.param == param)
        };
        let old_default_mode = if self.version < 2 {
            ModMode::Bipolar
        } else {
            ModMode::Unipolar
        };
        for c in &self.connections {
            let Some((id, port)) = c.to.split_once('.') else {
                continue;
            };
            let Some(param) = port.strip_prefix(crate::graph::PARAM_PREFIX) else {
                continue;
            };
            let Some(node) = self.nodes.iter_mut().find(|n| n.id == id) else {
                continue;
            };
            let Some(spec) = registry.get(&node.kind).and_then(|t| {
                t.spec
                    .params
                    .iter()
                    .find(|s| s.name == param && s.modulatable)
            }) else {
                continue;
            };
            if let Some(base) = spec.number_value(&node.params) {
                let entry = if legacy_amounts {
                    Modulation {
                        amount: legacy_default_amount(
                            spec,
                            octave(&node.kind, param).is_some(),
                            base,
                        ),
                        mode: old_default_mode,
                        overshoot: false,
                    }
                } else {
                    spec.default_modulation()
                };
                node.modulation.entry(param.to_owned()).or_insert(entry);
            }
        }
        for node in &mut self.nodes {
            let kind = registry.get(&node.kind);
            for (name, modulation) in &mut node.modulation {
                let spec = kind.and_then(|k| k.spec.params.iter().find(|s| s.name == *name));
                if let (Some(spec), Some(o)) = (spec, octave(&node.kind, name)) {
                    // How far it moved in octaves, from whatever the amount meant then.
                    let reach = match modulation.mode {
                        ModMode::Bipolar => 0.5,
                        ModMode::Unipolar => 1.0,
                    };
                    let octaves = if version < 3 {
                        modulation.amount
                    } else {
                        let range = if version < 5 {
                            o.old
                        } else {
                            let ParamKind::Number { min, max, .. } = spec.kind else {
                                continue;
                            };
                            (min, max)
                        };
                        modulation.amount / 100.0 * (range.1 / range.0).log2() * reach
                    };
                    let base = spec.number_value(&node.params).unwrap_or(1.0);
                    modulation.amount = octaves_to_percent(spec, modulation.mode, base, octaves);
                } else if legacy_amounts && let Some(spec) = spec {
                    modulation.amount = percent_of_span(spec, *modulation);
                }
                // Before version 4 nothing kept modulated values to the slider's range.
                if version < 4 {
                    modulation.overshoot = true;
                }
            }
        }
    }

    fn rename_kinds(&mut self, renames: &[RenamedKind]) {
        for node in &mut self.nodes {
            if let Some(r) = renames.iter().find(|r| r.old == node.kind) {
                node.kind = r.new.to_owned();
            }
        }
    }

    fn rename_params(&mut self, renames: &[RenamedParam]) {
        for r in renames {
            for node in self.nodes.iter_mut().filter(|n| n.kind == r.kind) {
                if let Some(value) = node.params.remove(r.old) {
                    node.params.entry(r.new.to_owned()).or_insert(value);
                }
                if let Some(modulation) = node.modulation.remove(r.old) {
                    node.modulation
                        .entry(r.new.to_owned())
                        .or_insert(modulation);
                }
                for c in &mut self.connections {
                    if c.to == format!("{}.@{}", node.id, r.old) {
                        c.to = format!("{}.@{}", node.id, r.new);
                    }
                }
            }
        }
    }

    fn rename_ports(&mut self, renames: &[RenamedPort]) {
        for r in renames {
            let ids: Vec<String> = self
                .nodes
                .iter()
                .filter(|n| n.kind == r.kind)
                .map(|n| n.id.clone())
                .collect();
            for id in ids {
                let old = format!("{id}.{}", r.old);
                for c in &mut self.connections {
                    for end in [&mut c.from, &mut c.to] {
                        if *end == old {
                            *end = format!("{id}.{}", r.new);
                        }
                    }
                }
            }
        }
    }

    fn rename_choices(&mut self, renames: &[RenamedChoice]) {
        for r in renames {
            for node in self.nodes.iter_mut().filter(|n| n.kind == r.kind) {
                if let Some(ParamValue::Text(value)) = node.params.get_mut(r.param)
                    && value == r.old
                {
                    *value = r.new.to_owned();
                }
            }
        }
    }

    /// Parameters that only make sense whole were once free numbers: their saved values are
    /// rounded (and kept within the limits, which a node like Beat division may have raised).
    fn round_whole_parameters(&mut self, registry: &Registry) {
        for node in &mut self.nodes {
            let Some(kind) = registry.get(&node.kind) else {
                continue;
            };
            for spec in kind.spec.params.iter().filter(|s| s.integer) {
                if let Some(ParamValue::Number(n)) = node.params.get_mut(spec.name) {
                    let (lo, hi) = spec.number_limits().unwrap_or((f64::MIN, f64::MAX));
                    *n = n.round().clamp(lo, hi);
                }
                node.integer.retain(|name| name != spec.name);
            }
        }
    }

    /// Generators once had a `layout` parameter; it is a setting of the node now.
    fn move_layout_to_settings(&mut self, registry: &Registry) {
        for node in &mut self.nodes {
            if !registry
                .get(&node.kind)
                .is_some_and(|t| t.spec.takes_layout)
            {
                continue;
            }
            if let Some(ParamValue::Text(layout)) = node.params.remove("layout")
                && layout == "audio"
            {
                node.layout = GeneratorLayout::Audio;
            }
        }
    }

    fn rename_units(&mut self, registry: &Registry) {
        for node in &mut self.nodes {
            let has_unit = registry
                .get(&node.kind)
                .is_some_and(|t| t.spec.params.iter().any(|p| p.name == "unit"));
            if !has_unit {
                continue;
            }
            if let Some(ParamValue::Text(value)) = node.params.get_mut("unit")
                && let Some((_, new)) = RENAMED_UNITS.iter().find(|(old, _)| old == value)
            {
                *value = (*new).to_owned();
            }
        }
    }

    fn convert_modulation_inputs(&mut self, inputs: &[ModulationInput]) {
        for &ModulationInput { kind, param } in inputs {
            for node in self.nodes.iter_mut().filter(|n| n.kind == kind) {
                let depth = match node.params.remove("depth") {
                    Some(ParamValue::Number(depth)) => depth,
                    _ => 0.0,
                };
                let old = format!("{}.modulation", node.id);
                let mut connected = false;
                for c in self.connections.iter_mut().filter(|c| c.to == old) {
                    c.to = format!("{}.@{param}", node.id);
                    connected = true;
                }
                if connected {
                    node.modulation
                        .entry(param.to_owned())
                        .or_insert(Modulation {
                            amount: depth,
                            mode: ModMode::Bipolar,
                            overshoot: false,
                        });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How far a full-scale signal moves `param` of node `id`, in its unit (octaves for
    /// frequencies): what the amount used to be before amounts became percentages.
    fn sweep(graph: &GraphDesc, id: &str, param: &str) -> f64 {
        let node = graph.nodes.iter().find(|n| n.id == id).unwrap();
        let kind = Registry::shared().get(&node.kind).unwrap();
        let spec = kind.spec.params.iter().find(|s| s.name == param).unwrap();
        spec.modulation_sweep(
            node.modulation[param],
            node.ranges
                .get(param)
                .map_or_else(|| spec.usual_range(), |r| (r[0], r[1])),
        )
    }

    #[test]
    fn modulation_inputs_become_parameter_modulation() {
        let graph = GraphDesc::from_json(
            r#"{ "version": 1,
                "nodes": [
                    { "id": "a", "type": "audio_input" },
                    { "id": "wave", "type": "delay", "params": { "time": 1, "depth": 1.5 } },
                    { "id": "smear", "type": "lowpass", "params": { "depth": -3 } },
                    { "id": "crush", "type": "bitcrush", "params": { "depth": 2 } }
                ],
                "connections": [
                    { "from": "a", "to": "wave.modulation" },
                    { "from": "a", "to": "smear.modulation" }
                ] }"#,
        )
        .unwrap();
        let node = |id: &str| graph.nodes.iter().find(|n| n.id == id).unwrap();
        // The old depths are what a full-scale signal still moves the parameter by.
        assert!((sweep(&graph, "wave", "time") - 1.5).abs() < 1e-9);
        // Three octaves either way from the default of 40, averaged, in linear terms.
        assert!((sweep(&graph, "smear", "cutoff") + 157.5).abs() < 1e-9);
        // Depth without a connected signal did nothing, and is simply dropped.
        assert!(node("crush").params.is_empty() && node("crush").modulation.is_empty());
        let targets: Vec<&str> = graph.connections.iter().map(|c| c.to.as_str()).collect();
        assert_eq!(targets, ["wave.@time", "smear.@cutoff"]);

        // Upgrading again changes nothing.
        let mut again = graph.clone();
        again.upgrade();
        assert_eq!(again, graph);
    }

    #[test]
    fn version_1_parameter_connections_keep_their_bipolar_meaning() {
        let json = r#"{ "version": 1,
            "nodes": [
                { "id": "a", "type": "audio_input" },
                { "id": "d", "type": "delay", "params": { "time": 4 } },
                { "id": "g", "type": "gate", "modulation": { "threshold": { "amount": 1, "mode": "unipolar" } } }
            ],
            "connections": [
                { "from": "a", "to": "d.@time" },
                { "from": "a", "to": "g.@threshold" }
            ] }"#;
        let graph = GraphDesc::from_json(json).unwrap();
        assert_eq!(graph.version, FORMAT_VERSION);
        let node = |id: &str| graph.nodes.iter().find(|n| n.id == id).unwrap();
        assert_eq!(node("d").modulation["time"].mode, ModMode::Bipolar);
        // An entry that was already there is left alone.
        assert_eq!(node("g").modulation["threshold"].mode, ModMode::Unipolar);

        // The old default amount (half the base) is still what it moves by, in percent now.
        assert!((sweep(&graph, "d", "time") - 2.0).abs() < 1e-9);
        assert!((sweep(&graph, "g", "threshold") - 1.0).abs() < 1e-9);

        // Version 2 had the same default amount, one way. It is written out too.
        let v2 = GraphDesc::from_json(&json.replace("\"version\": 1", "\"version\": 2")).unwrap();
        let d = v2.nodes.iter().find(|n| n.id == "d").unwrap();
        assert_eq!(d.modulation["time"].mode, ModMode::Unipolar);
        assert!((sweep(&v2, "d", "time") - 2.0).abs() < 1e-9);

        // Current graphs without an entry stay unentried and get the default amount.
        let current = GraphDesc::from_json(
            &json.replace("\"version\": 1", &format!("\"version\": {FORMAT_VERSION}")),
        )
        .unwrap();
        assert!(
            current
                .nodes
                .iter()
                .find(|n| n.id == "d")
                .unwrap()
                .modulation
                .is_empty()
        );
    }

    #[test]
    fn graphs_that_overshot_get_a_wider_slider_range_instead() {
        let json = r#"{ "version": 3,
            "nodes": [
                { "id": "a", "type": "audio_input" },
                { "id": "f", "type": "lowpass", "modulation": { "cutoff": { "amount": 50 } } },
                { "id": "d", "type": "delay" }
            ],
            "connections": [ { "from": "a", "to": "f.@cutoff" }, { "from": "a", "to": "d.@time" } ] }"#;
        let graph = GraphDesc::from_json(json).unwrap();
        let node = |id: &str| graph.nodes.iter().find(|n| n.id == id).unwrap();
        assert_eq!(graph.version, FORMAT_VERSION);
        // The cutoff used to go as far as 50% of its 23 octave span from 40 (bipolar, so half of
        // it each way: 5.8 octaves, 40 x 2^5.8), past its 200 slider.
        let [_, high] = node("f").ranges["cutoff"];
        assert!(high > 200.0, "the slider now reaches {high}");
        let m = node("f").modulation["cutoff"];
        assert!(!m.overshoot && m.amount > 0.0 && m.amount <= 100.0);
        // A parameter that stayed inside its range keeps it.
        assert!(node("d").ranges.is_empty());
        assert_eq!(node("d").modulation["time"].amount, 25.0);
        assert!(!graph.to_json().contains("overshoot"));
        // Loading what was written is stable from then on.
        let once = GraphDesc::from_json(&graph.to_json()).unwrap();
        assert_eq!(GraphDesc::from_json(&once.to_json()).unwrap(), once);
    }

    #[test]
    fn both_ways_amounts_halve_and_slider_ranges_rescale_them() {
        // Version 8: 40% both ways was 20% either side of a 0..10 range, and the slider's own
        // range is what amounts are a percentage of now.
        let json = r#"{ "version": 8,
            "nodes": [
                { "id": "a", "type": "audio_input" },
                { "id": "d", "type": "delay", "ranges": { "time": [0, 5] },
                  "modulation": { "time": { "amount": 40, "mode": "bipolar" } } }
            ],
            "connections": [ { "from": "a", "to": "d.@time" } ] }"#;
        let graph = GraphDesc::from_json(json).unwrap();
        let d = &graph.nodes[1];
        let spec = &Registry::shared().get("delay").unwrap().spec.params[0];
        let (lo, hi) = (d.ranges["time"][0], d.ranges["time"][1]);
        // The old reach was 40% of the usual range (0..50), halved: 10 units each way.
        let usual = spec.modulation_span();
        let old_sweep = 0.4 * usual * 0.5;
        // Held to 100% of a 5 wide range, since an amount can't be more.
        assert_eq!(d.modulation["time"].amount, 100.0);
        assert!(old_sweep >= hi - lo);
    }

    #[test]
    fn modulation_amounts_are_converted_once() {
        let json = r#"{ "version": 2,
            "nodes": [
                { "id": "a", "type": "audio_input" },
                { "id": "f", "type": "lowpass", "modulation": { "cutoff": { "amount": 2 } } }
            ],
            "connections": [ { "from": "a", "to": "f.@cutoff" } ] }"#;
        let graph = GraphDesc::from_json(json).unwrap();
        assert_eq!(graph.version, FORMAT_VERSION);
        // Two octaves either way from the default of 40 (160 up, 10 down), in linear terms.
        assert!((sweep(&graph, "f", "cutoff") - 75.0).abs() < 1e-9);
        let mut again = graph.clone();
        again.upgrade();
        assert_eq!(again, graph, "upgrading a current graph changes nothing");
        let reloaded = GraphDesc::from_json(&graph.to_json()).unwrap();
        assert_eq!(reloaded, graph);
    }

    #[test]
    fn octave_amounts_become_linear_amounts_at_the_base_value() {
        let json = r#"{ "version": 4,
            "nodes": [
                { "id": "a", "type": "audio_input" },
                { "id": "f", "type": "lowpass",
                  "modulation": { "cutoff": { "amount": 25, "mode": "unipolar" } } },
                { "id": "g", "type": "gate",
                  "modulation": { "threshold": { "amount": 25, "mode": "unipolar" } } }
            ],
            "connections": [
                { "from": "a", "to": "f.@cutoff" },
                { "from": "a", "to": "g.@threshold" }
            ] }"#;
        let graph = GraphDesc::from_json(json).unwrap();
        assert_eq!(graph.version, FORMAT_VERSION);
        // 25% of the old (0.01..100000) octave span, from the default 40: 40 x 2^oct - 40.
        let octaves = 0.25 * (100_000.0f64 / 0.01).log2();
        let hertz = 40.0 * (octaves.exp2() - 1.0);
        assert!(hertz > 200.0, "more than the slider can show");
        // An amount can't be more than the whole slider range now, which is as far as it ever
        // reached before.
        let (lo, hi) = Registry::shared().get("lowpass").unwrap().spec.params[0].usual_range();
        assert!((sweep(&graph, "f", "cutoff") - (hi - lo)).abs() < 1e-6);
        // Other parameters, and version 4's overshoot setting, are untouched.
        let node = |id: &str| graph.nodes.iter().find(|n| n.id == id).unwrap();
        assert_eq!(node("g").modulation["threshold"].amount, 25.0);
        assert!(!node("f").modulation["cutoff"].overshoot);
        // Before version 3 the amount was in octaves already.
        let v2 = GraphDesc::from_json(
            &json
                .replace("\"version\": 4", "\"version\": 2")
                .replace("\"amount\": 25", "\"amount\": 3"),
        )
        .unwrap();
        assert!((sweep(&v2, "f", "cutoff") - 40.0 * 7.0).abs() < 1e-6);
        let mut again = v2.clone();
        again.upgrade();
        assert_eq!(again, v2);
    }

    #[test]
    fn old_frequency_unit_names_are_renamed() {
        let mut graph = GraphDesc::from_json(
            r#"{ "version": 1,
                "nodes": [
                    { "id": "f", "type": "filter", "params": { "unit": "cycles/frame" } },
                    { "id": "o", "type": "oscillator", "params": { "unit": "Hz" } },
                    { "id": "e", "type": "equalizer", "params": { "unit": "cycles/row" } },
                    { "id": "d", "type": "delay", "params": { "unit": "rows" } }
                ] }"#,
        )
        .unwrap();
        let unit = |graph: &GraphDesc, id: &str| {
            graph.nodes.iter().find(|n| n.id == id).unwrap().params["unit"].clone()
        };
        let text = |s: &str| ParamValue::Text(s.to_owned());
        assert_eq!(unit(&graph, "f"), text("frame"));
        assert_eq!(unit(&graph, "o"), text("second"));
        assert_eq!(unit(&graph, "e"), text("row"));
        assert_eq!(unit(&graph, "d"), text("row"));
        let once = graph.clone();
        graph.upgrade();
        assert_eq!(graph, once);
    }

    #[test]
    fn a_generators_layout_moves_from_its_parameters_to_its_settings() {
        let mut graph = GraphDesc::from_json(
            r#"{ "version": 6,
                "nodes": [
                    { "id": "a", "type": "beat", "params": { "layout": "audio", "shape": "phase" } },
                    { "id": "v", "type": "oscillator", "params": { "layout": "video" } },
                    { "id": "n", "type": "noise", "layout": "audio" }
                ] }"#,
        )
        .unwrap();
        let node =
            |graph: &GraphDesc, id: &str| graph.nodes.iter().find(|n| n.id == id).unwrap().clone();
        assert_eq!(node(&graph, "a").layout, GeneratorLayout::Audio);
        assert!(!node(&graph, "a").params.contains_key("layout"));
        assert!(node(&graph, "a").params.contains_key("shape"));
        assert_eq!(node(&graph, "v").layout, GeneratorLayout::Video);
        assert!(node(&graph, "v").params.is_empty());
        assert_eq!(node(&graph, "n").layout, GeneratorLayout::Audio);
        let once = graph.clone();
        graph.upgrade();
        assert_eq!(graph, once);
        assert!(graph.to_json().contains(r#""layout": "audio""#));
        assert!(
            !GraphDesc::from_json(&graph.to_json())
                .unwrap()
                .to_json()
                .contains("video")
        );
    }

    #[test]
    fn whole_number_parameters_are_rounded_on_load() {
        let graph = GraphDesc::from_json(
            r#"{ "version": 6,
                "nodes": [
                    { "id": "b", "type": "beat", "params": { "division": 0.5, "steps": 3.6 } },
                    { "id": "c", "type": "chorus", "params": { "voices": 2.4 }, "integer": ["voices", "time"] }
                ] }"#,
        )
        .unwrap();
        let number =
            |id: &str, name: &str| match graph.nodes.iter().find(|n| n.id == id).unwrap().params
                [name]
            {
                ParamValue::Number(n) => n,
                _ => unreachable!(),
            };
        assert_eq!(
            number("b", "division"),
            1.0,
            "raised to the lowest whole division"
        );
        assert_eq!(number("b", "steps"), 4.0);
        assert_eq!(number("c", "voices"), 2.0);
        // Only the parameters the user chose to round are listed; the node always rounds voices.
        let c = graph.nodes.iter().find(|n| n.id == "c").unwrap();
        assert_eq!(c.integer, ["time"]);
    }

    #[test]
    fn time_and_frequency_units_share_one_list_of_names() {
        let mut graph = GraphDesc::from_json(
            r#"{ "version": 6,
                "nodes": [
                    { "id": "d", "type": "delay", "params": { "unit": "rows" } },
                    { "id": "g", "type": "gate", "params": { "unit": "beats" } },
                    { "id": "l", "type": "lowpass", "params": { "unit": "Row" } },
                    { "id": "p", "type": "phaser", "params": { "unit": "Hertz" } },
                    { "id": "o", "type": "oscillator", "params": { "unit": "cycles/frame" } },
                    { "id": "r", "type": "reverb", "params": { "unit": "ms" } }
                ] }"#,
        )
        .unwrap();
        let unit = |graph: &GraphDesc, id: &str| {
            graph.nodes.iter().find(|n| n.id == id).unwrap().params["unit"].clone()
        };
        let text = |s: &str| ParamValue::Text(s.to_owned());
        assert_eq!(unit(&graph, "d"), text("row"));
        assert_eq!(unit(&graph, "g"), text("beat"));
        assert_eq!(unit(&graph, "l"), text("row"));
        assert_eq!(unit(&graph, "p"), text("second"));
        assert_eq!(unit(&graph, "o"), text("frame"));
        assert_eq!(unit(&graph, "r"), text("ms"));
        let once = graph.clone();
        graph.upgrade();
        assert_eq!(graph, once);
    }

    #[test]
    fn rgb_split_and_combine_ports_are_numbered() {
        let mut graph = GraphDesc::from_json(
            r#"{ "version": 2,
                "nodes": [
                    { "id": "s", "type": "split" },
                    { "id": "c", "type": "combine" },
                    { "id": "d", "type": "delay" }
                ],
                "connections": [
                    { "from": "s.b", "to": "c.r" },
                    { "from": "s.r", "to": "d" },
                    { "from": "d", "to": "c.g" }
                ] }"#,
        )
        .unwrap();
        let ends: Vec<(&str, &str)> = graph
            .connections
            .iter()
            .map(|c| (c.from.as_str(), c.to.as_str()))
            .collect();
        assert_eq!(ends, [("s.c3", "c.c1"), ("s.c1", "d"), ("d", "c.c2")]);
        let once = graph.clone();
        graph.upgrade();
        assert_eq!(graph, once);
    }

    #[test]
    fn renames_apply_to_kinds_parameters_and_their_modulation() {
        let mut graph = GraphDesc::from_json(
            r#"{ "version": 1,
                "nodes": [
                    { "id": "a", "type": "audio_input" },
                    { "id": "x", "type": "old_kind", "params": { "old_param": 2 },
                      "modulation": { "old_param": { "amount": 1 } } }
                ],
                "connections": [ { "from": "a", "to": "x.@old_param" } ] }"#,
        )
        .unwrap();
        graph.rename_kinds(&[RenamedKind {
            old: "old_kind",
            new: "new_kind",
        }]);
        let rename = [RenamedParam {
            kind: "new_kind",
            old: "old_param",
            new: "new_param",
        }];
        graph.rename_params(&rename);
        let node = &graph.nodes[1];
        assert_eq!(node.kind, "new_kind");
        assert_eq!(node.params["new_param"], ParamValue::Number(2.0));
        assert!(node.modulation.contains_key("new_param") && node.params.len() == 1);
        assert_eq!(graph.connections[0].to, "x.@new_param");
        // Running it again changes nothing.
        let once = graph.clone();
        graph.rename_params(&rename);
        assert_eq!(graph, once);
    }
}
