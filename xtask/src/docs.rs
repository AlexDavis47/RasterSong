//! `cargo xtask docs`: renders the node reference (`docs/nodes.md`) from the node registry, so a
//! node's description, ports and parameters are written once, in its source file.

use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

use anyhow::{Result, bail};
use rastersong_graph::{Category, NodeType, ParamKind, ParamSpec, Range, Registry, TagRule};

use crate::util::workspace_root;

/// Writes the reference, or with `check` fails if the file on disk is out of date.
pub fn generate(check: bool) -> Result<()> {
    let path: PathBuf = workspace_root().join("docs").join("nodes.md");
    let rendered = render(Registry::shared());
    if check {
        // Git may check files out with CRLF line endings.
        let current = fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        if current != rendered {
            bail!(
                "{} is out of date; run `cargo xtask docs` and commit the result",
                path.display()
            );
        }
        println!("{} is up to date", path.display());
    } else {
        fs::write(&path, rendered)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// The reference for every node type in `registry`.
pub fn render(registry: &Registry) -> String {
    let mut out = String::new();
    out.push_str(
        "# Node reference\n\n\
         Generated from the node definitions by `cargo xtask docs`; edit the node's source file, not \
         this page.\n\n",
    );

    let types = registry.types();
    for category in Category::ALL {
        let in_category: Vec<&&NodeType> = types
            .iter()
            .filter(|t| t.spec.category == category)
            .collect();
        if in_category.is_empty() {
            continue;
        }
        let _ = writeln!(out, "## {}\n", category.label());
        let _ = writeln!(out, "| Node | What it does |\n|---|---|");
        for t in &in_category {
            let _ = writeln!(
                out,
                "| [{}](#{}) | {} |",
                t.spec.label, t.kind, t.spec.description
            );
        }
        out.push('\n');
        for t in in_category {
            node(&mut out, t);
        }
    }
    out
}

fn node(out: &mut String, t: &NodeType) {
    let spec = &t.spec;
    let _ = writeln!(out, "### `{}`\n", t.kind);
    let _ = writeln!(out, "**{}**: {}\n", spec.label, spec.description);
    if !spec.doc.is_empty() {
        let _ = writeln!(out, "{}\n", spec.doc.trim());
    }
    if spec.per_channel {
        out.push_str("Can process R, G and B separately.\n\n");
    }

    if !spec.inputs.is_empty() {
        out.push_str("**Inputs**\n\n");
        for (i, input) in spec.inputs.iter().enumerate() {
            let kind = match (i, input.required) {
                (0, _) => "main, required",
                (_, true) => "required",
                (_, false) => "optional",
            };
            let _ = writeln!(out, "- `{}` ({kind}): {}", input.name, input.help);
        }
        out.push('\n');
    }
    out.push_str("**Outputs**\n\n");
    for output in spec.outputs {
        let carries = if output.tag.is_inherit() {
            String::new()
        } else {
            format!(" ({})", tag_rule_name(output.tag))
        };
        let _ = writeln!(out, "- `{}`{carries}: {}", output.name, output.help);
    }
    out.push('\n');

    if !spec.params.is_empty() {
        out.push_str("**Parameters**\n\n| Name | Default | Range | Modulation | What it does |\n|---|---|---|---|---|\n");
        for p in spec.params {
            let _ = writeln!(
                out,
                "| `{}` ({}) | {} | {} | {} | {} |",
                p.name,
                p.label,
                default(p),
                range(p),
                modulation(p),
                p.help
            );
        }
        out.push('\n');
    }
}

fn tag_rule_name(rule: TagRule) -> String {
    let mut words = Vec::new();
    if let Some(kind) = rule.kind {
        words.push(kind.label().to_owned());
    }
    if let Some(part) = rule.part.and_then(|p| p.label()) {
        words.push(part);
    }
    if let Some(range) = rule.range.and_then(Range::label) {
        words.push(range.to_owned());
    }
    words.join(", ")
}

fn number(n: f64) -> String {
    if n.is_infinite() {
        if n > 0.0 { "∞" } else { "-∞" }.to_owned()
    } else {
        format!("{n}")
    }
}

fn default(p: &ParamSpec) -> String {
    let unit = if p.unit.is_empty() {
        String::new()
    } else {
        format!(" {}", p.unit)
    };
    match p.kind {
        ParamKind::Number { default, .. } => format!("{}{unit}", number(default)),
        ParamKind::Choice { default, .. } | ParamKind::Text { default } => format!("`{default}`"),
    }
}

fn range(p: &ParamSpec) -> String {
    match p.kind {
        ParamKind::Number {
            min,
            max,
            limit_min,
            limit_max,
            ..
        } => {
            let usual = format!("{} to {}", number(min), number(max));
            if (limit_min, limit_max) == (min, max) {
                usual
            } else {
                format!(
                    "{usual} (up to {} to {})",
                    number(limit_min),
                    number(limit_max)
                )
            }
        }
        ParamKind::Choice { options, .. } => options
            .iter()
            .map(|o| format!("`{o}`"))
            .collect::<Vec<_>>()
            .join(", "),
        ParamKind::Text { .. } => "text".to_owned(),
    }
}

fn modulation(p: &ParamSpec) -> String {
    if p.modulatable {
        "yes".to_owned()
    } else if p.locked.is_empty() {
        "no".to_owned()
    } else {
        format!("no: {}", p.locked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_node_has_a_section() {
        let registry = Registry::shared();
        let page = render(registry);
        for t in registry.types() {
            assert!(
                page.contains(&format!("### `{}`", t.kind)),
                "{} missing",
                t.kind
            );
        }
        assert_eq!(page, render(registry), "rendering is deterministic");
    }
}
