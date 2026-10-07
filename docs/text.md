# Text and languages

All user-facing text lives in string tables, not in code, so wording and language can change without touching it.
The tables and their loader are the `rastersong-lang` crate (`crates/rastersong-lang`).

## The tables

Plain `key = value` files, one per line. `#` starts a comment; a line that starts with whitespace continues the
previous value, joined by a space, so long text can be wrapped; `\n` is a line break, `\s` a space that survives
at the edge of a value, `\\` a backslash. `{name}` marks a value the program fills in.

English is embedded in the program and is the source of truth (`crates/rastersong-lang/lang/en/`):

| File | Holds |
|---|---|
| `nodes.lang` | Every node's label, description, ports and parameters, keyed by node kind |
| `ui.lang` | Menus, dialogs, the graph editor, inspector, timeline and settings |
| `messages.lang` | Node categories, signal descriptions, errors and diagnostics |

## Keys

Keys are stable ids of lowercase words joined by dots.

- Nodes: `node.<kind>.label`, `.description`, optional `.doc`, `node.<kind>.input.<port>`,
  `node.<kind>.output.<port>`, `node.<kind>.param.<name>.label`, `.help`, and `.locked` (why a parameter can't be
  modulated). A parameter with no entry of its own falls back to `param.<name>.*` (today only `mix`).
- Everything else: an area, then the thing: `menu.file.open`, `timeline.track.add`, `error.graph.cycle`.

## In code

```rust
use rastersong_lang::{tr, tr_args};

ui.button(tr("menu.file.open"));
tr_args("dialog.save_changes.title", &[("name", &name)]);   // "Save changes to {name}?"
node_type.label();                                          // node text goes through NodeType
```

`tr` returns `&'static str`. A key with no entry comes back as the key itself and is recorded
(`rastersong_lang::missing()`), so a gap shows up as an id rather than blank text. Node specs (`NodeSpec`,
`ParamSpec`, `InputSpec`, `OutputSpec`) carry names and numbers only; read their text through `NodeType` (`label`,
`description`, `doc`, `param_label`, `param_help`, `param_locked`, `input_help`, `output_help`).

Don't build sentences by joining pieces: give the whole sentence a key with placeholders, since other languages
order words differently. Plurals get one key per form (`editor.menu.copy`, `editor.menu.copy_many`).

## Checks

- `rastersong-lang/tests/coverage.rs`: every key the code asks for exists in English, and every interface or message
  entry is asked for somewhere (no stale text).
- `the_lang_files_cover_every_node` (in `node_properties.rs`): every registered node, port and parameter has its
  entries, and no `node.*` key names something that no longer exists.
- `cargo xtask docs` reads node text through the same accessors, so [nodes.md](nodes.md) follows the tables.

## Other languages

A language is a folder of `.lang` files under the `lang` folder next to the program (`lang/<code>/*.lang`), using
the same keys. It overlays English: a key it lacks shows in English. The Settings window (Application → Language)
lists the folders found and switches at once; the choice is remembered between sessions.

Not yet moved: messages from the media layer and the command-line tool, and text in the number formatting (units
like `Hz`, `fr`).
