//! Bringing graphs written for older versions of the nodes up to date.

use crate::ParamValue;
use crate::desc::{GraphDesc, ModMode, Modulation};

/// Nodes that used to have a `modulation` input and a `depth` parameter, and the parameter that
/// input moved. Parameter modulation does the same job now: a signal connected to `node.@param`
/// with the old depth as its amount.
const MODULATION_INPUTS: &[(&str, &str)] = &[
    ("delay", "time"),
    ("bitcrush", "bits"),
    ("lowpass", "cutoff"),
];

impl GraphDesc {
    /// Rewrites anything written for older node versions. Graphs already up to date are left
    /// as they are. [`GraphDesc::from_json`] calls it; call it on graphs deserialized any other
    /// way (e.g. inside a project file).
    pub fn upgrade(&mut self) {
        for &(kind, param) in MODULATION_INPUTS {
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
}
