//! `cargo xtask docs`: renders the node reference (`docs/nodes.md`) from the node registry, so a
//! node's description, ports and parameters are written once, in its source file.

use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

use anyhow::{Result, bail};
use rastersong_graph::{Category, ModScale, NodeType, ParamKind, ParamSpec, PortHint, Registry};

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
        let carries = match output.hint {
            PortHint::Inherit => String::new(),
            hint => format!(" ({})", hint_name(hint)),
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

fn hint_name(hint: PortHint) -> &'static str {
    match hint {
        PortHint::Inherit => "same as the main input",
        PortHint::Rgb => "RGB video",
        PortHint::Red => "red channel",
        PortHint::Green => "green channel",
        PortHint::Blue => "blue channel",
        PortHint::Audio => "audio",
        PortHint::Low => "low band",
        PortHint::Mid => "mid band",
        PortHint::High => "high band",
        PortHint::AsAudio => "as audio",
        PortHint::AsVideo => "as video",
    }
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

fn modulation(p: &ParamSpec) -> &'static str {
    match (p.modulatable, p.scale) {
        (false, _) => "no",
        (true, ModScale::Linear) => "yes",
        (true, ModScale::Octaves) => "yes, in octaves",
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
