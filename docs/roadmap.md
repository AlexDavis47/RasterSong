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
- [ ] [Timeline, resources and graph layers](#timeline-resources-and-graph-layers): multi-track timeline of items,
  the Resources panel with linking and embedding, several graphs on stacked layers, subgraphs, raw files, images,
  automation clips, output buses and GUI export
- [ ] The foundation workstream below (parameter types, units, modulation, text), since most node work depends on it
- [ ] The settings page (connection inspection is done; its keys are not rebindable yet)

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
6. **[Timeline, resources and graph layers](#timeline-resources-and-graph-layers)**, in its own stage order. It
   replaces the project model, the source nodes and the renderer's single graph, so finish other work that touches
   those (Video and Audio Input, the timeline, project loading) before starting, or fold it into the stages.

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

- [x] **chore** Move all user-facing text into **lang files** (done: see [Text and languages](text.md)) so locale and wording can be changed without touching
  code. Covers: node labels, descriptions, parameter labels and help text, port help, choice labels, menu and
  button text, tooltips, notes and warnings, dialogs and errors. Proposed shape: a string table keyed by stable ids
  (`node.delay.label`, `node.delay.param.time.help`, `ui.timeline.loop`), `en` embedded at build time as the
  source of truth, other locales loadable at runtime, missing keys fall back to `en`. A registry test fails when a
  node, parameter or port has no `en` entry; `cargo xtask docs` reads the same table so [nodes.md](nodes.md) stays
  in step. Evaluate Fluent against a simple key/value format first (plurals and units need some formatting;
  keep it as small as that allows). Do this early: every new string in this roadmap should go in a lang file from
  the start.
- [x] **chore** Settings page picks the language (see [Project and settings](#project-and-settings)).

---

## Project and settings

- [ ] **feature** **Settings page.** A real Settings window (File → Settings, and a toolbar button) so hidden
  settings can be exposed. *Partly done:* the window (File → Settings…, Ctrl+,), its Application and Project pages
  and the settings that already existed (theme, wire style, node stats, keep connections, tempo, audio rate) are in;
  cache budget, render-ahead and the default preview resolution are in too (with a reset button); what is left is
  the default tool, the toolbar button and reset buttons on the older settings. Two scopes, clearly separated:
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
- [ ] **feature** **Keybinds.** Every keyboard action can be rebound by the user on a *Keys* page of Settings
  (Application scope: remembered between sessions, never in project files), with a reset per binding and for all.
  *Design:* one table of actions (`Action::Undo`, `Action::SplitItems`, `Action::Copy`, …), each with an id for
  the settings file, a label and help from the language files, a scope (*global*, *timeline*, *graph*) and default
  chords. Code asks the table (`keys.pressed(ui, Action::SplitItems)`) instead of testing keys, and menus and
  tooltips show the bound chord from it, so a binding changes everywhere at once. Today the keys are tested in
  `app.rs` (`shortcuts`), `timeline.rs` (`item_keys`), `editor/canvas.rs` and the menus' shortcut texts, which
  all move over. The page lists actions by scope with a "press a key" capture and warns about a chord bound twice
  in the same scope (timeline and graph may share one, since the pointer decides which gets it). Copy, cut and
  paste arrive from the platform as clipboard events rather than keys (only when the clipboard holds text, for
  paste), so rebinding them means handling those chords as plain keys and reading the system clipboard ourselves.
  Mouse modifiers (Ctrl to add to a selection, Shift to drag freely, Alt to stretch) stay fixed for 1.0.

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

- [x] **feature** Hovering a connection shows its metadata as a tooltip: source node and port, destination, kind, *(Done except a note or warning on the wire itself: notes show on the node's badge.)*
  channels and part (tag), nominal range, layout (width × height × samples per pixel), sample count per frame,
  and any note or warning on it.
- [x] **feature** Hovering a **pin** shows the pin's current output value as a tooltip, **sampled at the frame
  rate** (one value per rendered frame, the same cadence as the modulation ghost handle). Define what the one value
  is (recommended: the mean of the frame's block for a signal, the exact value for a constant) and show min/max too.
  Parameter pins show the live modulated value.

### Performance

- [x] **feature** Performance display on nodes, to find slow ones. The engine measures each node's processing time
  per frame (cheap timer around `process`, smoothed over recent frames, off the hot path) and the editor shows it
  as a badge on the node, and optionally a heat colour across the whole graph (View → Show performance). Report
  cost as time per frame and as a share of the graph's total, at the current preview resolution. The numbers are
  advisory and never part of the cache key. Feeds the benchmarks workflow in [benchmarks.md](benchmarks.md).

### Inspecting connections

This **supersedes the earlier Probe/Inspect tool** (see [Decisions](decisions.md#looklisten-replaces-the-probeinspect-tool-october-2026)),
and the separate Look and Listen tools tried first: hands-on testing showed a mode switch got in the way when the
meters already show on hover.

- [x] **feature** **Inspection on hover.** Hovering a connection shows its meter, readings and a view of the signal at
  the playhead; hold **Alt** and scroll to go through the views (picture, scope, spectrum, readings only; remembered for audio and for other signals),
  any signal in any view. The view is a setting; the update rate while the playhead moves is a project setting.
- [x] **feature** **Listen key.** Hold **Shift** over a connection to hear it through the Audio Output sink path, with the
  playback ducked and fades in and out.
- [ ] **feature** Keys are fixed (Alt for the views, Shift to listen) until the Settings page can rebind them.
- [x] **feature** **Engine taps.** *(done: see [Engine](engine.md#taps-and-listening); a dropped connection answers "not rendered")* The engine can be asked for the output of any connection at one frame without
  changing the render or the cache: the request is read-only, rate-limited, and dropped when the playhead moves or
  the graph is edited. A tap never enters the cache key and never changes deterministic output.
- [ ] **feature** More views: histogram, vectorscope, and a view of a signal's mean level over time.
- [x] **chore** Remove the Probe tool item and its distance-based master-fade mixer from the backlog; they are not
  being built.

---

## UI components and metering

Reusable widgets, built once in `rastersong-gui` (a `widgets/` module) and used everywhere they apply. See the
[DRY rule](#code-health-and-dry).

- [x] **feature** **Level meter** *(done for Audio Output and Gain; the preview volume control and timeline headers still to do)* (peak and RMS, with hold and clip indicator), used by Audio Output, Gain,
  Compressor, Limiter, the preview's volume control and the timeline's audio headers.
- [x] **feature** **Spectrum analyzer** *(done as a shared widget, `dsp::Fft` underneath, in the inspection popup; the Equalizer, Filter, Three-Band Split and DC Filter inspectors don't show one yet)* (FFT-based, log frequency axis, smoothing), for Equalizer, Filter, Three-Band
  Split, DC Filter and the Look/Listen popup. FFT through a permissively licensed crate (checked by `cargo-deny`),
  computed only for visible meters.
- [x] **feature** **Gain-reduction meter**, so the Compressor, Gate and Limiter show how much reduction is being
  applied right now.
- [x] **feature** **Waveform / scope** and a **picture thumbnail** widget *(shared in `widgets/`; node previews still to do)* (shared by the Look tool and node previews).
- [x] **feature** **Node telemetry.** A way for a node to publish a few meter values per frame (gain reduction,
  peak, band energy) without allocation in `process` and without affecting output; the editor reads the last
  rendered frame, the same way it reads modulated values. Declared in the node's `SPEC` so the inspector draws the
  right widget with no per-node GUI code.
- [ ] **chore** Meters take values from telemetry or taps; no widget computes DSP of its own beyond display
  smoothing and the FFT.

---

## Timeline

- [x] Reaper-style track headers, grab-scroll, scroll-zoom, tick lines, tempo ruler, thumbnails and waveforms
- [x] **chore** Removed the Fit button (its tooltip was wrong and F already fits the view).
- Automation clips and the multi-clip timeline are now part of
  [Timeline, resources and graph layers](#timeline-resources-and-graph-layers).

---

## Timeline, resources and graph layers

The 1.0 project model, decided in October 2026 (reasons in
[Decisions](decisions.md#timeline-resources-and-graph-layers-october-2026)). It replaces the one-video timeline,
the single graph and the plain JSON project file. Build it in the stages below, in order: each stage is a
foundation for the next, so nothing is built on a model about to be replaced. Format changes need no migration
(version 0).

**Rule: explicit over hidden.** Every signal a graph uses enters through a port that something visibly filled.
There are no fallbacks and no overrides that only apply in some situations (the old "Audio Output replaces the
source audio unless…" rules are what this avoids). Implicit behaviour needs a clear reason, such as Video Output's
stretch, and is documented where it happens. Something that reads nothing reads zeros and shows a note.

### The model

**Project timebase.** The project has its own settings, like a Premiere sequence: resolution, frame rate, audio
rate, length (to the end of the last item). They are taken from the first video dropped in and can be changed in
Project settings; items keep their times when they change. A block stays one project frame, and every track is
conformed to the project grid on its way into a graph. Positions are saved as time, not frames. Graph items snap
to whole frames; audio items are placed to the sample. Today `RenderInfo` and `pixel_scale` come from the source
video; both move to the project settings.

**Resources.** The Resources panel holds everything a project uses:

| Kind | Native rate | Notes |
|---|---|---|
| Video | its frame rate | one video stream |
| Audio | its sample rate | one audio stream, any channel count |
| Image | none | an item can be any length; every frame is the same picture |
| Raw file | set by the user | any file read byte by byte (text, executables, ISOs, media files opened raw) |
| Automation curve | its layout (audio rate by default) | a signal drawn by hand |
| Graph | none | see *Graphs and ports* |
| Glued item | its source's | an edit list over another resource |

- *Importing* a file with several streams shows "Found multiple tracks in this media" with a checkbox per stream;
  each chosen stream becomes its own resource. Embedding stores the file once.
- *Linked* by default (path relative to the project file when inside its folder). **Embed** copies the file into
  the project file; **Embed all used resources** is a separate command, so normal saves stay fast.
- **Unembed** reads the original path again. If the original is gone, a popup offers to **relocate** it or to
  **save the embedded copy** somewhere first; the data is never dropped without one of these.
- A linked resource missing on open shows an error on the resource and its items, with **Relocate**; its items
  read zeros until found.
- Embedded resources remember the original path, size, modified time and content hash. When the original changes
  they are marked **out of date** with **Re-embed**; a changed linked resource is reloaded. Checked on open and when
  the app regains focus (size and time, confirmed by hash). A changed resource invalidates the cached frames that
  read it.
- *User resources* live in a library in AppData, mostly graphs used as tools. Projects link to them by default
  (editing the library changes every project using it) and can embed them to freeze them. This replaces the
  separate subgraph library planned earlier.
- *Raw files* have interpretation settings: sample format (u8 by default, i8, i16, i32, f32, with endianness),
  channels, rate and a byte offset to skip headers. Unsigned formats map to `0..1`, signed to `-1..1` (advisory
  tags). Presets: *As audio* (project audio rate) and *As video* (each frame takes width × height × channels
  bytes, laid out as a picture). Any file can be opened raw, media files included. Read through a memory map,
  never converted in memory.
- *Audio* is decoded once on import into an uncompressed `f32` file in the AppData cache (named by content hash,
  rebuilt when missing, clearable from Settings) and read through a memory map, the same path as raw files; an
  uncompressed WAV is mapped directly. Video keeps streaming through its decoder.
- **Glue** (as in Reaper) saves selected items of one track as a new resource: an edit list pointing at the source
  with each piece's in/out points and rate. Nothing is re-encoded or copied, for media and automation alike.
  Editing the source changes glued copies; **Make unique** bakes one into an independent resource.

**Tracks and items.**

- A track holds items from **one resource** only, which can be cut up and rearranged freely. This keeps each
  source's layout fixed along the timeline, which graph compilation needs.
- **+ Track** adds an empty track to drag a resource onto; dragging a resource onto empty timeline space creates a
  track named after it.
- An item has a position, in/out points and a **rate modifier**: video holds or skips frames, audio is resampled so
  its pitch shifts like tape (no pitch-preserving stretch in 1.0). Gaps read as zeros. Audio item edges get a short
  fixed fade against clicks.
- Every item has a **header bar** along its top for dragging and the right-click menu, with a **mute** button (a
  muted item reads as a gap). The area below belongs to the item's content. Items have no solo.
- Tracks can be **linked** by the user so their items move and split together; nothing links automatically.
- Track headers keep name, mute and volume, plus solo; track height is adjustable. The offset becomes the item's
  position.
- A new image or automation item made from the Resources panel is 5 seconds long (a Project setting).
- Editing: move, trim, split at the playhead, delete, Alt+drag an edge to change the rate, snapping (grid, beats,
  item edges, playhead) on by default with a modifier to drag freely, multi-select, copy/paste, glue, undo.

**Automation curves.** The **automation tool** drags out a new item on an automation track and adds its curve to
the Resources panel; a curve made in the panel is dragged in at the default length. Points are edited on the item:
Ctrl+click on the line adds one, dragging moves it, right-click sets its interpolation and its exact time and
value. Points snap like items. As a signal a curve is a generator like Oscillator or Beat, sampled at the shared
generator layout, audio rate by default.

**Track mix and output buses.** With no graph item, the timeline plays the *track mix*: the top video (or image or
raw-as-video) track with an item at that frame, stretched to the project size as Video Output stretches (no
per-track opacity or blend modes); and every unmuted audio track summed with its volume into its bus. The master
audio is a set of **buses** in the project settings, each with a name and a channel count (default *Main*, stereo;
5.1 is *Main* with six channels; stems are extra buses). Tracks route to a bus (*Main* by default); each Audio
Output picks one. Adding buses or channels breaks nothing; removing them warns first, listing the connections and
tracks affected.

**Graphs and ports.** Graphs are resources; a project can have any number.

- A graph has any number of named **Input** and **Output** nodes, each a port. Video Output and Audio Output (one
  per bus) are the master outputs; other outputs are plain ports, for example an oscillator exposed as a control
  signal. Input ports replace today's Video Input and Audio Input, which pick a source by name.
- **The parent fills the ports, with no fallback.** As a subgraph, the parent graph wires the pins; an unwired pin
  reads zeros, with a note. On a graph layer the item is the parent: its inspector lists the inputs, each bound to
  *Layer below* (video, or a bus), a track, or nothing. Bindings belong to the item, so one graph can be placed
  twice reading different tracks; copying an item keeps them.
- **New graphs start as a passthrough:** `In: Video` wired to Video Output and `In: Audio` to Audio Output, bound to
  the layer below when dropped on a layer. Effects are inserted into the wires, and a graph that only changes the
  picture leaves the sound alone because its audio wire is still there. A master output with nothing connected, or
  missing, gives black or silence, with a note on the node (or on the item when the node is missing).
- **Timeline context generators** read where the render is, not what is on the timeline: **Graph Progress** (new;
  0 to 1 across the item rendering the top-level graph, also inside subgraphs; 0 during pre-roll), and Beat and
  tempo units as today.
- **Subgraphs:** dragging a graph resource into a graph adds it as a node whose ports are pins; double-clicking
  opens it, editing the resource everywhere it is used. A subgraph is a linked reference (**Make unique** copies
  it), each use has its own state, a graph can't contain itself (the compiler rejects the cycle), and subgraphs are
  flattened at compile so latency, warmup, channel settings and modulation work unchanged. Exposed parameters are
  [later](#later).

**Graph layers.** Lanes above the tracks holding graph items.

- Items on a layer can't overlap (dropping one onto another trims it) and have no crossfades. They can be moved, cut
  and trimmed, not stretched.
- **Stacking:** the bottom layer's *Layer below* is the track mix; each layer above reads the one below; the top
  layer's outputs are the master. A layer with no item at a frame is transparent there. Only master outputs pass
  between layers; control signals are shared through subgraphs.
- Layer headers have mute and solo; graph items have mute in their header bar.
- **Pre-roll** is a per-item setting, on by default: the graph warms up as if it had been running before the item,
  so trimming the left edge never changes what follows. Off, it starts cold at the edge.

**Editor and preview.** Double-clicking a graph resource or graph item opens it in the editor. The preview always
shows the master output. Hovering a connection in the open graph inspects it even when the graph isn't under the
playhead (the tap renderer renders it on its own).

**Engine.** Track readers read items (position, in/out, rate) conformed to the project grid. The renderer keeps a
compiled graph per graph item and switches at item edges; latency is already compensated against the timeline
(output frame N comes from source frame N + latency), so graphs with different latencies stay in sync. Each layer
has its own renderer state, so layers can later run as a pipeline across frames on separate cores. The cache key
becomes what produced the frame: the active graphs' versions, the versions of the resources and tracks they read,
and the preview scale, so editing one graph keeps frames rendered only by others. Each video track needs a decoder
on the render thread, the tap renderer and the thumbnails; benchmark this early.

**Project file.** `.rastersong` becomes a zip: `project.json` (settings, resources with links, embed metadata and
raw settings, tracks, items, layers, project graphs, tempo, loop region) and `resources/` with embedded files
stored uncompressed. Embedded files are extracted on open to the AppData cache by content hash, so FFmpeg and
memory maps read ordinary files. Saving rewrites the zip, copying unchanged entries raw.

### Stages

1. **Timebase and tracks**
   - [x] **chore** The model underneath *(done: see [Timeline](engine.md#timeline))*: the project has a timebase
     (taken from the first video track until set; length to the end of the last item), separate video and audio
     track lists ([Decisions](decisions.md#video-and-audio-tracks-are-separate-lists-october-2026)), tracks hold
     items (position, in/out, rate, mute), and every track enters the graph by its name. The app still shows one
     item per track.
   - [x] **feature** Project settings (resolution, frame rate) on the Project page of Settings: a Picture section
     shows the timebase in use, and editing it sets the project's own (the reset button follows the video again).
   - [x] **feature** Tracks of items for video and audio, item header bars with mute, track solo and height, linking
     *(done: see [Timeline](app.md#timeline) and
     [Decisions](decisions.md#tracks-of-items-october-2026); the app still opens one video track until the
     Resources panel)*.
   - [x] **feature** Item editing (move, trim, split, delete, rate drag, snapping, multi-select, copy/paste, undo)
     *(done: see [Timeline](app.md#timeline) and [Decisions](decisions.md#item-editing-october-2026))*.
   - [x] **feature** Track mix and output buses, with the warning on removal *(done: see
     [Track mix and output buses](engine.md#track-mix-and-output-buses); with no graph items yet, bypassing the
     graph plays the track mix, and the CLI's `--bus` renders a bus other than the master)*.
   - [x] **feature** Track readers in the engine: video conformed to the project's grid and size, audio placed item
     by item, gaps read zeros.
   - [x] **feature** Audio decoded to cache files and memory-mapped (`Modulator` and the waveform read the mapped
     samples instead of owning copies). *(Done: see [Media](media.md); uncompressed WAVs are still decoded to the
     cache rather than mapped directly, and clearing the cache from Settings waits for the Settings page.)*
   - [x] **feature** Short fixed fades on audio item edges *(5 ms, crossfading where items overlap; see
     [Timeline](engine.md#timeline))*.
   - [x] **chore** New project file contents (still plain JSON at this stage); CLI `render <project> <out>` *(the
     old `render <video> <audio> <graph> <out>` stays; without an Audio Output the CLI writes the mix of the audio
     tracks)*.
2. **Resources and graphs**
   - [x] **feature** Resources panel (linked resources only) and the multi-stream import dialog *(done: see
     [Resources](app.md#resources) and [Decisions](decisions.md#resources-panel-october-2026))*.
   - [x] **feature** **+ Track**, dropping a resource onto an existing track, and Relocate for missing files *(done:
     see [Timeline](app.md#timeline) and [Decisions](decisions.md#empty-tracks-relocate-and-graph-resources-october-2026))*.
   - [x] **feature** Graphs as resources, with the passthrough template *(done: see [Resources](app.md#resources)
     and [Decisions](decisions.md#empty-tracks-relocate-and-graph-resources-october-2026); one graph is open and
     rendered at a time until graph layers)*.
   - [x] **bug** Resources pane polish: Relocate always clickable, rename field no longer closes its menu, media
     and graphs as one shared card in a grid with Media and Graphs tabs, clearer track outlines *(done: see
     [Resources](app.md#resources))*.
   - [x] **chore** `CONTRIBUTING.md` development rules, and one shared channel-count label *(done)*.
   - [ ] **feature** Input and Output port nodes replacing Video Input and Audio Input *(deliberately after graph
     layers: a port is filled by a layer item's bindings or a subgraph's pins, so before those it would only be the
     current nodes under another name)*.
   - [ ] **feature** Graph layers: items, bindings in the item inspector, stacking, mute/solo, pre-roll setting.
   - [ ] **feature** Renderer switching graphs at item edges, one renderer state per layer, the new cache key.
   - [ ] **feature** Graph Progress node.
   - [ ] **feature** Inspecting connections in a graph that isn't under the playhead.
3. **Subgraphs**
   - [ ] **feature** Subgraph nodes with pins, linked references, Make unique, cycle check, flattening at compile.
4. **Embedding**
   - [ ] **feature** Zip project file, embed / embed all / unembed (with the relocate-or-save popup), change detection
     and Re-embed, relocating missing files, the user library in AppData.
5. **More resource kinds**
   - [ ] **feature** Raw files (interpretation settings, presets, memory-mapped).
   - [ ] **feature** Images.
   - [ ] **feature** Automation curves: the automation tool, point editing, generator layout.
   - [ ] **feature** Glue and Make unique for edit lists.
6. **Export**
   - [ ] **feature** GUI export of the master output, with buses written as streams in one file or as separate files
     (separate by default when there is more than one bus).

### Workflows to check at the end

These are the acceptance tests for the model, end to end in the app:

1. **No graph:** import media, drag it to the timeline, play the track mix like a normal editor.
2. **One effect throughout:** a new graph (passthrough) with AM inserted on the video wire and an input `Kick` on
   its modulation, placed on layer 1 over the whole video with `Kick` bound to the kick track. The sound is
   untouched.
3. **Effects per section:** graphs A and B side by side on layer 1, trimmed into hard cuts.
4. **A persistent audio chain:** graph C (a compressor on its audio wire) across layer 1, A and B on layer 2
   reading C's audio through *Layer below*.
5. **Composing graphs:** graph M with inputs `Video`, `Kick`, `Bass` and A and B as subgraphs, `Video` wired into
   both and into two Blends driven by `Kick` and `Bass`; M's item binds the inputs.
6. **Building up over a section:** a graph crossfading dry to wet with Graph Progress; stretching the item sets
   how long it takes.
7. **Reusing part of a curve:** automation items cut and arranged, glued into a new curve resource, dragged in
   elsewhere.

---

## Nodes

Reference for what exists today is [nodes.md](nodes.md). Items here change that reference; each one needs tests
and a regenerated `nodes.md`.

### Inputs and outputs

- **Audio Output**
  - [x] **feature** Volume control (gain, in dB, with a meter). Applies before sanitizing and clipping.
- **Video Output**
  - [x] **feature** *Implicit stretch* toggle: stretch the incoming signal to the project's video size as if it
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
  - [x] **feature** Make Equalizer a **single band**: any number of bands in series or parallel are equivalent, so
    users add several nodes instead. A band has a type (peak, low shelf, high shelf, low cut, high cut, notch,
    band pass), adjustable frequency, adjustable Q, and gain where it applies. The old three-band node is replaced by chained bands.
- **Dynamic Equalizer Band** (new)
  - [x] **feature** Same controls as the single-band Equalizer, plus a dynamics response (threshold, ratio,
    attack, release, optional sidechain). Shows its gain change on a gain-reduction meter. Shares the band design
    code with Equalizer and the detector with the Compressor.
- **Frequency Modulation (FM)**
  - [x] **chore** Rename the "Index" parameter to something that explains itself (candidate: *Depth*, with help
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
  - [x] **feature** Removes DC offset from a signal (a very low-frequency high pass, with its cutoff exposed and a
    sensible default). Documents when it is needed (after Offset, Distortion, Rectify).
- **Reverb**
  - [ ] **feature** The current reverb makes beautiful patterns but is nearly impossible to use subtly on video.
    Either rework it (a wet-level range that reaches subtle values, a lower-density option, shorter decay range,
    early-reflections-only mode) or add a second, simpler reverb node (a short comb/all-pass or Schroeder-style
    one) with few controls. Prototype both on the example graphs and pick by how usable subtle settings are.
- **Three-Band Split**
  - [x] **feature** Slope in dB/octave on the crossovers (shared filter code; Linkwitz-Riley cascades so the bands
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
- [x] **chore** **Filters.** Filter, Three-Band Split, Equalizer, DC Filter, the Chorus/Flanger/Phaser filtering and
  Low Pass share one biquad / cascaded-stage implementation in `dsp.rs`, with slope and Q designs in one place.
- [x] **chore** **Level detectors and smoothers** (`dsp::AttackRelease`; Slew is a linear rate limiter and keeps its own).** Envelope, Slew, Compressor, Gate, Limiter and the sidechain
  share one detector (peak/RMS, attack/release) and one smoother in `dsp.rs`.
- [ ] **chore** **Layout and generators.** One shared generator layout setting (see above).
- [x] **chore** **Dry/wet.** One shared `mix` definition and helper, not a copy per node.
- [ ] **chore** **Inspector widgets.** One slider+value-box, one integer field, one choice control, one tooltip
  helper, one meter, one number formatter. No widget re-implements these. The inspector, the timeline and the
  editor use the same ones.
- [x] **chore** **Text.** All strings from lang files (see [Text and localization](#text-and-localization)); no
  string literals in widgets.
- [ ] **chore** **Stretch.** One "stretch a signal to the project size" implementation, shared by the Stretch node,
  Video Output and the Look tool.
- [x] **chore** **Audio sinks.** One resample-sanitize-clip path, shared by Audio Output and the Listen tool.
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
- State snapshots for faster seeking with stateful graphs
- Timeline: item fades, ripple editing, slip, pitch-preserving stretch, crossfades between graph items
- Exposed parameters on subgraphs (knobs on the subgraph node bound to inner parameters)
- Caching each graph layer's output separately, if measurements show it is worth the memory
- Running graph layers as a pipeline across frames on separate cores
- A tempo map (tempo changes over time)
- More nodes and interpolation modes
- Fuzzing

## Decisions waiting on the GUI

To settle by trying them in the app. Details in [Decisions](decisions.md#open).

- What `interleave` and `pack` should mean
- Value ranges (video `0..1`, audio `-1..1`) and mono-only modulators
- "Audio to video" / "Video to audio" names
