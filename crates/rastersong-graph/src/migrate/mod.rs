//! Bringing graphs written by older builds up to date.
//!
//! The format is version 0 until 1.0: breaking changes happen freely and old files may stop
//! loading, so there are no migration steps yet. At the first stable release, `FORMAT_VERSION`
//! becomes 1 and each later format change adds one file here (`v1_to_v2.rs`, ...) holding that
//! step, registered in [`STEPS`] in order. The reusable rewrites (renaming a node type, parameter,
//! port or choice) live in [`tooling`].

mod tooling;

use crate::desc::{FORMAT_VERSION, GraphDesc};

/// Rewrites a graph from one format version to the next. `STEPS[n]` takes version `n` to `n + 1`.
type Step = fn(&mut GraphDesc);

const STEPS: &[Step] = &[];

impl GraphDesc {
    /// Rewrites anything written for an older format version, one step at a time. Graphs already
    /// up to date are left as they are. [`GraphDesc::from_json`] calls it; call it on graphs
    /// deserialized any other way (e.g. inside a project file).
    pub fn upgrade(&mut self) {
        run_steps(self, STEPS, FORMAT_VERSION);
    }
}

/// Runs each step from the graph's version up to `target`, bumping the version after each.
fn run_steps(graph: &mut GraphDesc, steps: &[Step], target: u32) {
    while graph.version < target {
        let Some(step) = steps.get(graph.version as usize) else {
            break;
        };
        step(graph);
        graph.version += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Connection;

    fn graph(version: u32) -> GraphDesc {
        GraphDesc {
            version,
            nodes: Vec::new(),
            connections: Vec::new(),
        }
    }

    fn mark(graph: &mut GraphDesc, name: &str) {
        graph.connections.push(Connection {
            from: name.to_owned(),
            to: name.to_owned(),
        });
    }

    fn names(graph: &GraphDesc) -> Vec<&str> {
        graph.connections.iter().map(|c| c.from.as_str()).collect()
    }

    #[test]
    fn a_current_graph_passes_through_untouched() {
        let mut current = graph(FORMAT_VERSION);
        let before = current.clone();
        current.upgrade();
        assert_eq!(current, before);
    }

    #[test]
    fn steps_run_once_each_and_in_order() {
        let steps: &[Step] = &[
            |g| mark(g, "0to1"),
            |g| mark(g, "1to2"),
            |g| mark(g, "2to3"),
        ];
        let mut old = graph(0);
        run_steps(&mut old, steps, 3);
        assert_eq!(names(&old), ["0to1", "1to2", "2to3"]);
        assert_eq!(old.version, 3);

        // Starting part way skips the earlier steps, and a second run changes nothing.
        let mut middle = graph(1);
        run_steps(&mut middle, steps, 3);
        assert_eq!(names(&middle), ["1to2", "2to3"]);
        run_steps(&mut middle, steps, 3);
        assert_eq!(names(&middle), ["1to2", "2to3"]);
    }
}
