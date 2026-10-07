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

Decided with it: *both ways*, the amount is the peak-to-peak swing, so 100% covers the span in either mode (an
older "±2" becomes the percentage that gives the same ±2). *Octave* scaling was dropped (see below): every parameter, frequencies included, takes a percentage of its linear
span. A new connection starts at 25%, one way, whatever the parameter and its
base value.

Also decided: modulation keeps the value between the slider's ends by default (widened to include a base value
typed beyond them), with a per-modulator toggle to allow overshoot up to the parameter's limits. Graphs from before
the toggle load with it on, so nothing changes for them. The modulator menu takes the amount as a percentage or as
a distance in the parameter's unit, kept in step.

### Modulation is one rule (October 2026)

Hands-on use showed too many special cases (peak to peak both ways but full swing one way, an overshoot toggle,
a knob that could leave its range). Now: the percentage is the only stored amount and is a percentage of the
slider's range (the user's, else the usual one). A full-scale signal moves the value that far from where it is,
either way for both ways. The value never leaves the slider's range (widen it for more room), so the overshoot
toggle is gone. The knob, the percentage box and the distance box are views of the one number, all from −100% to
100%. This supersedes the peak-to-peak and overshoot decisions above; format 9 migrates old graphs.

### Logarithmic and exponential sliders are removed (October 2026)

They confuse users. Sliders are linear. Parameters that are naturally multiplicative (frequencies) get that
behavior from their units, not from a warped slider. To keep linear sliders usable, the usual ranges of the frequency
parameters were narrowed (Cutoff to 0.01–200, Phaser frequency to 20–5000 Hz, Equalizer corners to 0.01–500); 

Octave modulation is dropped too (also October 2026): with a linear slider, a percentage of a 14-octave span made the
last few percent of the knob cover the whole slider. Frequencies modulate linearly. Graph format 6 converts saved
octave amounts to the linear amount that moves the value as far at its base value (exact there, approximate
elsewhere), so old graphs sound and look close to, but not identical with, what they did.

### Integer parameters are a real type (October 2026)

A parameter that can only be a whole number (channel counts, divisions, steps, voices) is declared as an integer in
its spec, and the slider and value box always snap to whole values. A float is never allowed for these.

### One unit type for time and frequency (October 2026)

Time and frequency are the same domain, and both are "samples per unit" converted by multiplying or dividing. They
become one `Unit` enum (pixel, sample, row, frame, ms, second, beat, bar) with one label set. A time parameter
multiplies by the unit's sample count; a frequency parameter divides, and its label says "cycles per". The old
`Hertz` option is `second` read as a frequency. Old graphs are migrated.

Done (October 2026), with the pixel and sample units from the next decision. Names are singular and lowercase
(`pixel, sample, row, frame, ms, second, beat, bar`); a frequency parameter's dropdown is labelled **Cycles per**
instead of **Unit**. Old graphs are upgraded by `migrate.rs` (`rows`/`Row` → `row`, `Hertz`/`seconds` → `second`, and
so on, on every node with a `unit` parameter).

### The "users never see samples" principle is dropped (October 2026)

It was outdated and hid a useful thing from the user. Pixels and samples become ordinary units. Resolution-
independent units (rows, frames, time, beats) stay the default because they make a preview match the export, not
because samples are forbidden.

### Per-node `mix` stays (October 2026)

A small optional `mix` on a node is fine. Blend modes exist as the Blend node and are not a reason to strip `mix`.
What changes is that `mix` is one shared parameter definition (`ParamSpec::mix()`) and one dry/wet helper
(`dsp::mix`), and a node omits it only where it is meaningless. Every `mix` starts at 1 (fully processed): no
per-node presets. Graphs from before version 10 keep the lower defaults Reverb (0.3), Phaser, Flanger and Chorus (0.5)
used to have, written out on load. Wording specific to a node lives in its description, not in the `mix` help.

### Modulation toggle off by default for constants (October 2026)

A Constant's *value* has no modulation pin exposed by default: nobody modulates a constant.

### Pixel is a project pixel; sample is the render's sample (October 2026)

A **pixel** is a pixel of the project (full) resolution and is scaled with the preview scale, so a downscaled
preview is as consistent with the full render as possible. A **sample** is the literal sample at the render in front
of you, so it changes with preview scale, and its help text says so.

### Modulation is allowed unless infeasible (October 2026)

RasterSong is datamoshing software, so the user is not protected from parameters they might want to modulate.
A parameter is `fixed` only when modulating it is truly infeasible or unreasonably hard, and every `fixed` carries a
reason shown in the inspector. "Nobody asked for it yet" and "it's structural" are not reasons by themselves: a
structural parameter such as voices or steps can usually be modulated by allocating the maximum and rounding the
value per sample.

### Duplicate and paste keep connections by default, with an option (October 2026)

Keeping the input connections of duplicated and pasted nodes is controlled by both a setting and a keybind. The
setting sets the default; the keybind does the opposite for one action. See the
[roadmap](roadmap.md#node-graph-editor).

## Open

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

### One Filter node; Low Pass removed (October 2026)

Filter has a slope (6, 12, 24 or 48 dB/oct) for low and high pass, built from Butterworth stages so it is flat by
default. Resonance is the filter's own Q: 0.707 is flat (the non-resonant setting) and higher peaks the cutoff, at
every slope except 6 dB, a single pole, which cannot resonate. Low Pass was exactly that one pole, so it is the
Filter at 6 dB/oct and old graphs are migrated. There is no separate "non-resonant" mode or node.
