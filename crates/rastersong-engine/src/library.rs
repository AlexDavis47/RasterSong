//! Graph presets: the factory presets that ship with RasterSong, and the user's own, kept as graph
//! files in one folder (the user library). A preset is a starting point: using one copies its
//! graph into the project, so editing the project's graph never changes the preset.
//!
//! A preset is named after its file (`Beat Flash.json` is *Beat Flash*), and the graph's
//! `description` says what it does. Factory presets are built in from `presets/`.

use std::path::{Path, PathBuf};

use rastersong_graph::GraphDesc;

/// The extension of graph files, presets included.
pub const GRAPH_EXTENSION: &str = "json";

/// Where a preset comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetOrigin {
    /// Built into RasterSong.
    Factory,
    /// A file in the user library.
    User(PathBuf),
}

/// A graph to start from.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub name: String,
    pub graph: GraphDesc,
    pub origin: PresetOrigin,
}

impl Preset {
    /// What the preset does, as its graph says, or nothing.
    pub fn description(&self) -> &str {
        self.graph.description.as_deref().unwrap_or("")
    }
}

/// The factory presets' names and files, in the order they are offered: from the basics to the
/// more involved.
macro_rules! factory {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_str!(concat!("../presets/", $name, ".json")))),*]
    };
}

const FACTORY: &[(&str, &str)] = factory![
    "Start Here",
    "Pulse to the Beat",
    "Bass Brightness",
    "Color Bands",
    "Sound as Picture",
    "Picture as Sound",
    "Echo Trails",
    "Bit Crush",
    "Datamosh",
];

/// Every factory preset, in the order they are offered.
pub fn factory_presets() -> Vec<Preset> {
    FACTORY
        .iter()
        .map(|&(name, json)| Preset {
            name: name.to_owned(),
            graph: GraphDesc::from_json(json)
                .unwrap_or_else(|e| panic!("factory preset `{name}` doesn't read: {e}")),
            origin: PresetOrigin::Factory,
        })
        .collect()
}

/// The graph files in the user library `dir`, by name (case doesn't matter), and the files that
/// couldn't be read, with why. A folder that doesn't exist yet has no presets.
pub fn user_presets(dir: &Path) -> (Vec<Preset>, Vec<(PathBuf, String)>) {
    let mut presets = Vec::new();
    let mut failed = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (presets, failed);
    };
    for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
        if !path.is_file() || path.extension().is_none_or(|e| e != GRAPH_EXTENSION) {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        let graph = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|json| GraphDesc::from_json(&json).map_err(|e| e.to_string()));
        match graph {
            Ok(graph) => presets.push(Preset {
                name,
                graph,
                origin: PresetOrigin::User(path),
            }),
            Err(e) => failed.push((path, e)),
        }
    }
    presets.sort_by_key(|p| p.name.to_lowercase());
    failed.sort();
    (presets, failed)
}

/// The file a preset named `name` is saved to in `dir`: the name, with the characters file names
/// can't hold replaced by `_`. `None` for a name that is blank.
pub fn preset_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let file: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let file = file.trim_end_matches(['.', ' ']);
    (!file.is_empty()).then(|| dir.join(format!("{file}.{GRAPH_EXTENSION}")))
}

/// Saves `graph` as preset `name` in `dir` (made if missing), replacing a preset of that name.
/// Returns the file written.
pub fn save_preset(dir: &Path, name: &str, graph: &GraphDesc) -> std::io::Result<PathBuf> {
    let path = preset_path(dir, name).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "a preset needs a name")
    })?;
    std::fs::create_dir_all(dir)?;
    std::fs::write(&path, graph.to_json())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rastersong_graph::{CompileOptions, Graph, Layout, Registry};

    /// A fresh, empty folder for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rastersong-library-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn every_factory_preset_compiles_and_says_what_it_does() {
        let options = CompileOptions {
            frame_rate: 30.0,
            tempo: Default::default(),
            sources: [
                ("Video".to_owned(), Layout::rgb(16, 9)),
                ("Audio".to_owned(), Layout::audio(1600)),
            ]
            .into(),
            output: Layout::rgb(16, 9),
            pixel_scale: 1.0,
            span: Default::default(),
            audio_bus: "Main".to_owned(),
            master: true,
        };
        let presets = factory_presets();
        assert_eq!(presets.len(), FACTORY.len());
        for preset in presets {
            assert!(
                !preset.description().is_empty(),
                "`{}` has no description",
                preset.name
            );
            Graph::compile(&preset.graph, Registry::shared(), &options)
                .unwrap_or_else(|e| panic!("`{}` doesn't compile: {e}", preset.name));
            // A first graph opens without amber: notes are fine, warnings are not.
            for stats in Graph::inspect(&preset.graph, Registry::shared(), &options) {
                for d in &stats.diagnostics {
                    assert_ne!(
                        d.severity,
                        rastersong_graph::Severity::Warning,
                        "`{}`: {} warns: {}",
                        preset.name,
                        stats.node,
                        d.message
                    );
                }
            }
            // Each one teaches through its annotations.
            assert!(
                preset
                    .graph
                    .nodes
                    .iter()
                    .any(|n| n.kind == crate::COMMENT || n.kind == crate::NOTE),
                "`{}` explains nothing",
                preset.name
            );
        }
    }

    #[test]
    fn saved_presets_read_back_by_name() {
        let dir = scratch("save");
        let graph = GraphDesc::from_json(crate::PASSTHROUGH_GRAPH).unwrap();
        let path = save_preset(&dir, "My: Glow?", &graph).unwrap();
        assert_eq!(path.file_name().unwrap(), "My_ Glow_.json");
        save_preset(&dir, "another", &graph).unwrap();
        std::fs::write(dir.join("broken.json"), "{ nope").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a graph").unwrap();
        let (presets, failed) = user_presets(&dir);
        let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["another", "My_ Glow_"]);
        assert_eq!(presets[0].graph, graph);
        assert_eq!(
            presets[0].origin,
            PresetOrigin::User(dir.join("another.json"))
        );
        assert_eq!(failed.len(), 1, "the broken file is reported");
        // Saving under a name that is taken replaces it.
        let mut changed = graph.clone();
        changed.description = Some("changed".into());
        save_preset(&dir, "another", &changed).unwrap();
        assert_eq!(user_presets(&dir).0[0].graph, changed);
        assert!(save_preset(&dir, "  ", &graph).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_has_no_presets() {
        let (presets, failed) = user_presets(&scratch("missing"));
        assert!(presets.is_empty() && failed.is_empty());
    }
}
