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
custom. Snapping, link colors, parameter modulation pins, tooltips and connection inspection all need that control.

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
  yet. *(Resolved since: without an Audio Output the CLI writes the bus's track mix; see
  [Track mix and output buses](#track-mix-and-output-buses-october-2026).)*

### Resampling is only applied when needed

`graph.rs` creates a resample buffer only for secondary inputs that are unconnected or differ in length from the
main input. Both gaps found are closed: `Modulator::fill_block` copies directly when a block maps one-to-one onto
whole source frames, and the decode-time `Resampler` is skipped when the decoded audio is already interleaved `f32`
at the target rate and channel layout.

### Look/Listen replaces the Probe/Inspect tool (October 2026)

*Superseded by [Inspection replaces the Look and Listen tools](#inspection-replaces-the-look-and-listen-tools-october-2026).*

The earlier plan was a *probe tool*: a magnifying glass for audio that fades the master down and fades nearby links
up, by distance from the cursor. It was replaced by Look and Listen tools (hold a key and hover a connection to see
or hear its output), which were in turn replaced by inspection on hover. The distance-based master fade mixer is
dropped; listening plays one tapped connection.

### Modulation amounts become percentages of the parameter's span (October 2026)

*Superseded in part by [Modulation is one rule](#modulation-is-one-rule-october-2026): the peak-to-peak and
overshoot rules below no longer apply; the 25% one-way default for a new connection still does.*

Hands-on testing showed that dialling in modulation with *minimum, maximum, base value and amount* all in the
parameter's own units is unruly. The amount becomes a percentage of the parameter's min..max span, so the same
number means the same swing on any parameter.

Decided with it: *both ways*, the amount is the peak-to-peak swing, so 100% covers the span in either mode (an
older "±2" becomes the percentage that gives the same ±2). *Octave* scaling was dropped (see below): every parameter, frequencies included, takes a percentage of its linear
span. A new connection starts at 25%, one way, whatever the parameter and its
base value.

Also decided: modulation keeps the value between the slider's ends by default (widened to include a base value
typed beyond them), with a per-modulator toggle to allow overshoot up to the parameter's limits. The modulator menu takes the amount as a percentage or as
a distance in the parameter's unit, kept in step.

### Modulation is one rule (October 2026)

Hands-on use showed too many special cases (peak to peak both ways but full swing one way, an overshoot toggle,
a knob that could leave its range). Now: the percentage is the only stored amount and is a percentage of the
slider's range (the user's, else the usual one). A full-scale signal moves the value that far from where it is,
either way for both ways. The value never leaves the slider's range (widen it for more room), so the overshoot
toggle is gone. The knob, the percentage box and the distance box are views of the one number, all from −100% to
100%. This supersedes the peak-to-peak and overshoot decisions above.

### Logarithmic and exponential sliders are removed (October 2026)

They confuse users. Sliders are linear. Parameters that are naturally multiplicative (frequencies) get that
behavior from their units, not from a warped slider. To keep linear sliders usable, the usual ranges of the frequency
parameters were narrowed (Cutoff to 0.01–200, Phaser frequency to 20–5000 Hz, Equalizer corners to 0.01–500).

Octave modulation is dropped too (also October 2026): with a linear slider, a percentage of a 14-octave span made the
last few percent of the knob cover the whole slider. Frequencies modulate linearly.

### Integer parameters are a real type (October 2026)

A parameter that can only be a whole number (channel counts, divisions, steps, voices) is declared as an integer in
its spec, and the slider and value box always snap to whole values. A float is never allowed for these.

### One unit type for time and frequency (October 2026)

Time and frequency are the same domain, and both are "samples per unit" converted by multiplying or dividing. They
become one `Unit` enum (pixel, sample, row, frame, ms, second, beat, bar) with one label set. A time parameter
multiplies by the unit's sample count; a frequency parameter divides, and its label says "cycles per". The old
`Hertz` option is `second` read as a frequency.

Done (October 2026), with the pixel and sample units from the next decision. Names are singular and lowercase
(`pixel, sample, row, frame, ms, second, beat, bar`); a frequency parameter's dropdown is labelled **Cycles per**
instead of **Unit**.

### The "users never see samples" principle is dropped (October 2026)

It was outdated and hid a useful thing from the user. Pixels and samples become ordinary units. Resolution-
independent units (rows, frames, time, beats) stay the default because they make a preview match the export, not
because samples are forbidden.

### Per-node `mix` stays (October 2026)

A small optional `mix` on a node is fine. Blend modes exist as the Blend node and are not a reason to strip `mix`.
What changes is that `mix` is one shared parameter definition (`ParamSpec::mix()`) and one dry/wet helper
(`dsp::mix`), and a node omits it only where it is meaningless. Every `mix` starts at 1 (fully processed): no
per-node presets. Wording specific to a node lives in its description, not in the `mix` help.

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

### Timeline, resources and graph layers (October 2026)

The one-video timeline, single graph and plain JSON project become a multi-track editor with several graphs. The
plan is in the [roadmap](roadmap.md#timeline-resources-and-routing); the reasons for its main choices are below.
The graph-layer choices (*stacked graph layers*, *layer below*, track links) are superseded by
[Timeline routing: folders and graphs as FX](#timeline-routing-folders-and-graphs-as-fx-october-2026).

- **Explicit over hidden.** Every signal enters a graph through a port that something visibly filled; no
  fallbacks, no situational overrides. The old Audio Output rules (replace the source audio, except when nothing is
  connected, bypassed or a track is wired straight in) were hard to predict, and hidden fallbacks make failures hard
  to diagnose. Implicit behaviour needs a clear reason (Video Output's stretch) and is documented where it happens.
  The code still has one interim exception: with no graph items, the open graph renders over the whole timeline
  (see [Graph layers](#graph-layers-october-2026)); it is removed in roadmap stage 2a.
- **The project has its own timebase**, like a Premiere sequence. With several videos, images and raw files, no
  source can be the clock.
- **One resource per track.** A graph is compiled for a fixed layout per source; one resource per track keeps a
  track's layout fixed along the timeline and avoids mixing rates within a track. The resource can still be cut up
  freely.
- **The parent fills a graph's ports, with no fallback to a track.** As a subgraph the parent graph wires them; on
  a layer the item binds them. One rule for both, graphs become reusable tools, and a fallback binding was rejected
  as a second, hidden way in.
- **Stacked graph layers rather than one graph lane** *(superseded: graphs become FX on tracks and folders)*. A single lane forced every graph to rewire everything it
  didn't change (all the audio, for a video effect). Layers stack like adjustment layers, with the track mix at the
  bottom, and new graphs start as a visible passthrough, so a graph only touches what it changes. Layers pass only
  master outputs; control signals are shared through subgraphs. The two systems work together: layers stack
  finished results, subgraphs share signals.
- **Pre-roll by default, per item.** A graph item warms up as if it had been running before its left edge, as
  after a seek, so trimming the edge never changes what follows. Starting cold stays available.
- **The project file will be a zip** (planned; it is still plain JSON) with embedded files stored uncompressed. Plain JSON can't hold multi-gigabyte media;
  a project folder would not be one file. Embedding is an explicit action because saving rewrites the zip.
- **Audio is decoded to cache files and memory-mapped**, not held in memory. In memory, 48 kHz stereo takes about
  23 MB a minute and each track was held about three times, so twenty 5-minute stems would take around 7 GB.
  Streaming from the compressed file instead would bring back inexact seeks, encoder priming on every seek and a
  decoder per reader. Mapping reuses the raw-file reader and the reading code still sees a slice of `f32`.
- **Output buses, and export writes the master output.** This settles the former open question on multi-track
  export: export renders the master video and each bus, as streams in one file or as separate files (separate by
  default with more than one bus, since most players only play the first audio stream).
- **Items have mute but no solo.** Solo on an item either does little (only its own track) or breaks graph
  layers (silencing what *Layer below* reads); tracks and layers keep solo.
- **Automation is a generator.** A curve is a function of time like Oscillator or Beat, sampled at the shared
  generator layout (audio rate by default, so ramps on audio-rate parameters don't step), and it snaps like
  everything else.
- **Glue makes edit lists**, for media and automation alike: one mechanism, nothing re-encoded, and Make unique
  bakes one when it should stop following its source.

### Video and audio tracks are separate lists (October 2026)

*Superseded by [Timeline routing: folders and graphs as FX](#timeline-routing-folders-and-graphs-as-fx-october-2026),
which replaces the two lists with one track tree; the code still has two lists until roadmap stage 2b.*

Built as the first stage of the timeline model. The project keeps video tracks and audio tracks in two lists, as
Premiere does, rather than one mixed stack: the track mix needs an order among video tracks (the top one with an
item wins) but audio tracks are summed, so an order between a video and an audio track would mean nothing. Images
and raw files read as video will join the video list.

Also decided with it:

- **A track's name is the source name** input nodes read it by, unique across both lists, until Input port nodes
  replace them. The video track is named after its file, extension included, so the video's own sound track
  (named without it) doesn't clash; video inputs follow the video track's name.
- **Item times are seconds**: position, in and out points in the file, and a rate in file seconds per timeline
  second. An item without an out point plays to the end of the file.
- **The project lasts to the end of its last item**, audio included. An audio track longer than the video now
  lengthens the project; the extra frames show the gap's zeros.
- **No timebase until one is set**: a project takes the timebase of its first video track that has a resource, or
  1920×1080 at 30 fps, so opening a video still sets up the project the way it did.

### Audio item fades and the audio cache (October 2026)

Built as the second step of the timeline model.

- **Fades are 5 ms, linear, at every item edge**, the start of a file included: long enough to stop clicks, short
  enough not to soften a transient anyone would hear. An item is mixed over the earlier items by its gain, so where
  items overlap the later one still wins, but its edges crossfade with what is below instead of dipping to zero.
- **Cache files are keyed by content, not by path**, so a moved, renamed or copied file reuses its cache and an
  edited one gets a new file. Hashing is remembered against path, size and modified time, because hashing a long
  video on every open would cost seconds.
- **A cache that can't be written is not an error**: the audio is decoded into memory as before, with a warning in
  the log. Tests and the engine's default configuration use no cache; the app and the CLI use the user's cache
  directory.

### Track mix and output buses (October 2026)

Built as the third step of the timeline model.

- **One bus is rendered at a time.** The renderer compiles only the Audio Output of the bus it renders (the
  master, unless the CLI's `--bus` picks another), so a graph with outputs for stems costs nothing extra in the
  preview. Export of every bus waits for GUI export.
- **Removing a bus moves its tracks to the master** and leaves Audio Outputs that write to it pointing at a bus
  that isn't there (not rendered, shown in red), rather than silently re-pointing them, which could put two on the
  master. The removal warning lists both. Renaming follows through to tracks and Audio Outputs.
- **Mono fills every channel; other signals map channel for channel.** A stereo track on a 5.1 bus plays in its
  first two channels; a mono Audio Output on a stereo bus plays in both. The preview plays a bus's first two
  channels.
- **Bypassing the whole graph (graph layers included) is the "no graph" case**: it shows the track mix's picture
  (the top video track with an item) and plays the master's track mix.

### Tracks of items (October 2026)

Built as the timeline's fourth step, before item editing.

- **An item is dragged by its header bar only.** The area below belongs to the item's content (thumbnails and
  waveforms now, automation points later), so a click or drag there seeks like empty lane space.
- **Solo is a flag, not a set of mutes.** A soloed track leaves the other tracks *of its kind* out of the track mix
  until it is un-soloed; mutes are untouched, so un-soloing needs no memory of them. This replaces Alt+click on
  mute. Video solo and mute pick which track the track mix's picture shows; graphs read every track either way.
- **Linked tracks move the items that overlap the dragged one** *(superseded: links move from tracks to item
  groups, see [Timeline routing](#timeline-routing-folders-and-graphs-as-fx-october-2026))*. Items carry no link of their own, so which items
  belong together is read from time: a cut on one track and the matching cut on a linked track overlap. A move that
  would push any of them before the start of the timeline stops where the first reaches it. Splitting together
  comes with item editing.
- **Track height and links are saved with the project** (`height`, `link` on the track), like mute and solo.
- File → Open Video replaces the video tracks; the Resources panel adds more video tracks, which is how tracks
  get their files.

### Item editing (October 2026)

Built as the timeline's fifth step.

- **Linked tracks act together on every edit** *(to become item groups, see
  [Timeline routing](#timeline-routing-folders-and-graphs-as-fx-october-2026))*. Move, split, delete, copy and paste take the items of linked tracks
  that overlap the ones edited, read from time as for moves. A trim takes only the linked edges at the same time
  (within a millisecond), so trimming a cut that lines up on both tracks keeps it lined up, and an item that only
  partly overlaps isn't cut short.
- **S with nothing selected splits everything under the playhead**, as in Reaper. With a selection only the
  selection (and its linked items) is split, and both halves stay selected.
- **Rate drags keep the far edge and the in and out points**, within 0.05× to 20×. An edge can't be trimmed past
  the file's ends or shorter than 10 ms.
- **The timeline has its own clipboard** for items, separate from the graph's text clipboard. Ctrl+C, Ctrl+X and
  Ctrl+V go to whichever the pointer is over. Pasting puts items back on the tracks they came from (a track of
  one resource can't take another's items), the earliest at the playhead.
- **Snapping works from the pointer's whole movement**, not frame by frame, so an item can be pulled off a target.
  It snaps to the ruler's ticks (frames and seconds, or beats and bars), other items' edges and the playhead,
  within 8 pixels. Shift drags freely; the Snap toggle is an app setting, on by default.
- **The selection isn't part of the project**: it isn't saved, and undo keeps it where the items still exist.

### Resources panel (October 2026)

Built as the first step of stage 2 of [Timeline, resources and routing](roadmap.md#timeline-resources-and-routing).

- **Tracks point at resources, not files.** A resource is one stream of a linked file (`id`, `name`, `kind`,
  `path`, `stream`), and a track names its resource by id. The engine still receives a path and a stream per
  track, so renderers and caches only gained the stream index.
- **Streams are chosen by their index in the file.** The import dialog stores the index it listed; `None` (the
  file's best stream of the kind) is kept for File → Open Video and Add Audio Tracks, which don't ask. The audio
  cache key includes a chosen index, so two audio streams of one file get two cache files.
- **Any file with more than one usable stream asks**, including an ordinary video with its sound: the roadmap's
  rule, with every stream ticked so importing both is one click.
- **Importing doesn't touch the timeline.** Placing a resource is a separate step (drag, double-click, or Add to
  Timeline), so importing a folder of takes doesn't fill the timeline.
- **Removing a resource removes its tracks**, after a warning naming them, rather than leaving tracks with nothing
  to play. Renaming a resource leaves its tracks' names alone, since graphs select tracks by name.
- **Resource ids are unique, not permanent.** A new resource can reuse a removed one's id once nothing points at
  it; undo restores whole projects, so nothing can hold a stale id.

### Empty tracks, Relocate and graph resources (October 2026)

Built as the second step of stage 2.

- **Only the first video track used to get a length and thumbnails**, so a second video track (the same resource
  dragged on twice, say) sat on "loading" for ever although the engine could render it. Thumbnails are now made per
  video stream (path and stream index), and tracks of one resource share a decoder.
- **An empty track has no resource** (`resource` left out, no items) and fills with the first resource of its kind
  dropped on it, because a track holds one resource only. Empty tracks are not sent to the engine. "The video"
  (the track graphs and the project timebase follow) is the first video track that has a resource.
- **A resource that can't be dropped on a track makes a new track** (another resource, the wrong kind, no track
  under the pointer), so a drop never silently does nothing and never changes what a track holds.
- **Missing files are left out of the timeline sent to the engine**, rather than failing the whole render the way an
  unopenable video does: their tracks read as gaps until found. Existence comes from the media backend, so the fake
  backend can say a file is there without a real path. Relocating moves every resource that reads the same file.
- **Graphs are kept as one open graph plus the others.** `Project::graph` stays the open graph, so the editor, the
  engine and every existing caller are unchanged; the other graphs wait in `graphs` and opening one swaps it with
  the open graph. This is a stepping stone: when graph layers place graphs on the timeline, each graph becomes a
  plain entry in one list.
- **Input and Output port nodes wait for graph layers.** A port is filled by a layer item's bindings or a
  subgraph's pins; with neither, a port node would be today's Video Input and Audio Input under another name, and
  the project-linked node machinery would have to be rewritten twice. *(Done in stage 2b: see
  [Input ports](#input-ports-october-2026).)*

### Graph layers (October 2026)

Built as the third step of stage 2 ([Graph layers](engine.md#graph-layers)). *Superseded by
[Timeline routing: folders and graphs as FX](#timeline-routing-folders-and-graphs-as-fx-october-2026); this is how
the code works until roadmap stage 2b replaces it.*

- **Items on a layer never overlap, and the model enforces it.** Placing or moving an item trims, cuts or removes
  what it lands on (`Project::place_graph`), and loading a file repairs overlaps the same way, so the renderer can
  rely on one item per layer per frame.
- **Graph items are not track items.** They have no resource, no rate and no stretch, so they get their own small
  type (`GraphItem`: position, length, `start` into the graph's own time, mute, pre-roll, bindings). Trimming the
  left edge moves `start`, which is what keeps what follows unchanged.
- **Bindings are keyed by input node id and rewrite the node's source at compile time** *(now by port name, see
  [Input ports](#input-ports-october-2026))*. Until the Input and Output
  port nodes exist, the Video Input and Audio Input nodes are the ports: an item binds each to *Layer below* (the
  reserved sources `@layer_below` and `@layer_below_audio`), a track (its name) or nothing (`@none` and
  `@none_audio`, zeros). The bottom layer's video *Layer below* is `@track_mix`. No fallback applies to an unbound
  input.
- **With no graph items the open graph still renders over the whole timeline.** Once a layer holds an item the
  project renders only what the layers say, and a frame with no item is transparent. This keeps a fresh project
  working the way it did before layers, and makes the first placed item the explicit switch to layered rendering.
  *(Reversed: it is an implicit graph, which the explicit-over-hidden rule rejects; see
  [Timeline routing](#timeline-routing-folders-and-graphs-as-fx-october-2026).)*
- **Each item has its own compiled graph and state; layers form a pipeline.** A layer's latency is the largest of its
  graphs and shorter ones are delayed to match, so output frame N always comes from source frame N plus the stack's
  total latency. Pre-roll runs the graph for its warm-up before the item's left edge.
- **Moving a graph item is applied when the drag ends.** The model trims what a moved item lands on, which would
  cut neighbours for good while the pointer merely passes over them, and re-sorts the items (so an item's index, and
  with it its widget, changes mid-drag). The timeline draws where the item would land and sends one move on release;
  trimming an edge stays within the neighbours, so it applies live. Track items and graph items share the header
  bar, edge handles, snapping and drag state in `timeline.rs`; only what a drag emits differs.
- **A new layer is made by dropping off the layers, with no permanent drop zone.** An empty "drop here" strip would
  shift every track row for projects that use no layers; instead a hint appears while a graph is dragged over
  anything but a layer.

### One Filter node; Low Pass removed (October 2026)

Filter has a slope (6, 12, 24 or 48 dB/oct) for low and high pass, built from Butterworth stages so it is flat by
default. Resonance is the filter's own Q: 0.707 is flat (the non-resonant setting) and higher peaks the cutoff, at
every slope except 6 dB, a single pole, which cannot resonate. Low Pass was exactly that one pole, so it is the
Filter at 6 dB/oct. There is no separate "non-resonant" mode or node.

### No migrations before 1.0 (October 2026)

The graph and project formats are version 0 until the first stable release. Breaking changes happen freely: old files
may stop loading, and a file with any other version is rejected with a clear message. Only demos exist, and keeping
every historical rewrite alive (1,100 lines in `migrate.rs`) made renames and merges expensive, and a change made and
later reverted would have needed migrating twice. The migration tooling stays (`migrate/`, with the rename helpers),
and from 1.0 each format change bumps the version and adds one step file. Mentions of migrations above describe
history and no longer apply to the code. Example graphs in `examples/graphs/` are kept current by hand.

### Text lives in lang files (October 2026)

Every user-facing string is in a `key = value` table keyed by a stable id, English embedded and other languages
loaded from folders next to the program. The format is a plain line-based file rather than Fluent or JSON: no new
dependency, comments, wrapped lines, a diff that reads well, and `{name}` placeholders cover what the interface
needs (a plural gets one key per form). Node specs no longer carry text; the node's label, help and port notes are
in `nodes.lang`, which a test checks against the registry so nothing is missing or stale. See
[Text and languages](text.md).

### Inspection replaces the Look and Listen tools (October 2026)

The Look and Listen tools were built as modes with a toolbar, and hands-on testing showed they were not the best
way in: the Select tool already shows meters on hover, so a separate mode only hid them. Inspection is now the
default behaviour of hovering a connection. A held modifier (Alt) and the wheel go through the views (picture, scope,
spectrum, readings only; any signal in any view), and listening is a second held modifier (Shift) rather than one of the views,
so a connection can be watched and heard at once. The update rate while the playhead moves is a project setting,
since how fast a project can afford to render is a property of the project. The views are shared widgets that take
plain data. See [Inspecting connections](roadmap.md#inspecting-connections).

### Timeline routing: folders and graphs as FX (October 2026)

Hands-on use of graph layers showed two problems. *Layer below* was hard to reason about, because a stack of graph
lanes sat apart from the tracks it read. It was also wrong for audio: the engine never mixed the audio tracks (the
app and the CLI summed them only after rendering), so a bottom layer's audio input read zeros. Two models were
weighed:

- **Premiere-style:** three fixed groups (Video feeding a video bus, Audio feeding an audio bus, Control feeding
  neither) with graph lanes above that read a bus or a track. This is the smaller change and readable for video
  editors. But the topology is fixed: processing one track needs a per-track binding, which brings most of today's
  binding complexity back. Lanes and tracks stay two worlds with their own timing, and stems or extra buses don't
  fit three fixed groups.
- **Reaper-style:** one track tree in which folders are buses and graphs are FX. This is the larger rewrite, and
  routing in this style can overwhelm newcomers.

**Chosen: Reaper-style routing, with the Premiere layout as the default template.**

- **One track tree.** Any track holds items of any kind. A folder track composites its children's video top-down
  and sums their audio, inside the renderer, then sends the result to its parent. Each track has a master send.
  One mechanism covers groups, buses and stems, and folders collapse like Reaper's.
- **New projects start with three folders:** *Video*, *Audio* and *Control*. Control has its master send off, so
  text files, executables, automation clips and the like can drive graphs without reaching the output. A Premiere
  user sees the familiar groups; a Reaper user can restructure them freely.
- **Graphs are FX.** A graph sits in an FX chain on a track, a folder or the master, and its main input is the
  signal of the track it is on, so *Layer below* disappears. Extra input ports are filled by **receives** from other
  tracks, shown in the track's routing; this keeps "the parent fills the ports" without a binding per input. Graphs
  that change over time are **item FX** (Reaper's take FX). They keep today's graph items' span, trim and pre-roll.
- **An FX graph without an Audio Output (or Video Output) passes that stream through unchanged**, and the chain
  shows a tag such as "audio: through". This is the DAW convention: an effect leaves alone the channels it doesn't
  touch. It is not a hidden fallback, because the tag makes it visible; silence with a note would make every video
  effect mute its track.
- **Nothing placed means nothing applied.** With no FX anywhere, the output is the plain mix. The interim rule that
  rendered the open graph over the whole timeline is dropped.
- **Items are grouped, not tracks linked.** Select items, then right-click → *Group* (Ctrl+G). Grouped items move,
  trim, split and delete together. A multi-stream import groups its items. Tracks can't be linked.
- **Automation lanes.** An automation node in a graph shows as an envelope lane under the track hosting that graph,
  edited as the automation-curve design already describes; automation clips stay available as items.
- **Order:** the bugs that don't depend on the model (track height reset, drag feedback, drop preview, the implicit
  graph, video track reorder) are fixed first, then the model is rebuilt. No migration is needed (version 0).

See [Timeline, resources and routing](roadmap.md#timeline-resources-and-routing).

### Input ports (October 2026)

The first step of roadmap stage 2b. Video Input and Audio Input stop naming tracks and become the graph's input
ports.

- **The same two node types, with a `port` setting instead of `source`.** A port's kind (picture or sound) decides
  its signal's layout, so the kind stays in the node type rather than becoming a setting. `Video` and `Audio` are
  the main ports, which a host track fills once graphs are FX; any other name is a port filled by a receive.
- **Ports are filled by name, not by node.** Two nodes reading one port read the same signal, so a graph can tap
  its input in several places without wiring one node across the canvas. A video port and an audio port with the
  same name read the picture and the sound of whatever fills that name. Graph item bindings are keyed by port name.
- **The project no longer manages any node.** The linked nodes (one Video node for the video, one Audio node per
  audio track, renamed and removed with their tracks) are gone: they tied a reusable graph to one project's track
  names. Ports are added, renamed and deleted like any node; only the Output is kept, because every graph has
  exactly one.
- **Renaming a track renames what reads it**, which is now bindings (receives, once they exist), not graph nodes.

## Open

### Open: "Audio to video" / "Video to audio" names

They are confusing. The names should describe the effect, which is the range change, with the ranges stated in the
node description. Candidates: "Brightness to Wave" / "Wave to Brightness". Not decided.

### Open: Interleave and Pack semantics

What `interleave` and `pack` should mean (currently a relabel between RGB and a 3×-wide mono carrier).

### Open: value ranges and mono-only modulators

Value ranges (video `0..1`, audio `-1..1`) and mono-only modulators are still to be settled by using the app.

## Open Questions

- **License review:** the custom LICENSE is a first draft. Have it reviewed, or switch to an established source-available license (e.g. PolyForm Strict), before any paid release.
- **Snapshots:** memory budget and spacing for state snapshots.
- **Hardware encoder fallback:** what to offer for H.264 export on machines with no usable OS/hardware encoder.
