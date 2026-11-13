//! Graph presets: the factory presets that ship with RasterSong, and the user's own, kept as graph
//! files in one folder (the user library). A preset is a starting point: using one copies its
//! graph into the project, so editing the project's graph never changes the preset.
//!
//! A preset is named after its file (`Beat Flash.json` is *Beat Flash*), and the graph's
//! `description` says what it does. Folders are categories: `tutorial/Learn The Basics.json` is
//! *Learn The Basics* in the *tutorial* category, and folders can nest. Factory presets are built
//! in from `presets/`.

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
    /// The folders the preset sits in, outermost first; empty at the top level.
    pub category: Vec<String>,
    pub graph: GraphDesc,
    pub origin: PresetOrigin,
}

impl Preset {
    /// What the preset does, as its graph says, or nothing.
    pub fn description(&self) -> &str {
        self.graph.description.as_deref().unwrap_or("")
    }
}

/// The factory presets' paths in `presets/` (without `.json`, folders as categories) and files, in
/// the order they are offered: from the basics to the more involved. A test checks this list
/// against the folder, so a preset file left off it fails the build's tests.
macro_rules! factory {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_str!(concat!("../presets/", $path, ".json")))),*]
    };
}

const FACTORY: &[(&str, &str)] = factory![
    "Start Here",
    "Tutorials/Learn The Basics",
    "Tutorials/Learn Control Signals",
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
        .map(|&(path, json)| {
            let mut category: Vec<String> = path.split('/').map(str::to_owned).collect();
            let name = category.pop().unwrap_or_default();
            Preset {
                graph: GraphDesc::from_json(json)
                    .unwrap_or_else(|e| panic!("factory preset `{path}` doesn't read: {e}")),
                name,
                category,
                origin: PresetOrigin::Factory,
            }
        })
        .collect()
}

/// Presets arranged by category, for menus and lists: each entry is a preset (its index in the
/// slice it came from) or a category holding more entries. Entries keep the order of the presets;
/// a category sits where its first preset would.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetEntry {
    Preset(usize),
    Category {
        name: String,
        entries: Vec<PresetEntry>,
    },
}

/// `presets` as a tree of categories; see [`PresetEntry`].
pub fn preset_tree(presets: &[Preset]) -> Vec<PresetEntry> {
    let mut tree = Vec::new();
    for (index, preset) in presets.iter().enumerate() {
        let mut level = &mut tree;
        for folder in &preset.category {
            let at = level
                .iter()
                .position(|e| matches!(e, PresetEntry::Category { name, .. } if name == folder))
                .unwrap_or_else(|| {
                    level.push(PresetEntry::Category {
                        name: folder.clone(),
                        entries: Vec::new(),
                    });
                    level.len() - 1
                });
            let PresetEntry::Category { entries, .. } = &mut level[at] else {
                unreachable!("found as a category")
            };
            level = entries;
        }
        level.push(PresetEntry::Preset(index));
    }
    tree
}

/// The graph files in the user library `dir` and the folders inside it (as categories), by
/// category and then name (case doesn't matter), and the files that couldn't be read, with why. A
/// folder that doesn't exist yet has no presets.
pub fn user_presets(dir: &Path) -> (Vec<Preset>, Vec<(PathBuf, String)>) {
    let mut presets = Vec::new();
    let mut failed = Vec::new();
    read_presets(dir, &[], &mut presets, &mut failed);
    presets.sort_by_cached_key(|p| {
        let mut key: Vec<String> = p.category.iter().map(|c| c.to_lowercase()).collect();
        key.push(p.name.to_lowercase());
        key
    });
    failed.sort();
    (presets, failed)
}

/// Reads the graph files in `dir`, in `category`, and the folders inside it.
fn read_presets(
    dir: &Path,
    category: &[String],
    presets: &mut Vec<Preset>,
    failed: &mut Vec<(PathBuf, String)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
        if path.is_dir() {
            if let Some(folder) = path.file_name().and_then(|s| s.to_str()) {
                let mut inner = category.to_vec();
                inner.push(folder.to_owned());
                read_presets(&path, &inner, presets, failed);
            }
            continue;
        }
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
                category: category.to_vec(),
                graph,
                origin: PresetOrigin::User(path),
            }),
            Err(e) => failed.push((path, e)),
        }
    }
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

    /// Every graph file under `presets/`, as its path without `.json`.
    fn preset_files(dir: &Path, prefix: &str, found: &mut Vec<String>) {
        for path in std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()) {
            let name = path.file_stem().unwrap().to_str().unwrap();
            if path.is_dir() {
                let name = path.file_name().unwrap().to_str().unwrap();
                preset_files(&path, &format!("{prefix}{name}/"), found);
            } else if path.extension().is_some_and(|e| e == GRAPH_EXTENSION) {
                found.push(format!("{prefix}{name}"));
            }
        }
    }

    #[test]
    fn every_file_in_the_presets_folder_is_a_factory_preset() {
        let mut files = Vec::new();
        preset_files(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("presets"),
            "",
            &mut files,
        );
        files.sort();
        let mut listed: Vec<String> = FACTORY.iter().map(|(p, _)| (*p).to_owned()).collect();
        listed.sort();
        assert_eq!(
            files, listed,
            "add each file in presets/ to FACTORY in library.rs, and only those"
        );
    }

    #[test]
    fn folders_are_categories() {
        let tutorial = factory_presets()
            .into_iter()
            .find(|p| p.name == "Learn The Basics")
            .unwrap();
        assert_eq!(tutorial.category, ["tutorial"]);

        let preset = |name: &str, category: &[&str]| Preset {
            name: name.to_owned(),
            category: category.iter().map(|&c| c.to_owned()).collect(),
            graph: GraphDesc::from_json(crate::PASSTHROUGH_GRAPH).unwrap(),
            origin: PresetOrigin::Factory,
        };
        let presets = [
            preset("a", &[]),
            preset("b", &["x"]),
            preset("c", &["x", "y"]),
            preset("d", &[]),
            preset("e", &["x"]),
        ];
        use PresetEntry::{Category, Preset as P};
        assert_eq!(
            preset_tree(&presets),
            [
                P(0),
                Category {
                    name: "x".into(),
                    entries: vec![
                        P(1),
                        Category {
                            name: "y".into(),
                            entries: vec![P(2)],
                        },
                        P(4),
                    ],
                },
                P(3),
            ]
        );
    }

    #[test]
    fn user_presets_read_from_folders_inside_the_library() {
        let dir = scratch("folders");
        let graph = GraphDesc::from_json(crate::PASSTHROUGH_GRAPH).unwrap();
        save_preset(&dir, "Top", &graph).unwrap();
        save_preset(&dir.join("Glitch").join("Heavy"), "Smear", &graph).unwrap();
        save_preset(&dir.join("Glitch"), "Crush", &graph).unwrap();
        let (presets, failed) = user_presets(&dir);
        assert!(failed.is_empty());
        let found: Vec<(Vec<String>, &str)> = presets
            .iter()
            .map(|p| (p.category.clone(), p.name.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                (vec!["Glitch".to_owned()], "Crush"),
                (vec!["Glitch".to_owned(), "Heavy".to_owned()], "Smear"),
                (vec![], "Top"),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
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
