//! Reusable rewrites for migration steps. Nothing calls them until the first release adds a step
//! (see the module docs), so they are only exercised by their tests for now.
#![allow(dead_code)]

use crate::{GraphDesc, ParamValue};

/// A node type that changed its name.
#[derive(Debug)]
pub struct RenamedKind {
    pub old: &'static str,
    pub new: &'static str,
}

/// A parameter that changed its name, on a node type that keeps its name.
#[derive(Debug)]
pub struct RenamedParam {
    pub kind: &'static str,
    pub old: &'static str,
    pub new: &'static str,
}

/// An input or output port that changed its name, on a node type that keeps its name.
#[derive(Debug)]
pub struct RenamedPort {
    pub kind: &'static str,
    pub old: &'static str,
    pub new: &'static str,
}

/// A choice parameter whose option was renamed, on a node type that keeps its name.
#[derive(Debug)]
pub struct RenamedChoice {
    pub kind: &'static str,
    pub param: &'static str,
    pub old: &'static str,
    pub new: &'static str,
}

impl GraphDesc {
    pub fn rename_kinds(&mut self, renames: &[RenamedKind]) {
        for node in &mut self.nodes {
            if let Some(r) = renames.iter().find(|r| r.old == node.kind) {
                node.kind = r.new.to_owned();
            }
        }
    }

    pub fn rename_params(&mut self, renames: &[RenamedParam]) {
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

    pub fn rename_ports(&mut self, renames: &[RenamedPort]) {
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

    pub fn rename_choices(&mut self, renames: &[RenamedChoice]) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FORMAT_VERSION;

    fn graph() -> GraphDesc {
        GraphDesc::from_json(&format!(
            r#"{{ "version": {FORMAT_VERSION},
                "nodes": [
                    {{ "id": "a", "type": "audio_input" }},
                    {{ "id": "x", "type": "old_kind", "params": {{ "old_param": 2, "mode": "old" }},
                      "modulation": {{ "old_param": {{ "amount": 1 }} }} }}
                ],
                "connections": [ {{ "from": "a", "to": "x.@old_param" }}, {{ "from": "x.p", "to": "a" }} ] }}"#
        ))
        .unwrap()
    }

    #[test]
    fn renames_apply_to_kinds_parameters_and_their_modulation() {
        let mut graph = graph();
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
        assert!(node.modulation.contains_key("new_param") && node.params.len() == 2);
        assert_eq!(graph.connections[0].to, "x.@new_param");
        // Running it again changes nothing.
        let once = graph.clone();
        graph.rename_params(&rename);
        assert_eq!(graph, once);
    }

    #[test]
    fn renames_apply_to_ports_and_choices() {
        let mut graph = graph();
        graph.rename_ports(&[RenamedPort {
            kind: "old_kind",
            old: "p",
            new: "q",
        }]);
        assert_eq!(graph.connections[1].from, "x.q");
        graph.rename_choices(&[RenamedChoice {
            kind: "old_kind",
            param: "mode",
            old: "old",
            new: "new",
        }]);
        assert_eq!(
            graph.nodes[1].params["mode"],
            ParamValue::Text("new".to_owned())
        );
    }
}
