//! Bringing graphs written for older versions of the nodes up to date.

use crate::ParamValue;
use crate::desc::{GraphDesc, ModMode, Modulation};

// Migrations are idempotent rewrites that recognise old graphs by their shape, so a graph that
// is already up to date passes through unchanged and `FORMAT_VERSION` stays 1. Bump the version
// only for a change to the file format itself (not to nodes), and keep the old shape readable.
//
// To rename or move something, add a row to one of the tables below and a test; never edit or
// remove an existing row, since files written long ago may still use it.

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

impl GraphDesc {
    /// Rewrites anything written for older node versions. Graphs already up to date are left
    /// as they are. [`GraphDesc::from_json`] calls it; call it on graphs deserialized any other
    /// way (e.g. inside a project file).
    pub fn upgrade(&mut self) {
        self.rename_kinds(RENAMED_KINDS);
        self.rename_params(RENAMED_PARAMS);
        self.rename_choices(RENAMED_CHOICES);
        self.convert_modulation_inputs(MODULATION_INPUTS);
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
                        });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(node("wave").modulation["time"].amount, 1.5);
        assert_eq!(node("smear").modulation["cutoff"].amount, -3.0);
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
        assert_eq!(unit(&graph, "f"), text("Frame"));
        assert_eq!(unit(&graph, "o"), text("Hertz"));
        assert_eq!(unit(&graph, "e"), text("Row"));
        assert_eq!(unit(&graph, "d"), text("rows"));
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
