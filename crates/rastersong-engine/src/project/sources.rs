//! What graphs read: the tracks their input nodes name, kept pointing at them through renames.
//! What a graph reads is set on its input nodes, not on its items. See
//! [Composition](../../../../docs/engine.md#composition).

use rastersong_graph::ParamValue;
use rastersong_graph::nodes::{AUDIO_INPUT, NAMED_INPUT, SOURCE_PARAM, TRACK_PARAM, VIDEO_INPUT};

use super::Project;
use crate::timeline::{Source, TrackKind};

impl Project {
    /// Whether any graph item plays: one on a graph track in the mix, not muted. Without one
    /// nothing is applied: the output is the plain mix.
    pub fn has_graph_items(&self) -> bool {
        let graphs = self.section(TrackKind::Graph);
        graphs
            .filter(|&i| self.in_mix(i))
            .any(|i| self.tracks[i].items.iter().any(|item| !item.muted))
    }

    /// Whether the preview skips every graph and plays the mix: no graph item plays.
    pub fn plays_track_mix(&self) -> bool {
        !self.has_graph_items()
    }

    /// Whether `source` names something the project has: a video or audio track for a stem, a
    /// graph track for a graph track's output. Whether a graph track is below the item reading
    /// it depends on where the item is, so it isn't checked.
    pub fn source_exists(&self, source: &Source) -> bool {
        let is = |name: &str, graph: bool| {
            self.track(name)
                .is_some_and(|t| (t.kind == TrackKind::Graph) == graph)
        };
        match source {
            Source::Track(name) => is(name, false),
            Source::Graph(track) | Source::Named { track, .. } => is(track, true),
            _ => true,
        }
    }

    /// Points every input reading track `old` (a stem or a graph track), in every graph, at
    /// `new`.
    pub(super) fn rename_sources(&mut self, old: &str, new: &str) {
        for node in self.graphs_mut().flat_map(|g| &mut g.nodes) {
            match node.kind.as_str() {
                VIDEO_INPUT | AUDIO_INPUT => {
                    let Some(ParamValue::Text(source)) = node.params.get_mut(SOURCE_PARAM) else {
                        continue;
                    };
                    for prefix in ["track:", "graph:"] {
                        if source.strip_prefix(prefix) == Some(old) {
                            *source = format!("{prefix}{new}");
                        }
                    }
                }
                NAMED_INPUT => {
                    if let Some(ParamValue::Text(track)) = node.params.get_mut(TRACK_PARAM)
                        && track == old
                    {
                        *track = new.to_owned();
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rastersong_graph::GraphDesc;

    use crate::composition::input_ports;

    fn project() -> Project {
        let graph = GraphDesc::from_json(
            r#"{ "version": 0, "nodes": [
                { "id": "v", "type": "video_input" },
                { "id": "a", "type": "audio_input" },
                { "id": "k", "type": "audio_input", "params": { "source": "track:kick" } },
                { "id": "o", "type": "output" } ] }"#,
        )
        .unwrap();
        let mut project = Project::new(graph);
        project.add_empty_track(TrackKind::Graph, "Graphs");
        project
    }

    #[test]
    fn renaming_a_track_follows_through_to_the_source_nodes_of_every_graph() {
        let mut project = project();
        project.add_track(TrackKind::Audio, "kick", "kick.wav");
        let lower = project.add_empty_track(TrackKind::Graph, "Lower");
        let stored = project.add_graph("B", None);
        let b = project.graph_mut(stored).unwrap();
        b.nodes
            .push(rastersong_graph::NodeDesc::new("g".into(), VIDEO_INPUT));
        b.nodes.last_mut().unwrap().params.insert(
            SOURCE_PARAM.to_owned(),
            ParamValue::Text(format!("graph:{lower}")),
        );
        let mut named = rastersong_graph::NodeDesc::new("n".into(), NAMED_INPUT);
        named
            .params
            .insert(TRACK_PARAM.to_owned(), ParamValue::Text(lower.clone()));
        named.params.insert(
            rastersong_graph::nodes::NAME_PARAM.to_owned(),
            ParamValue::Text("Glow".into()),
        );
        b.nodes.push(named);
        let kick = Source::Track("kick".into());
        assert!(project.source_exists(&kick));
        // A graph track's name isn't a stem, nor a stem's a graph track.
        assert!(!project.source_exists(&Source::Track(lower.clone())));
        assert!(!project.source_exists(&Source::Graph("kick".into())));
        assert!(project.rename_track("kick", "drum"));
        assert!(project.rename_track(&lower, "Bottom"));
        assert!(!project.source_exists(&kick));
        let sources = |desc: &GraphDesc| -> Vec<Source> {
            input_ports(desc)
                .into_iter()
                .map(|p| p.source)
                .filter(|s| *s != Source::Below)
                .collect()
        };
        assert_eq!(
            sources(project.graph(Project::FIRST_GRAPH).unwrap()),
            [Source::Track("drum".into())]
        );
        assert_eq!(
            sources(project.graph(stored).unwrap()),
            [
                Source::Graph("Bottom".into()),
                Source::Named {
                    track: "Bottom".into(),
                    name: "Glow".into()
                }
            ]
        );
        assert!(project.source_exists(&Source::Graph("Bottom".into())));
    }
}
