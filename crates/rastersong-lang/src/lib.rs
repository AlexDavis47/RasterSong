//! User-facing text, kept out of the code so wording and language can change without touching it.
//!
//! Text lives in string tables, plain `key = value` files keyed by stable ids such as
//! `node.delay.label` or `ui.timeline.loop`. English is embedded in the program and is the source
//! of truth; another locale is a folder of `.lang` files loaded at run time over it, and any key it
//! lacks falls back to English. Look text up with [`tr`] (or [`tr_args`] for text with `{name}`
//! placeholders); an id with no entry shows as the id itself and is recorded in [`missing`], so a
//! gap is visible instead of blank, and tests can assert there are none.
//!
//! File format: one `key = value` per line. `#` starts a comment line. A line that starts with
//! whitespace continues the previous value (joined by a space), so long text can be wrapped.
//! In a value `\n` is a line break, `\s` a space that survives at the edge of a value, and `\\` a
//! backslash.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock, RwLock};

/// The embedded English tables: file name and contents.
const ENGLISH: &[(&str, &str)] = &[
    ("nodes.lang", include_str!("../lang/en/nodes.lang")),
    ("ui.lang", include_str!("../lang/en/ui.lang")),
    ("messages.lang", include_str!("../lang/en/messages.lang")),
];

/// The locale code of the embedded text.
pub const ENGLISH_CODE: &str = "en";

/// What is wrong with a string table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LangError {
    /// A line that is not a comment, a continuation or `key = value`.
    BadLine { file: String, line: usize },
    /// The same key twice in one table.
    Duplicate {
        file: String,
        line: usize,
        key: String,
    },
    /// A locale folder could not be read.
    Io(String),
}

impl std::fmt::Display for LangError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadLine { file, line } => write!(f, "{file}:{line}: expected `key = value`"),
            Self::Duplicate { file, line, key } => {
                write!(f, "{file}:{line}: `{key}` is defined twice")
            }
            Self::Io(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for LangError {}

/// Parses one table into its `(key, value)` pairs, in file order.
pub fn parse(file: &str, source: &str) -> Result<Vec<(String, String)>, LangError> {
    let bad = |line| LangError::BadLine {
        file: file.to_owned(),
        line,
    };
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut seen = HashSet::new();
    for (index, raw) in source.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if raw.starts_with(char::is_whitespace) {
            let Some((_, value)) = entries.last_mut() else {
                return Err(bad(line));
            };
            value.push(' ');
            value.push_str(trimmed);
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            return Err(bad(line));
        };
        let key = key.trim();
        if key.is_empty() || key.contains(char::is_whitespace) {
            return Err(bad(line));
        }
        if !seen.insert(key.to_owned()) {
            return Err(LangError::Duplicate {
                file: file.to_owned(),
                line,
                key: key.to_owned(),
            });
        }
        entries.push((key.to_owned(), value.trim().to_owned()));
    }
    for (_, value) in &mut entries {
        *value = unescape(value);
    }
    Ok(entries)
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('s') => out.push(' '),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The text currently in use: every key's value, leaked once so lookups hand out `&'static str`.
/// Switching locale builds a new table (the old one stays allocated; locales change rarely).
#[derive(Debug)]
struct Table {
    code: String,
    entries: HashMap<String, &'static str>,
}

fn english_entries() -> Vec<(String, String)> {
    let mut all = Vec::new();
    for (file, source) in ENGLISH {
        all.extend(parse(file, source).unwrap_or_else(|e| panic!("embedded English text: {e}")));
    }
    all
}

fn build(code: &str, overlay: Vec<(String, String)>) -> &'static Table {
    let mut entries: HashMap<String, &'static str> = HashMap::new();
    for (key, value) in english_entries().into_iter().chain(overlay) {
        entries.insert(key, Box::leak(value.into_boxed_str()));
    }
    Box::leak(Box::new(Table {
        code: code.to_owned(),
        entries,
    }))
}

fn current() -> &'static RwLock<&'static Table> {
    static TABLE: OnceLock<RwLock<&'static Table>> = OnceLock::new();
    TABLE.get_or_init(|| RwLock::new(build(ENGLISH_CODE, Vec::new())))
}

fn missing_keys() -> &'static Mutex<HashMap<String, &'static str>> {
    static MISSING: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    MISSING.get_or_init(|| Mutex::new(HashMap::new()))
}

fn table() -> &'static Table {
    *current().read().expect("language table lock")
}

/// The text for `key`, or `None` if no table has it.
pub fn try_tr(key: &str) -> Option<&'static str> {
    table().entries.get(key).copied()
}

/// The text for `key`. A key with no entry comes back as the key itself and is recorded in
/// [`missing`].
pub fn tr(key: &str) -> &'static str {
    if let Some(text) = try_tr(key) {
        return text;
    }
    let mut missing = missing_keys().lock().expect("missing-key lock");
    missing
        .entry(key.to_owned())
        .or_insert_with(|| Box::leak(key.to_owned().into_boxed_str()))
}

/// The text for `key` with each `{name}` replaced by its value from `args`.
pub fn tr_args(key: &str, args: &[(&str, &str)]) -> String {
    let mut text = tr(key).to_owned();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

/// Keys looked up that no table had, sorted. Empty when all text is accounted for.
pub fn missing() -> Vec<String> {
    let mut keys: Vec<String> = missing_keys()
        .lock()
        .expect("missing-key lock")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// The locale code in use.
pub fn locale() -> &'static str {
    &table().code
}

/// Every key of the English tables, in file order: what other locales translate and what tests
/// check coverage against.
pub fn english_keys() -> Vec<String> {
    english_entries().into_iter().map(|(key, _)| key).collect()
}

/// The locales available in `dir` (each a subfolder of `.lang` files), plus English, sorted.
pub fn available_locales(dir: &Path) -> Vec<String> {
    let mut codes = vec![ENGLISH_CODE.to_owned()];
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            if entry.path().is_dir()
                && let Some(name) = entry.file_name().to_str()
                && name != ENGLISH_CODE
            {
                codes.push(name.to_owned());
            }
        }
    }
    codes.sort();
    codes
}

/// Switches to locale `code`, read from `dir/<code>/*.lang` over the embedded English. `"en"`
/// switches back to the embedded text. On an error the previous text stays in use.
pub fn set_locale(code: &str, dir: &Path) -> Result<(), LangError> {
    let overlay = if code == ENGLISH_CODE {
        Vec::new()
    } else {
        let folder = dir.join(code);
        let read = std::fs::read_dir(&folder)
            .map_err(|e| LangError::Io(format!("{}: {e}", folder.display())))?;
        let mut files: Vec<_> = read
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "lang"))
            .collect();
        files.sort();
        let mut all = Vec::new();
        for path in files {
            let source = std::fs::read_to_string(&path)
                .map_err(|e| LangError::Io(format!("{}: {e}", path.display())))?;
            all.extend(parse(&path.display().to_string(), &source)?);
        }
        all
    };
    *current().write().expect("language table lock") = build(code, overlay);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comments_continuations_and_escapes() {
        let entries = parse(
            "t",
            "# a comment\n\na.b = One\nc = long text\n  that wraps\nd = two\\nlines \\\\ done\ne = \\sx\\s\n",
        )
        .unwrap();
        assert_eq!(
            entries,
            [
                ("a.b".to_owned(), "One".to_owned()),
                ("c".to_owned(), "long text that wraps".to_owned()),
                ("d".to_owned(), "two\nlines \\ done".to_owned()),
                ("e".to_owned(), " x ".to_owned()),
            ]
        );
    }

    #[test]
    fn rejects_bad_lines_and_duplicates() {
        assert!(matches!(
            parse("t", "no equals"),
            Err(LangError::BadLine { line: 1, .. })
        ));
        assert!(matches!(
            parse("t", "  orphan"),
            Err(LangError::BadLine { line: 1, .. })
        ));
        assert!(matches!(
            parse("t", "a = 1\na = 2"),
            Err(LangError::Duplicate { line: 2, .. })
        ));
        assert!(matches!(
            parse("t", "a b = 1"),
            Err(LangError::BadLine { .. })
        ));
    }

    #[test]
    fn the_embedded_english_parses_without_duplicates() {
        let keys = english_keys();
        let unique: HashSet<_> = keys.iter().collect();
        assert_eq!(keys.len(), unique.len(), "a key is in two files");
    }

    #[test]
    fn a_missing_key_shows_as_itself_and_is_recorded() {
        assert_eq!(tr("test.no.such.key"), "test.no.such.key");
        assert!(missing().contains(&"test.no.such.key".to_owned()));
    }

    #[test]
    fn arguments_replace_every_placeholder() {
        // A key with no entry still goes through substitution, as itself.
        assert_eq!(
            tr_args("x {a} {a} {b}", &[("a", "1"), ("b", "2")]),
            "x 1 1 2"
        );
    }

    #[test]
    fn another_locale_overlays_english_and_falls_back() {
        let dir = std::env::temp_dir().join(format!("rastersong-lang-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("xx")).unwrap();
        std::fs::write(dir.join("xx/a.lang"), "test.only.in.xx = Salut\n").unwrap();
        assert_eq!(available_locales(&dir), ["en", "xx"]);
        set_locale("xx", &dir).unwrap();
        assert_eq!(tr("test.only.in.xx"), "Salut");
        assert_eq!(locale(), "xx");
        assert!(set_locale("zz", &dir).is_err());
        assert_eq!(locale(), "xx", "a failed switch keeps the previous text");
        set_locale("en", &dir).unwrap();
        assert_eq!(locale(), "en");
        let _ = std::fs::remove_dir_all(dir);
    }
}
