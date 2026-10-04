//! Parameter modulation in the editor: which parameters show a pin on the node (exposed), and
//! the wires connected to them.
//!
//! A parameter's pin is an input port numbered [`PARAM_PORT`] + its index in the node's specs,
//! so it can't clash with the node's ordinary inputs. In graph files it's written `node.@name`.

use std::collections::BTreeSet;

use rastersong_engine::{ModMode, Modulation, ParamSpec};

use super::{EditorNode, GraphEditor, NodeKey};

/// The first port number used for parameter pins.
pub const PARAM_PORT: usize = 1 << 16;

/// The port of parameter `index`.
pub fn param_port(index: usize) -> usize {
    PARAM_PORT + index
}

/// The parameter index of `port`, if it's a parameter pin.
pub fn as_param(port: usize) -> Option<usize> {
    port.checked_sub(PARAM_PORT)
}

impl GraphEditor {
    fn specs_of(&self, node: &EditorNode) -> &'static [ParamSpec] {
        self.registry.get(&node.kind).map_or(&[], |k| k.spec.params)
    }

    /// Whether a signal is connected to parameter `index` of `key`.
    pub fn param_connected(&self, key: NodeKey, index: usize) -> bool {
        self.wires.iter().any(|w| w.to == (key, param_port(index)))
    }

    /// Whether parameter `index` of the node shows its pin: when the user (or the node type, by
    /// default) exposed it, or a signal is connected to it.
    pub fn param_exposed(&self, node: &EditorNode, index: usize) -> bool {
        let Some(spec) = self.specs_of(node).get(index) else {
            return false;
        };
        let chosen = match &node.exposed {
            Some(names) => names.contains(spec.name),
            None => spec.exposed,
        };
        spec.modulatable && (chosen || self.param_connected(node.key, index))
    }

    /// The parameters whose pins the node shows, by index.
    pub fn exposed_params(&self, node: &EditorNode) -> Vec<usize> {
        (0..self.specs_of(node).len())
            .filter(|&i| self.param_exposed(node, i))
            .collect()
    }

    /// Shows or hides parameter `index`'s pin. Hiding a connected parameter disconnects it.
    pub fn set_param_exposed(&mut self, key: NodeKey, index: usize, exposed: bool) {
        let Some(node) = self.node(key) else { return };
        let specs = self.specs_of(node);
        let Some(spec) = specs.get(index).filter(|s| s.modulatable) else {
            return;
        };
        // Start from what's shown now (the type's defaults, if the user hasn't chosen).
        let mut names: BTreeSet<String> = specs
            .iter()
            .enumerate()
            .filter(|(_, s)| match &node.exposed {
                Some(names) => names.contains(s.name),
                None => s.exposed,
            })
            .map(|(_, s)| s.name.to_owned())
            .collect();
        if exposed {
            names.insert(spec.name.to_owned());
        } else {
            names.remove(spec.name);
            self.disconnect_input((key, param_port(index)));
        }
        let defaults: BTreeSet<String> = specs
            .iter()
            .filter(|s| s.exposed)
            .map(|s| s.name.to_owned())
            .collect();
        let node = self.node_mut(key).unwrap();
        node.exposed = (names != defaults).then_some(names);
    }

    /// How parameter `index` of the node is modulated: its entry, or the default amount.
    pub fn modulation_of(&self, node: &EditorNode, index: usize) -> Modulation {
        let spec = &self.specs_of(node)[index];
        let base = spec.number_value(&node.params).unwrap_or(0.0);
        node.modulation
            .get(spec.name)
            .copied()
            .unwrap_or(Modulation {
                amount: spec.default_modulation_amount(base),
                mode: ModMode::Bipolar,
            })
    }
}

#[cfg(test)]
mod tests {
    use rastersong_engine::GraphDesc;

    use super::*;

    const GRAPH: &str = include_str!("../../../../examples/graphs/am_bands.json");

    fn editor_with_delay() -> (GraphEditor, NodeKey) {
        let mut editor = GraphEditor::new(&GraphDesc::from_json(GRAPH).unwrap());
        let delay = editor.add_node("delay", eframe::egui::Pos2::ZERO).unwrap();
        (editor, delay)
    }

    #[test]
    fn nodes_start_with_their_types_exposed_parameters() {
        let (editor, delay) = editor_with_delay();
        let node = editor.node(delay).unwrap();
        // Delay exposes time and feedback; mix is hidden; unit is a choice.
        assert_eq!(editor.exposed_params(node), [0, 2]);
    }

    #[test]
    fn exposing_and_hiding_parameters() {
        let (mut editor, delay) = editor_with_delay();
        editor.set_param_exposed(delay, 3, true);
        let node = editor.node(delay).unwrap();
        assert_eq!(editor.exposed_params(node), [0, 2, 3]);
        // Hiding a connected parameter disconnects it.
        let audio = editor.key_of("audio").unwrap();
        editor.connect((audio, 0), (delay, param_port(0)));
        assert!(editor.param_connected(delay, 0));
        editor.set_param_exposed(delay, 0, false);
        assert!(!editor.param_connected(delay, 0));
        let node = editor.node(delay).unwrap();
        assert_eq!(editor.exposed_params(node), [2, 3]);
        // Back to the defaults: nothing is stored.
        editor.set_param_exposed(delay, 0, true);
        editor.set_param_exposed(delay, 3, false);
        assert_eq!(editor.node(delay).unwrap().exposed, None);
        // A choice can't be exposed.
        editor.set_param_exposed(delay, 1, true);
        assert_eq!(editor.node(delay).unwrap().exposed, None);
    }

    #[test]
    fn parameter_wires_round_trip_through_graph_files() {
        let (mut editor, delay) = editor_with_delay();
        let audio = editor.key_of("audio").unwrap();
        editor.connect((audio, 0), (delay, param_port(2)));
        editor.node_mut(delay).unwrap().modulation.insert(
            "feedback".into(),
            Modulation {
                amount: 0.4,
                mode: ModMode::Unipolar,
            },
        );
        let desc = editor.to_desc();
        assert!(
            desc.connections
                .iter()
                .any(|c| c.from == "audio.out" && c.to == "delay.@feedback")
        );
        let again = GraphEditor::new(&desc);
        assert!(again.warnings.is_empty(), "{:?}", again.warnings);
        assert_eq!(again.to_desc(), desc);
        let node = again.node(again.key_of("delay").unwrap()).unwrap();
        assert_eq!(again.modulation_of(node, 2).amount, 0.4);
    }
}
