//! Undo and redo, as snapshots of the whole project.
//!
//! A project is small (a graph description and a few tracks), so a snapshot per edit is cheap and
//! can't drift out of sync with the thing it restores. The app records a snapshot whenever the
//! project differs from the last one and the user isn't in the middle of a gesture, so a drag, a
//! slider move or a typed name becomes one step.

use rastersong_engine::Project;

/// Snapshots kept for undoing.
const MAX_STEPS: usize = 200;

#[derive(Debug, Clone)]
pub struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
    /// The latest recorded state of the project.
    current: Project,
}

impl History {
    pub fn new(project: Project) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            current: project,
        }
    }

    /// Records `project` as a new step if it differs from the last one. Returns whether it did.
    pub fn record(&mut self, project: &Project) -> bool {
        if *project == self.current {
            return false;
        }
        let previous = std::mem::replace(&mut self.current, project.clone());
        self.undo.push(previous);
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
        self.redo.clear();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The project as it was one step back, if there is one.
    pub fn undo(&mut self) -> Option<&Project> {
        let previous = self.undo.pop()?;
        self.redo
            .push(std::mem::replace(&mut self.current, previous));
        Some(&self.current)
    }

    /// The project as it was before the last undo, if there is one.
    pub fn redo(&mut self) -> Option<&Project> {
        let next = self.redo.pop()?;
        self.undo.push(std::mem::replace(&mut self.current, next));
        Some(&self.current)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rastersong_engine::{GraphDesc, ProjectTrack};

    use super::*;

    fn project(video: &str) -> Project {
        let mut project =
            Project::new(GraphDesc::from_json(r#"{ "version": 0, "nodes": [] }"#).unwrap());
        project
            .video_tracks
            .push(ProjectTrack::new("video".into(), PathBuf::from(video)));
        project
    }

    #[test]
    fn undoes_and_redoes_in_order() {
        let mut history = History::new(project("a"));
        assert!(!history.record(&project("a")), "unchanged isn't a step");
        assert!(history.record(&project("b")));
        assert!(history.record(&project("c")));
        assert_eq!(history.undo(), Some(&project("b")));
        assert_eq!(history.undo(), Some(&project("a")));
        assert_eq!(history.undo(), None);
        assert_eq!(history.redo(), Some(&project("b")));
        assert_eq!(history.redo(), Some(&project("c")));
        assert_eq!(history.redo(), None);
    }

    #[test]
    fn a_new_edit_drops_the_redo_steps() {
        let mut history = History::new(project("a"));
        history.record(&project("b"));
        history.undo();
        assert!(history.can_redo());
        history.record(&project("x"));
        assert!(!history.can_redo());
        assert_eq!(history.undo(), Some(&project("a")));
    }

    #[test]
    fn keeps_a_bounded_number_of_steps() {
        let mut history = History::new(project("0"));
        for i in 1..=MAX_STEPS + 50 {
            history.record(&project(&i.to_string()));
        }
        let mut steps = 0;
        while history.undo().is_some() {
            steps += 1;
        }
        assert_eq!(steps, MAX_STEPS);
    }
}
