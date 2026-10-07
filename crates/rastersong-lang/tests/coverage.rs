//! The lang files and the code agree: every key the code asks for has English text, and every
//! interface or message key is asked for somewhere (node text is checked against the registry in
//! `rastersong-graph`'s tests).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n == "tests" || n == "target")
            {
                continue;
            }
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Every string literal in `text` made only of lowercase words joined by dots.
fn key_literals(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'"' && bytes[j] != b'\n' {
                j += if bytes[j] == b'\\' { 2 } else { 1 };
            }
            if j < bytes.len() && bytes[j] == b'"' {
                let literal = &text[start..j];
                let shaped = literal.contains('.')
                    && !literal.starts_with('.')
                    && !literal.ends_with('.')
                    && !literal.contains("..")
                    && literal.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.'
                    });
                if shaped {
                    found.push(literal.to_owned());
                }
                i = j;
            }
        }
        i += 1;
    }
    found
}

#[test]
fn the_code_and_the_english_text_agree() {
    let english: BTreeSet<String> = rastersong_lang::english_keys().into_iter().collect();
    // Node text is keyed by node kind, so the registry test covers it; the rest is asked for by id.
    let ids: BTreeSet<&String> = english
        .iter()
        .filter(|k| !k.starts_with("node.") && !k.starts_with("param."))
        .collect();
    let prefixes: BTreeSet<&str> = ids.iter().filter_map(|k| k.split('.').next()).collect();

    let own = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    let root = own.parent().unwrap().to_path_buf();
    let mut files = Vec::new();
    sources(&root, &mut files);
    let mut asked = BTreeSet::new();
    for file in files {
        if file.starts_with(&own) {
            continue;
        }
        // Windows checkouts may have CRLF line endings, which would defeat the split below.
        let text = std::fs::read_to_string(&file)
            .unwrap()
            .replace("\r\n", "\n");
        // Test modules come last in these files and use made-up keys.
        let code = text
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or(&text);
        for literal in key_literals(code) {
            if literal
                .split('.')
                .next()
                .is_some_and(|p| prefixes.contains(p))
            {
                asked.insert(literal);
            }
        }
    }

    let missing: Vec<_> = asked.iter().filter(|k| !english.contains(*k)).collect();
    assert!(
        missing.is_empty(),
        "the code asks for text that isn't in the lang files: {missing:#?}"
    );
    let unused: Vec<_> = ids.iter().filter(|k| !asked.contains(**k)).collect();
    assert!(
        unused.is_empty(),
        "lang entries nothing asks for: {unused:#?}"
    );
}
