# Decisions

Decisions made, with the reasons, and the ones still open. The work that follows from them is in the
[roadmap](roadmap.md).

## Made

### Nodes: separate files, not one mega-file

One node per file under `nodes/<category>/`, with its spec, parameters, implementation and tests together. The
registry stays in `nodes/mod.rs`. Each node also declares its own `SPEC`, which the registry lists. See
[Node authoring](node-authoring.md).

### Own node graph

There is no library; the canvas (`editor/canvas.rs`) is already custom, so the issues are our own code. Keep it
custom. Snapping, link colors, parameter modulation pins, tooltips and the Look/Listen tools all need that control.

### Categories stay as they are

"Channels" and "Conversion" stay. Channels changes a signal's shape and leaves its values alone; Conversion changes
the value range. (Generators, Effects and the rest are unchanged.) See [Node behavior](node-behavior.md#categories).

### Channels setting hover text

The setting name stays. The text is in [Node behavior](node-behavior.md#shared-node-settings).

### Signal unification (October 2026, done)

No node may require one domain: video and audio are the same `Signal`, and nothing converts implicitly. Type
information is an advisory `Tag` on each `Layout` that colours wires and produces notes, never errors; conversions
are explicit nodes; Relabel rewrites a tag on purpose. Audio inputs arrive natively interleaved (no downmix),
Split/Combine Channels take any channel count, Stretch to Match and the per-node grouping setting make rate matching
explicit, and the optional Audio Output renders the graph's sound (resampled to the project's `audio_rate`,
sanitized, cached with each frame, muxed by the CLI). Both outputs line up at the later one's latency inside the
graph. See [Signals & units](signals-and-units.md).

Deferred on purpose:

- NaN and infinity are scrubbed only at the sinks, so a NaN inside a recursive node (feedback, IIR) stays in its
  state until a reset, and a seek's warmup can bring it back.
- Variable frame rate: the compiler assumes a constant frame rate; blocks keep their real timestamps, so VFR video
  works but its sound is resampled as if the rate were constant.
- Export from the app: the engine and the CLI's `.mkv` mux handle the rendered sound, but there is no GUI export
  yet. Source audio without an Audio Output is, for the CLI, its one audio file; a multi-track project needs a
  decision on mixing or multiple streams.

### Resampling is only applied when needed

`graph.rs` creates a resample buffer only for secondary inputs that are unconnected or differ in length from the
main input. Both gaps found are closed: `Modulator::fill_block` has a copy fast path at a ratio of exactly 1, and
the decode-time `Resampler` is skipped when the file's rate already matches.

### Look/Listen replaces the Probe/Inspect tool (October 2026)

The earlier plan was a *probe tool*: a magnifying glass for audio that fades the master down and fades nearby links
up, by distance from the cursor. It is **superseded** by the Look and Listen tools in the
[roadmap](roadmap.md#look-and-listen-tools): hold a key and hover a connection to see (Look) or hear (Listen) that
connection's output in a tooltip-style popup, with a toolbar to pick the default tool. The distance-based master
fade mixer is dropped; Listen plays one tapped connection.

### Modulation amounts become percentages of the parameter's span (October 2026)

Hands-on testing showed that dialling in modulation with *minimum, maximum, base value and amount* all in the
parameter's own units is unruly. The amount becomes a percentage of the parameter's min..max span, so the same
number means the same swing on any parameter. The roadmap holds the details and the migration of existing graphs.

### Logarithmic and exponential sliders are removed (October 2026)

They confuse users. Sliders are linear. Parameters that are naturally multiplicative (frequencies) get that
behavior from their units and from modulating in octaves, not from a warped slider; the roadmap's modulation work
decides how octave modulation is presented.

### Integer parameters are a real type (October 2026)

A parameter that can only be a whole number (channel counts, divisions, steps, voices) is declared as an integer in
its spec, and the slider and value box always snap to whole values. A float is never allowed for these.

## Open

### Open: "pixel" and "sample" units vs the no-samples principle

[Design principle 3](concepts.md#design-principles) says users never see samples, so results look the same at any
preview resolution. The notes ask for explicit "pixel" and "sample" units. They are compatible only if these units
are defined against the **project** (full) resolution and scaled by the preview scale, so a half-resolution preview
still looks like a scaled-down render. The alternative, raw units at preview resolution, would break that guarantee.
Recommendation: define both against full resolution and scale in the engine; update principle 3 to say users may
choose to think in pixels or samples, but never in preview-scaled ones.

### Open: what replaces the "mix" parameter

Many effects have a dry/wet `mix`. The notes call it redundant "with blend modes being available". Today blend
exists as the Blend node. Options: (a) remove `mix` from effects and rely on a Blend node (more wiring, but one
implementation); (b) add a shared per-node *output blend* setting (mode + amount) alongside Resampling and Channels,
so every effect gets blend modes for free with no extra node. Recommendation: (b), because it removes the per-node
`mix` code and doesn't add wiring. Nodes where mix is intrinsic to the algorithm (a reverb's wet level, a delay's
feedback) keep their own controls.

### Open: multi-track export

How a multi-track project mixes for export: one mixed stream, or several streams. Needed before GUI export.

### Open: "Audio to video" / "Video to audio" names

They are confusing. The names should describe the effect, which is the range change, with the ranges stated in the
node description. Candidates: "Brightness to Wave" / "Wave to Brightness". Not decided.

### Open: Interleave and Pack semantics

What `interleave` and `pack` should mean (currently a relabel between RGB and a 3×-wide mono carrier). The notes
also ask for Pack's channel count to be a true integer.

### Open: value ranges and mono-only modulators

Value ranges (video `0..1`, audio `-1..1`) and mono-only modulators are still to be settled by using the app.

## Open Questions

- **License review:** the custom LICENSE is a first draft. Have it reviewed, or switch to an established source-available license (e.g. PolyForm Strict), before any paid release.
- **Snapshots:** memory budget and spacing for state snapshots.
- **Hardware encoder fallback:** what to offer for H.264 export on machines with no usable OS/hardware encoder.
