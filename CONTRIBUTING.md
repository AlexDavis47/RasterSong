# Contributing: critical development rules

Every contributor, human or agent, follows these. Each one is checkable. Details live in the linked documents;
this page is the short list. If a rule here and a document disagree, fix the document in the same change.

## Branches and merging

1. The 1.0 rebuild lives on `1.0-foundation`. **Nothing from it goes to `master`** and no PR targets `master`.
2. Work on a branch off `1.0-foundation`; merge back only when all of these pass: `cargo fmt --all --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo xtask docs --check`, `cargo test --workspace`
   (and `rustfmt --edition 2024 --check crates/rastersong-graph/src/nodes/*/*.rs`). See [Testing](docs/testing.md).
3. No migrations before 1.0: the file format is version 0 and breaking changes are allowed
   ([Decisions](docs/decisions.md#no-migrations-before-10-october-2026)).

## One implementation per job

4. **Before writing a widget, helper or code path, search for an existing one** (`rg` the crate, look in
   `crates/rastersong-gui/src/widgets/` and the shared modules). Reuse or extend it. Never copy-paste.
5. **Things of one kind share one component.** Every resource card (media, graphs, later more) is drawn by
   `resource_card`; every renamable name uses `name_edit`; channel counts are named by `channels_label`. A new
   kind gets a new `Card` value, not a new widget.
6. No parallel code paths for the same job (two ways to rename, label, drag, load). If a second one seems
   needed, change the first.
7. Don't use egui helpers whose behaviour you haven't checked against the interaction you need
   (`dnd_drag_source` senses drags only, so it never gets clicks or menus; use the `drag_source` in `resources.rs`).
   A menu that holds a text field closes on outside clicks only (`PopupCloseBehavior::CloseOnClickOutside`).

## Text, units and diagnostics

8. **All user-visible text lives in the lang files** (`crates/rastersong-lang/lang/`), looked up with `tr` /
   `tr_args`; no literals in code. The coverage test fails on unused or missing entries
   ([Text and languages](docs/text.md)).
9. Notes, not errors, for reinterpretation; warnings only for loss or ignored settings
   ([Decisions](docs/decisions.md)). Units follow [Signals & units](docs/signals-and-units.md).
10. Nodes are defined only through the `nodes!`/`params!` pattern ([Node authoring](docs/node-authoring.md));
    node docs are generated: run `cargo xtask docs` after changing a node.

## Tests

11. **Every behavior change or fix ships a test that fails without it** (unit test, `egui_kittest` interaction
    test, property test or golden, whichever the [Testing](docs/testing.md) table names for that layer). A bug
    fix includes a test that reproduces the bug.
12. GUI interactions are tested with real pointer events through `egui_kittest`, not only by calling helper
    methods, when the bug is about clicks, drags or menus.
13. Don't weaken or delete a test to make a change pass; change it only when the behavior it pins changed on
    purpose, and say so in the commit.

## Docs and roadmap

14. **Docs describe the code as it is.** A change that alters behavior updates the matching document
    ([index](docs/README.md)) in the same commit. A doc that disagrees with the code is a bug.
15. **Roadmap:** tick the item in [Roadmap](docs/roadmap.md) when it ships (with a link to the doc section), add
    items for work found but deferred, and tag them (**bug**, **feature**, **chore**).
16. Non-obvious design choices get an entry in [Decisions](docs/decisions.md) with the reason.

## Dependencies and licensing

17. Only LGPL-compatible dependencies; `cargo-deny` must pass. FFmpeg is the pinned shared LGPL build; never
    enable GPL or nonfree options ([Licensing](docs/licensing.md), [Development](docs/development.md)).

## Working style

18. Keep changes small and committed in logical steps with clear messages; leave the tree clean.
19. When a rule here is missing or wrong, edit this file in the same change rather than working around it.
