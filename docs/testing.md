# Testing

Testing is a first-class part of the project. Every phase has tests that must pass before the next phase depends on it.

| Layer | What it proves | Tooling |
|---|---|---|
| Node unit tests | Each node's numbers: known inputs give known outputs (filter responses, shapes, ports and layouts), in the node's own file with `crate::testing` | `cargo test` |
| Node property tests | Processing in blocks of any size gives the same output as one large block; `reset()` + re-render is identical; no NaN/inf for valid inputs; every modulatable parameter keeps all of that while swept. Run for every node on its defaults and on its `TEST_CONFIGS`, read from the registry | `proptest` |
| Engine | Seek with warmup matches a render from frame 0 (exact for finite-memory nodes, within tolerance otherwise); graph edits and cancellation never serve a stale frame; output is deterministic; latency compensation aligns branches | fake media backend, no FFmpeg |
| Media correctness | Random-access decode of frame *i* is byte-identical to sequential decode of frame *i*, for every fixture. This works for any codec without hand-made expected outputs | generated fixtures |
| Fixtures | Small generated clips: B-frames, open GOP, variable frame rate, odd dimensions, rotation metadata, frame index encoded into lossless frames, audio-only, video-only, truncated files | `cargo xtask fixtures` (uses the `ffmpeg` installed by `cargo xtask fetch-ffmpeg`, not one on `PATH`) |
| End to end | CLI renders the example graphs; selected frames compared to golden images in `crates/rastersong-cli/tests/golden/` with tolerances for cross-platform floating-point differences. After an intended change, inspect the new frames and update them with `RASTERSONG_BLESS=1 cargo test -p rastersong-cli --test golden` | snapshot tests |
| Performance | Samples/sec per node, decode fps, full-graph fps, recorded in [benchmarks.md](benchmarks.md). Automated regression checks in CI are planned; shared CI runners are too noisy for tight thresholds | `criterion` |
| GUI | Graph ↔ editor conversion and timeline math are unit tested. Headless interaction tests (`crates/rastersong-gui/tests/app.rs`, `resources_panel.rs`) drive the real UI with real pointer events on the fake backend; every interaction a hint or tooltip promises gets one ([CONTRIBUTING](../CONTRIBUTING.md)), and `param_field.rs` does the same for the parameter field's track, value box and amount knob. `cargo test -p rastersong-gui --test screenshots -- --ignored` renders the UI offscreen (needs a GPU) to `target/tmp/screenshots/` for checking layout changes | `egui_kittest` |
| Robustness (later) | Malformed media never crashes the app | `cargo-fuzz` |

CI runs `clippy` (with `-D warnings`), the fixtures and tests, and the generated-docs check
(`cargo xtask docs --check`) on Windows, macOS and Linux, and `cargo xtask fmt --check` and `cargo-deny` on Linux.

`cargo fmt` doesn't reach the node files (they're declared through the `nodes!` macro), so formatting goes through
xtask: `cargo xtask fmt --check` checks the workspace and the node files, and `cargo xtask fmt` formats them.

## Tests the roadmap adds

Work in the [roadmap](roadmap.md) carries its own test obligations, so they aren't forgotten:

- From the first release, every migration step has a load test from an old graph (none exist while the format is version 0).
- Warmup cap: a seek with a small cap still renders a delay or feedback node's *full* effect length.
- Integer parameters: the property tests sweep them and assert only whole values reach `process`.
- Conditional parameters: a hidden parameter keeps its value and still round-trips through files.
- Percentage-based modulation: the same percentage gives the same fraction of the span on any parameter.
- New filters and distortion types: frequency-response or transfer-curve unit tests, plus `TEST_CONFIGS` and `BENCH`.
- Taps (inspection and listening) never change the rendered output or the cache (they are read-only probes).
- Routing: with no FX the output equals the plain mix; an audio FX on a folder hears the sum of its children; a graph
  with no Audio Output passes its track's audio through; a Control folder with master send off reaches no output.
- Timeline interactions: grouped items move together.
