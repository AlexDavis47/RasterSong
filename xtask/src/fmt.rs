//! `cargo xtask fmt`: the same formatting CI checks.

use std::fs;
use std::process::Command;

use anyhow::{Context, Result};

use crate::util::{run, workspace_root};

/// Formats the whole workspace, plus the node files that `cargo fmt` can't reach because the
/// `nodes!` macro declares them. With `check`, fails instead of writing.
pub fn format(check: bool) -> Result<()> {
    let root = workspace_root();
    let check_flag: &[&str] = if check { &["--check"] } else { &[] };

    run(Command::new("cargo")
        .args(["fmt", "--all"])
        .args(check_flag)
        .current_dir(&root))?;

    let nodes = root.join("crates/rastersong-graph/src/nodes");
    let mut files = Vec::new();
    for category in fs::read_dir(&nodes).with_context(|| format!("reading {nodes:?}"))? {
        let category = category?.path();
        if !category.is_dir() {
            continue;
        }
        for file in fs::read_dir(&category)? {
            let file = file?.path();
            if file.extension().is_some_and(|ext| ext == "rs") {
                files.push(file);
            }
        }
    }
    files.sort();

    run(Command::new("rustfmt")
        .args(["--edition", "2024"])
        .args(check_flag)
        .args(&files)
        .current_dir(&root))
}
