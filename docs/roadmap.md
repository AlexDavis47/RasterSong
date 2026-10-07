# Roadmap

What has shipped, what is next, and the backlog from hands-on testing, grouped by workstream. Decisions and their
reasons are in [Decisions](decisions.md); how things work today is in the other documents (start at the
[index](README.md)).

Each phase ends with its tests passing in CI. A checked box (`[x]`) is done; `[ ]` is open. Items tagged **bug**
are defects, **feature** is new behavior, **chore** is cleanup or refactoring with no new behavior. A tag in
*italics* after an item says what it depends on.

## Phases

**Phase 0: Foundation** (done)
- Tag the prototype as `prototype-2025`, start a fresh workspace with the [crate layout](engine.md#crate-layout)
- `cargo xtask fetch-ffmpeg`, `cargo xtask fixtures`
- CI on all three platforms: fmt, clippy, tests, `cargo-deny`
- `tracing` for logging
- Add the LICENSE file

**Phase 1: Media** (done)
- Backend trait and fake backend
- FFmpeg backend: packet index, frame-index API, sequential fast path, seeking, EOF flush, scaler reuse
- Full audio decode to `f32`
- Exit: media correctness tests pass on all fixtures

**Phase 2: Graph + CLI** (done)
- `Signal` and `Layout`, rate matching with interpolation modes, compiled sequential schedule, latency compensation
- Nodes: video input, audio input, Split, Combine, Interleave, three-band splitter, AM, Delay, Bit crush, Low pass, Output
- `rastersong-cli render <video> <audio> <graph> <out>`
- Exit: node property tests and first golden end-to-end renders pass. **This is where the core idea is proven.**

**Phase 3: Render Engine** (done)
- Background render-ahead thread, frame cache, warmup on seek, cancellation, playback clock with dynamic speed, preview scale
- Exit: engine tests pass; benchmarks recorded

**Phase 4: GUI** (in progress; the workstreams below are what remains)
- egui viewer driven only by the engine
- Node editor and parameter panels
- Minimal timeline: one video track, one audio track, with an offset
- Buffer health and playback speed indicators

**Phase 5: Export & Polish**
- Export (H.264 via OS/hardware encoders, ProRes, FFV1) with progress
- State snapshots for faster seeking with stateful graphs
- SIMD and parallelism work guided by benchmarks
- Packaging and installers

## 1.0 requirements

- [x] Undo/redo (graph edits, timeline edits, parameter changes)
- [x] Copy and paste (nodes with their internal connections; see the bug below)
- [x] New nodes: compressor, gate, distortion
- [x] Node parameter inputs (modulation)
- [ ] Automation clips (see [Timeline](#timeline))
- [ ] The foundation workstream below (parameter types, units, modulation, text), since most node work depends on it
- [ ] Look/Listen tools and the settings page

## Suggested order

Many items depend on others. Doing them in this order avoids reworking nodes twice:

1. **Foundations**: integer parameters, conditional parameters, shared node settings (layout, blend), unit cleanup,
   percentage modulation, removal of log/exponential sliders, lang files, the inspector-growth fix. Every node
   change below builds on these.
2. **Project and settings**: the Settings page, max warmup frames.
3. **Editor fixes**: copy/paste connections, tooltips, timeline Fit removal, performance display.
4. **Shared UI and metering components**, then **Look/Listen tools** (they reuse the same widgets and the Audio
   Output sink).
5. **Node work**, starting with shared DSP (filter slope, detectors) so Filter, EQ, Three-Band Split, DC Filter and
   the dynamics nodes use one implementation.

Renames, merges, splits and removals need **no migration** while the format is version 0 (see
[Decisions](decisions.md#no-migrations-before-10-october-2026)); migrations start at 1.0.

---

## Node settings and parameters

### Parameter types

- [x] **feature** Real integer parameters. A parameter that can only be whole (channel counts, divisions, steps,
  voices, band counts) is declared as an integer in its spec. The slider and the value box always snap to whole
  values; nothing between integers is ever shown or accepted, and floats are never allowed for these. *Fixes: Beat
  division, Pack channels, chorus voices, and every "int locked" slider that still lets the handle sit between
  values before snapping.*
- [x] **chore** Audit every number parameter and mark the integer ones (Beat division and steps, Pack channels,
  Chorus voices, Bit Crush bits if whole, Quantize levels, and so on). The property tests sweep them and assert
  only whole values reach `process`.
- [x] **feature** Conditional parameters: a declarative "shown when" rule on `ParamSpec`
  (`.shown_when(Self::MODE, Mode::Step)`). The inspector and the generated docs hide the parameter when the rule
  fails; a hidden parameter keeps its value and still saves. *Needed by: Beat steps and width, Oscillator pulse
  width, Filter slope, Distortion character, and any node whose mode changes which controls matter.*
- [x] **bug** Unmodulatable parameters have no explanation. *Done: see [Node authoring](node-authoring.md#parameters); only Pack channels and Resample width and height stay locked, with their reason shown in the inspector.* It is not about warmup. `.fixed()` covers three
  different reasons (structural, precomputed in `prepare`, or simply not implemented; the breakdown is in
  [Node authoring](node-authoring.md#parameters)). **Policy ([Decisions](decisions.md#modulation-is-allowed-unless-infeasible-october-2026)):
  this is datamoshing software, so a parameter is locked only when modulating it is truly infeasible or
  unreasonably hard.** Steps:
  1. Replace `.fixed()` with `.fixed("reason")`, shown in the inspector as a disabled pin with a tooltip. A
     reason that is only "nobody wrote it" is not accepted.
  2. Make the "not implemented" group modulatable, with a per-sample coefficient like the Phaser's: Envelope
     attack/release, Slew rise/fall, Limiter release, Beat width and division, Oscillator phase, Chorus spread,
     Reverb pre-delay, Three-Band Split crossovers (recompute the filter per sample or per small block).
  3. Make the "structural" group modulatable by allocating the maximum and rounding the value per sample: Chorus
     voices, Phaser stages, Beat steps, noise seed. Integer parameters round, not truncate, and the modulation
     range snaps to whole values.
  4. Reverb size and damping: attempt it (crossfading between delay-line lengths); lock with a reason only if it
     cannot be made to sound reasonable.
  5. What stays locked, with its reason: Pack channels and anything else that changes the output *layout* (the
     graph is compiled for a fixed layout, so it can't change per sample).

### Modulation

- [x] **feature** Modulation amount is a **percentage of the parameter's span** (done): 100% sweeps the whole
  usual range, both ways as the swing from lowest to highest. The inspector's knob and number show percent, the
  tooltip shows what it comes to in the parameter's unit, the outlined range is computed from it, and a new
  connection starts at 25%, one way. Graph format version 3 migrates older graphs, writing out the old base-dependent
  default amount, so they move parameters exactly as before. *Interacts with
  [Later: custom response curves and minimum/maximum](#later).*
- [x] Octave modulation was dropped: frequencies modulate linearly like every other parameter (a percentage of their
  linear span), which keeps the knob, the amount boxes and the slider's shaded range in agreement.
- [x] **feature** Remove exponential and logarithmic parameter sliders entirely. All sliders are linear. Frequency
  parameters keep their units (Hz, Row, …); neither the control nor the modulation is warped.
  Drops the "logarithmic for wide ranges" behavior in the [inspector](app.md#inspector).

### Units and parameter semantics

- [x] **chore** **Merge `TimeUnit` and `FreqUnit` into one `Unit`** (see
  [Decisions](decisions.md#one-unit-type-for-time-and-frequency-october-2026)). They are the same domain: both
  answer "how many samples is one of these", and a frequency is a time inverted. Today they are two enums with two
  label sets (`rows, frames, ms, seconds, beats, bars` against `Row, Frame, Hertz, Beat, Bar`), which is the
  inconsistency users see. One enum with `samples_per_unit(ctx)`, one `Unit::param`, one label set; time
  parameters multiply, frequency parameters divide and their label says "cycles per". `Hertz` becomes `second`.
  Migration maps every old option name.
- [x] **feature** New **pixel** and **sample** units (the "users never see samples" principle is dropped).
  *Decided ([Decisions](decisions.md#pixel-is-a-project-pixel-sample-is-the-renders-sample-october-2026)):
  a pixel is a project-resolution pixel, scaled with the preview so downscaled previews match the full render as
  closely as possible; a sample is the literal sample at the current render size, and its help text says it changes
  with preview scale.*
- [x] **chore** Keep the optional per-node `mix`, but build it once: one shared `MIX` parameter definition and one
  dry/wet helper in `dsp.rs`, used by every node that has it. No removal or migration.

### Shared node settings

- [x] **feature** Move **layout** out of per-node parameters and into the shared node settings (next to
  Resampling, Grouping and Channels). Beat, Constant, Noise and Oscillator all carry a `layout` parameter today.
  One shared implementation (`GeneratorLayout`) and one settings control.
- [x] **bug** Layout "audio" never works: Beat and Oscillator fail with `⚠ Beat: the host provides no 'audio'
  source to take a layout from`. Fix the host to supply an audio source layout to the compile step (or to fall
  back to a sensible default with a note when there is no audio track), and test it through the engine, not only
  the graph tests.

### Text and localization

- [ ] **chore** Move all user-facing text into **lang files** so locale and wording can be changed without touching
  code. Covers: node labels, descriptions, parameter labels and help text, port help, choice labels, menu and
  button text, tooltips, notes and warnings, dialogs and errors. Proposed shape: a string table keyed by stable ids
  (`node.delay.label`, `node.delay.param.time.help`, `ui.timeline.loop`), `en` embedded at build time as the
  source of truth, other locales loadable at runtime, missing keys fall back to `en`. A registry test fails when a
  node, parameter or port has no `en` entry; `cargo xtask docs` reads the same table so [nodes.md](nodes.md) stays
  in step. Evaluate Fluent against a simple key/value format first (plurals and units need some formatting;
  keep it as small as that allows). Do this early: every new string in this roadmap should go in a lang file from
  the start.
- [ ] **chore** Settings page picks the language (see [Project and settings](#project-and-settings)).

---

## Project and settings

- [ ] **feature** **Settings page.** A real Settings window (File → Settings, and a toolbar button) so hidden
  settings can be exposed. *Partly done:* the window (File → Settings…, Ctrl+,), its Application and Project pages
  and the settings that already existed (theme, wire style, node stats, keep connections, tempo, audio rate) are in;
  what is left is language, cache budget, render-ahead, default tool, the toolbar button and
  the per-setting reset. Two scopes, clearly separated:
  - *Application* (remembered between sessions, not in project files): theme, wire style, language, default
    preview resolution, cache budget (1 GiB today), render-ahead window (10 s today), default tool, keep input
    connections when duplicating and pasting.
  - *Project* (saved in the `.rastersong` file): tempo, beats per bar, first-beat offset, audio rate (48 kHz
    today), **max warmup frames** (done), and later export defaults.
  Each setting has help text. Anything currently only changeable by editing a file or a constant should appear here
  or be consciously left out.
- [x] **feature** **Max warmup frames as a project setting** (default 120, range 0–9999). Nodes now report their
  real length and the engine applies the project's limit, so a small limit makes seeks less accurate for
  long-memory nodes but never shortens or alters what a node does. A test renders a feedback graph from the start
  with several limits and gets identical frames; only a seek differs. The Project page of Settings says when the
  graph needs more than the limit and which node needs it, and node stats flag it. See
  [Seeking and warmup](engine.md#seeking-and-warmup).

---

## Node graph editor

### Fixes

- [x] **bug** Duplicating a node lost the connections that feed it. Root cause found: `GraphEditor::fragment`
  (`editor/mod.rs`) keeps only connections whose *both* ends are in the selection, and drops linked nodes
  (Video, Audio) from the fragment entirely. Duplicate and paste share that path, so a lone node, or a node fed
  from outside the selection, comes back with no inputs. The existing test is not failing silently: it copies two
  nodes wired to each other and asserts that one wire, so it never covers external inputs. Fix: keeping the *input* connections (the copy's inputs wired to the same sources as the original, including
  the linked Video and Audio nodes and `@param` modulation wires; outputs are not duplicated, since an input takes
  one connection) is **both a setting and a keybind**:
  - Setting *Keep input connections when duplicating and pasting* (Settings page, application scope), which sets
    the default for Ctrl+D, the context menu and Ctrl+V.
  - Keybind that does the opposite for one action: Ctrl+Shift+V pastes with connections (or without, if the
    setting is on); Ctrl+Shift+D likewise for duplicate. Both are listed in the Edit menu and rebindable.
  - Internal wires (between the copied nodes) are always kept.
  Add tests for a lone node, a node fed by a linked input, a modulated parameter, a mixed selection, and each
  keybind and setting combination. Paste from another project can only keep connections to inputs that exist; the
  rest are skipped without an error.
  Done: the setting is a checkbox in the Edit menu until the Settings page exists (it then moves there), and
  Shift inverts it for one Ctrl+D or Ctrl+V. Not done: rebinding the keys, and tests for a node fed by a linked
  input and a modulated parameter (the test covers a lone node, the setting both ways, Shift and pasting into
  a project without the sources).
- [x] **bug** Typing a long number into a value box makes the inspector grow wider, repeatedly. This is a sustained
  problem and points to messy layout code, so do not patch it again. Root-cause it (a text edit sizing itself to
  its content and feeding the width back into the panel), then fix it once, permanently: one shared value-box
  widget with a fixed width, clipping, and a maximum number of characters, used by every numeric field in the
  inspector and the timeline; the inspector panel width is owned by the panel, not by its contents. Add a
  headless regression test that types a very long number and asserts the panel width does not change.
  Done: the cause was egui's `DragValue`, whose text field sizes itself to what is typed. All numeric fields now use
  `value_box::ValueBox` (fixed width, clipped, at most 12 characters, click to type, Escape cancels). The
  regression test types 40 digits and checks the box stays 58 px wide and the text is capped.

### Tooltips

- [ ] **feature** Hovering a connection shows its metadata as a tooltip: source node and port, destination, kind,
  channels and part (tag), nominal range, layout (width × height × samples per pixel), sample count per frame,
  and any note or warning on it.
- [ ] **feature** Hovering a **pin** shows the pin's current output value as a tooltip, **sampled at the frame
  rate** (one value per rendered frame, the same cadence as the modulation ghost handle). Define what the one value
  is (recommended: the mean of the frame's block for a signal, the exact value for a constant) and show min/max too.
  Parameter pins show the live modulated value.

### Performance

- [ ] **feature** Performance display on nodes, to find slow ones. The engine measures each node's processing time
  per frame (cheap timer around `process`, smoothed over recent frames, off the hot path) and the editor shows it
  as a badge on the node, and optionally a heat colour across the whole graph (View → Show performance). Report
  cost as time per frame and as a share of the graph's total, at the current preview resolution. The numbers are
  advisory and never part of the cache key. Feeds the benchmarks workflow in [benchmarks.md](benchmarks.md).

### Look and Listen tools

This **supersedes the earlier Probe/Inspect tool** (see [Decisions](decisions.md#looklisten-replaces-the-probeinspect-tool-october-2026)).

- [ ] **feature** **Look tool.** While its key is held, hovering a connection opens a tooltip-style popup showing the
  output of that connection at the playhead: a picture for video-like signals, a waveform for audio-like ones.
  Pictures use **implicit stretch to match the project's video size**, exactly as the Video Output node does with
  its stretch toggle (see [Nodes](#nodes)), so any signal is visible whatever its layout. One shared stretch
  implementation serves both. *Needs: a read-only engine tap, shared preview widgets.*
- [ ] **feature** **Listen tool.** A separate key; while held, hovering a connection plays that connection's output
  as audio. It reuses the Audio Output sink (resampling to the project audio rate, sanitizing, clipping) so what
  you hear is what an Audio Output node would play. Fades in and out so there are no clicks when the pointer
  moves between connections. The master playback is ducked while listening.
- [ ] **feature** **Tool bar and default tool.** A new toolbar in the graph for choosing the default tool: Select
  (today's behavior), Look, Listen. Holding the key for another tool temporarily overrides the default. Keys are
  rebindable once the Settings page exists.
- [ ] **feature** **Engine taps.** The engine can be asked for the output of any connection at one frame without
  changing the render or the cache: the request is read-only, rate-limited, and dropped when the playhead moves or
  the graph is edited. A tap never enters the cache key and never changes deterministic output. The design has to
  say what happens for a tap on a connection the graph dropped (not feeding the output): either compute it on
  demand or show "not rendered".
- [ ] **chore** Remove the Probe tool item and its distance-based master-fade mixer from the backlog; they are not
  being built.

---

## UI components and metering

Reusable widgets, built once in `rastersong-gui` (a `widgets/` module) and used everywhere they apply. See the
[DRY rule](#code-health-and-dry).

- [ ] **feature** **Level meter** (peak and RMS, with hold and clip indicator), used by Audio Output, Gain,
  Compressor, Limiter, the preview's volume control and the timeline's audio headers.
- [ ] **feature** **Spectrum analyzer** (FFT-based, log frequency axis, smoothing), for Equalizer, Filter, Three-Band
  Split, DC Filter and the Look/Listen popup. FFT through a permissively licensed crate (checked by `cargo-deny`),
  computed only for visible meters.
- [ ] **feature** **Gain-reduction meter**, so the Compressor, Gate and Limiter show how much reduction is being
  applied right now.
- [ ] **feature** **Waveform / scope** and a **picture thumbnail** widget (shared by the Look tool and node previews).
- [ ] **feature** **Node telemetry.** A way for a node to publish a few meter values per frame (gain reduction,
  peak, band energy) without allocation in `process` and without affecting output; the editor reads the last
  rendered frame, the same way it reads modulated values. Declared in the node's `SPEC` so the inspector draws the
  right widget with no per-node GUI code.
- [ ] **chore** Meters take values from telemetry or taps; no widget computes DSP of its own beyond display
  smoothing and the FFT.

---

## Timeline

- [x] Reaper-style track headers, grab-scroll, scroll-zoom, tick lines, tempo ruler, thumbnails and waveforms
- [x] **chore** Removed the Fit button (its tooltip was wrong and F already fits the view).
- [ ] **feature** **Automation clips:** signals drawn as tracks in the playlist to time effects to specific
  moments. The graph sees one more input signal, keeping the processing graph unified.
- [ ] **feature** Multi-clip timeline (see [Later](#later)).

---

## Nodes

Reference for what exists today is [nodes.md](nodes.md). Items here change that reference; each one needs tests
and a regenerated `nodes.md`.

### Inputs and outputs

- **Audio Output**
  - [ ] **feature** Volume control (gain, in dB, with a meter). Applies before sanitizing and clipping.
- **Video Output**
  - [ ] **feature** *Implicit stretch* toggle: stretch the incoming signal to the project's video size as if it
    were connected to the Video input node. **Defaults to on**, so the output always shows something whatever the
    signal's layout. Shares its implementation with the Look tool.

### Generators

- **Beat**
  - [x] **bug** Audio layout never works (see [Shared node settings](#shared-node-settings)).
  - [x] **feature** *Division* defaults to an integer (and is an integer parameter). Slower than the period is the bar period; old fractional divisions are raised to 1.
  - [x] **feature** *Steps* shows only when the mode is Step (conditional parameter).
  - [x] **feature** *Width* is conditional on the modes that use it.
- **Constant**
  - [x] **feature** *Value*'s modulation toggle is off by default (no pin exposed): nobody modulates a constant.
- **Noise**
  - [ ] **feature** Add Gaussian noise and a smooth noise (Perlin or similar) as new types. Smooth noise takes a
    scale in the usual units and is deterministic across seeks.
- **Oscillator**
  - [x] **chore** Remove the Ramp wave; it is a Saw followed by a Flip. Migration rewrites old Ramp oscillators to
    Saw + Flip (so *depends on the Flip rename/split below*).
  - [x] **bug** Audio layout error, same as Beat.
  - [x] **feature** *Pulse width* only shows for the pulse wave (conditional parameter).

### Structure

- **Pack**
  - [x] **feature** *Channels* defaults to an integer and is a real integer parameter. Review related structural
    nodes for the same.
- **Flip**
  - [x] **chore** The node's scope does not match its name. Either rename it to say what it does, or split
    **Transpose** into its own node so Flip only flips. Migration for old graphs (Flip with transpose becomes
    Transpose).

### Effects

- **Distortion**
  - [ ] **feature** Analogue-style types: tube, diode and tape, each with its own transfer curve and
    harmonic character. Needs oversampling or a note about aliasing at low signal rates.
- **Envelope / Slew**
  - [x] **chore** Investigated: they differ (Envelope follows magnitude, Slew limits the signal's own rate), so both stay with cross-referencing descriptions and shared attack/release code (`dsp::AttackRelease`). Original note: Envelope (detector + attack/release, `peak` or `rms`) and Slew (rise/fall rate
    limit) look like the same effect except for RMS. If they are, combine into one node (keeping RMS) and remove
    the other. If they differ (slew limits the signal's own rate; envelope follows its magnitude), document the
    difference in both descriptions and share the smoothing code. Either way it is one smoothing implementation.
- **Equalizer**
  - [ ] **feature** Make Equalizer a **single band**: any number of bands in series or parallel are equivalent, so
    users add several nodes instead. A band has a type (peak, low shelf, high shelf, low cut, high cut, notch,
    band pass), adjustable frequency, adjustable Q, and gain where it applies. The old three-band node is replaced by chained bands.
- **Dynamic Equalizer Band** (new)
  - [ ] **feature** Same controls as the single-band Equalizer, plus a dynamics response (threshold, ratio,
    attack, release, optional sidechain). Shows its gain change on a gain-reduction meter. Shares the band design
    code with Equalizer and the detector with the Compressor.
- **Frequency Modulation (FM)**
  - [ ] **chore** Rename the "Index" parameter to something that explains itself (candidate: *Depth*, with help
    text saying it is how far the modulator pushes the carrier's frequency).
- **Filter**
  - [x] **feature** Adjustable Q **and** a slope in dB/octave for sharper cuts (12, 24, 48, … dB/oct by cascading
    stages). Filter has it; sharing it with Three-Band Split and Equalizer is still to do.
  - [x] **feature** A non-resonant version: either a response option on Filter, or a "resonance off" mode that gives
    a maximally flat (Butterworth) response. Prefer one node with conditional parameters over two nodes.
- **Low Pass**
  - [x] **chore** Redundant in favor of Filter. Deprecate and remove once Filter has a non-resonant mode and a
    slope; migration maps an old Low Pass to a Filter set to low pass. Update the "low pass as blur" examples in
    [Concepts](concepts.md#advanced-effects) and `examples/graphs/`. *Depends on the Filter items above.*
- **DC Filter** (new)
  - [ ] **feature** Removes DC offset from a signal (a very low-frequency high pass, with its cutoff exposed and a
    sensible default). Documents when it is needed (after Offset, Distortion, Rectify).
- **Reverb**
  - [ ] **feature** The current reverb makes beautiful patterns but is nearly impossible to use subtly on video.
    Either rework it (a wet-level range that reaches subtle values, a lower-density option, shorter decay range,
    early-reflections-only mode) or add a second, simpler reverb node (a short comb/all-pass or Schroeder-style
    one) with few controls. Prototype both on the example graphs and pick by how usable subtle settings are.
- **Three-Band Split**
  - [ ] **feature** Slope in dB/octave on the crossovers (shared filter code; Linkwitz-Riley cascades so the bands
    still sum flat).

### Cross-cutting node items

- [x] **chore** Every node's unit parameter uses the one `Unit` (guard test `every_unit_parameter_offers_the_shared_units`) ([Units](#units-and-parameter-semantics)).
- [x] **chore** Every node with a dry/wet `mix` uses the one shared definition
  ([Units and parameter semantics](#units-and-parameter-semantics)).
- [ ] **feature** Add nodes only after the foundations are in, so new nodes use integer and conditional parameters
  from the start.
- [ ] **chore** Update [Concepts](concepts.md), [Node behavior](node-behavior.md) and the examples when a node they
  mention is renamed or removed.

---

## Code health and DRY

The aim: one implementation per idea, so fixes and features land in one place. Known candidates to consolidate:

- [x] **chore** **Units.** One `Unit` enum and one label set in `nodes/support.rs`, used by every node.
- [ ] **chore** **Filters.** Filter, Three-Band Split, Equalizer, DC Filter, the Chorus/Flanger/Phaser filtering and
  Low Pass share one biquad / cascaded-stage implementation in `dsp.rs`, with slope and Q designs in one place.
- [x] **chore** **Level detectors and smoothers** (`dsp::AttackRelease`; Slew is a linear rate limiter and keeps its own).** Envelope, Slew, Compressor, Gate, Limiter and the sidechain
  share one detector (peak/RMS, attack/release) and one smoother in `dsp.rs`.
- [ ] **chore** **Layout and generators.** One shared generator layout setting (see above).
- [x] **chore** **Dry/wet.** One shared `mix` definition and helper, not a copy per node.
- [ ] **chore** **Inspector widgets.** One slider+value-box, one integer field, one choice control, one tooltip
  helper, one meter, one number formatter. No widget re-implements these. The inspector, the timeline and the
  editor use the same ones.
- [ ] **chore** **Text.** All strings from lang files (see [Text and localization](#text-and-localization)); no
  string literals in widgets.
- [ ] **chore** **Stretch.** One "stretch a signal to the project size" implementation, shared by the Stretch node,
  Video Output and the Look tool.
- [ ] **chore** **Audio sinks.** One resample-sanitize-clip path, shared by Audio Output and the Listen tool.
- [ ] **chore** Add guard tests where practical (the `mix` one is done): a registry test that fails if a node declares its own `unit` list
  or its own `mix`, so duplicates cannot creep back.
- [ ] **chore** Codebase organization and code-quality checkup (not started), run after the foundations land and
  scoped by this list.

---

## Parameters (done)

- [x] **Modulation inputs on parameters** (major feature). The parameter value is the baseline and the connected
  signal modulates it on top, as in Serum 2: one-direction and bidirectional modes, with the connected signal's
  colour. (Being reworked as percentages; see above.)
- [x] Soft-bounded fields like Substance Designer: entering a value outside the bounds widens the slider's range.

## Preview and graph (done)

- [x] 1/8 and 1/16 preview resolutions
- [x] Load button in the preview when no video is loaded
- [x] Audio offset leaves the preview pane (it belongs to the audio track)
- [x] Link colors come from port metadata (red/green/blue from Split Channels, outline and gradient options)
- [x] Audio output is part of the graph (the Audio Output node); audio and video share one workflow
- [x] Colors come from a theme file, with no magic numbers
- ~~Links snap to nearby pins~~: dropped, the grab radius is fine
- ~~Better graph background~~: dropped, the current background is fine
- [x] Effects on RGB signals scramble channels when modulated: effects now have the `channels` setting
- [x] Undo/redo in the editor; asking before closing with unsaved changes

## Bugs fixed

- [x] Playback slows near the end of the video while waiting for more buffer
- [x] Moving a node sets the dirty flag and invalidates the cache (position is editor-only metadata)
- [x] Preview pane layout is wrong until a video is loaded
- [x] Preview pane controls jump to the top while the loading spinner shows
- [x] The node search popup shrinks in height over a long session
- [x] Rarely, dragging a slider resizes it instead of changing the value

---

## Later

- Modulator settings beyond direction and amount: custom response curves, and minimum and maximum values. (Revisit
  once percentage-based amounts are in; a min/max clamp may be redundant with them.)
- Multi-clip timeline
- GUI export (H.264, ProRes, FFV1) and state snapshots for faster seeking with stateful graphs
- A tempo map (tempo changes over time)
- More nodes and interpolation modes
- Fuzzing

## Decisions waiting on the GUI

To settle by trying them in the app. Details in [Decisions](decisions.md#open).

- What `interleave` and `pack` should mean
- Value ranges (video `0..1`, audio `-1..1`) and mono-only modulators
- "Audio to video" / "Video to audio" names
