# Render engine

The engine turns a compiled graph and the project's timeline into rendered frames, in the background, into a
cache. The GUI only talks to the engine; it never decodes or schedules anything itself.

## Crate Layout

```
crates/
  rastersong-media    FFmpeg I/O only: probing, frame index, video/audio decode, encode
  rastersong-graph    Pure Rust, no FFmpeg: Signal type, Node trait, scheduler, built-in nodes
  rastersong-lang     User-facing text: string tables (English embedded) and the lookup, see [Text and languages](text.md)
  rastersong-engine   Render service: sequential renderer, warmup, frame cache, cancellation, playback clock
  rastersong-cli      Headless file-in → file-out renderer (testing, benchmarking, batch use)
  rastersong-gui      egui application: viewer, node editor, parameters, timeline
xtask/                Developer tasks (fetch FFmpeg, generate test fixtures, packaging)
```

Dependency direction: `gui → engine → (media, graph)`, and `graph`, `engine` and `gui` use `lang`. `media` and `graph` never depend on each other.

## Timeline

The project is the clock, like a Premiere sequence: a **timebase** (width, height, frame rate) that no media file
owns. A project without one takes the first video track's, or 1920×1080 at 30 fps when it has no video; the app
sets it on the Project page of Settings. One block
of every graph is one project frame, frame `n` covering `n / fps` to `(n + 1) / fps` seconds, and the project
lasts to the end of its last item (`timeline.rs`).

Tracks hold **items** of one media file: a position on the timeline, in and out points in the file and a rate,
all in seconds so they survive a change of frame rate. Each track enters the graph as a source named after the
track, read by the input nodes that name it, through a track reader in the renderer:

- **Video tracks** are conformed to the grid: project frame `n` shows the file's frame displayed at the item's
  time for `n` (frames are held or skipped when the rates differ), scaled to the project size the way Video
  Output stretches.
- **Audio tracks** fill each frame's block with the audio of exactly that frame's time span, item by item
  (`Modulator::fill_items`), resampled when an item's rate isn't 1. The samples are read from the audio cache's
  memory map ([Media](media.md)), shared with the preview mixer and the waveform rather than copied.
- **Gaps read zeros**, as do muted items and input nodes that name no track. Where items overlap, the later one
  in the list plays (for audio, crossfading at its edges; see below).

**Audio item edges fade** over a fixed 5 ms (`EDGE_FADE`, at most half the item), linearly, so cuts don't click.
Each item is mixed over the items listed before it by its gain (`Item::gain_at`): inside the item it replaces
them, and at its edges it crossfades with them, so a cut between overlapping items never dips to silence. The
first sample of an item that starts at the beginning of its file fades in too. The preview mixer applies the same
gains.

Video frames are placed by the file's nominal frame rate, so variable-frame-rate video is conformed as if its rate
were constant.

## Sequential schedule

The graph is compiled into a **topologically sorted schedule** and rendered **sequentially**, one frame at a time,
in the style of an audio DAW. There is no pull-based random access inside the graph.

The graph compiler validates the graph (unknown nodes, ports or parameters, missing inputs, cycles, exactly one
output, layout mismatches), drops nodes that don't feed the output, orders the rest, and computes latency
compensation. Each frame, it runs the nodes in that order with no allocation.

**Lookahead as latency.** A node that needs future samples (e.g. a symmetric blur) reports it as latency and
delays its own output. The engine sums latency along each path and inserts compensating delays on shorter branches
so that signals meet in sync at Combine and other multi-input nodes (like plugin delay compensation in a DAW).
The engine pre-rolls enough frames to cover total latency.

## Seeking and warmup

The graph reports `K = max(warmup_frames)` over all nodes. When rendering starts at frame N:

1. `reset()` all nodes
2. Render frames `N-K .. N-1` and discard the output
3. Render from frame N onward, keeping the output

Nodes report their real settling length (the time to decay or settle to about 0.1%; `UNBOUNDED_WARMUP` for a node
that never settles) and never shorten it. The **Max warmup frames** project setting (default 120, at most 9999;
**File → Settings → Project**) limits only how many frames the renderer pre-renders after a seek: the effect itself,
such as a delay's buffer or a feedback tail, is untouched. Preview after a seek is therefore exact for nodes whose
memory fits within the limit and a close approximation otherwise. Requests that continue forward from where the
renderer already is, within the warmup length, skip the reset and just keep rendering. **Export always renders from
the first frame** (or later, from a saved state snapshot), so export is exact whatever the limit. When the graph
needs more than the limit, the Settings page names the node and node stats show it in the warning colour. Changing
the limit re-renders, like an edit.

## Always rendering ahead

A background render thread continuously renders forward from the playhead into the frame cache while the app is
open, whether playback is running or not. It fills the first missing frame in a window ahead of the playhead: 10
seconds by default, limited to ¾ of the cache budget (1 GiB by default) so some frames behind the playhead survive
for scrubbing back. When the cache is over budget, the frame farthest from the playhead is evicted, with frames
behind it counting as twice as far.

**Cache rule.** Rendered frames are keyed by `(graph_version, preview_scale, frame_index)`. Any graph edit
(parameter, node, connection) bumps `graph_version`, which invalidates everything rendered with the old version
and cancels in-flight work. Seeking never invalidates the cache.

**Cancellation.** The render thread checks between frames whether the project was edited, or the playhead moved so
the frame it is working on is outside the window. If so, it stops and picks the next job, so scrubbing or rapid
parameter tweaks never build a backlog. Playback moving the playhead forward doesn't cancel anything. A cancelled
render leaves the renderer consistent, so it can carry on later without starting over. Frames finished for an old
version are refused by the cache, so a stale frame can never be shown after an edit.

**Frame timing.** Each frame's audio block covers that frame's span of time. Containers round timestamps (Matroska
to whole milliseconds), so timestamps within 1 ms of the nominal grid `n / frame_rate` are snapped to it. Otherwise
frame spans would alternate between e.g. 33 and 34 ms and the modulation would wobble. Variable-frame-rate frames
sit well off the grid and keep their real times.

## Dynamic Playback

Preview playback speed is driven by how much is rendered ahead of the playhead:

```rust
playback_speed = min(buffered_seconds, 1.0)
```

- 1.0+ seconds buffered → full speed
- 0.5 seconds buffered → half speed
- Nothing buffered (just after a seek or edit) → paused, speeding up as frames render

This self-regulates: light graphs play at full speed, heavy graphs slow to a sustainable rate, and seeking or editing
naturally drops to zero and recovers. Playback never moves onto a frame that hasn't been rendered. The UI shows the
current speed and buffer depth.

## Preview Resolution

The user can choose a preview scale (e.g. full, ½, ¼). Processing cost scales with pixel count, so ¼ scale is
roughly 16× cheaper. Because all parameters are in normalized units, a reduced-resolution preview is a faithful
rough version of the full-resolution render. Export always renders at full project resolution.

## Track mix and output buses

The project's master audio is a set of **output buses** (`Bus`: a name and 1 to 8 channels), Main in stereo by
default; the first is the master, which the preview plays and the export writes. Each audio track is routed to one
bus (Main by default), and its volume and mute set its level there.

The **track mix** is what the timeline plays with no graph in the way: the picture of the top video track with an
item at each frame (the renderer's `@track_mix` source), and for each bus the sum of the unmuted tracks routed to
it at their volumes. A mono track plays in every channel of its bus; any other track's channels go to the bus's
channels in order. While any track is **soloed**, only the soloed tracks of its kind (video or audio) are in the mix;
a muted item reads as a gap everywhere, graphs included. Bypassing the whole graph shows the track mix's picture
and plays the master's track mix. Audio levels and routing only shape the track mix, so changing them keeps every
rendered frame; muting or soloing a video track changes the picture, so it renders again.

## Audio Output

A graph can have one **Audio Output** node per bus (optional; add it from the node menu), naming its bus in its
`bus` setting (Main by default). Its sound replaces that bus's track mix in the preview and the export. Without
one, with nothing connected to it, or with the graph bypassed, the bus plays its track mix; a track wired straight
into it is also used as it is, not re-rendered. The renderer renders one bus, the master by default (the CLI's
`--bus` picks another): only the Audio Output writing to it is compiled.

- **Any signal goes in.** One sample per pixel is mono and goes to every channel of the bus; anything else is
  written as interleaved samples across the bus's channels (a stereo signal to a stereo bus as it is; a picture,
  say, makes a raw sound, with a note). Nothing is converted on the way in: wire a video through Video to Audio
  first if you want its range mapped to `-1..1`.
- **Resampling.** The graph works in one block per frame, so the sink treats the blocks as one stream at
  `block length × frame rate` samples a second and resamples it to the project's audio rate (`audio_rate` in the
  project file, 48 kHz by default) with a windowed-sinc kernel. History carries across blocks, so block edges don't
  click. Picture-sized blocks are first averaged down in groups that divide the block evenly. The resampler is
  causal, which delays the sound by its kernel's half-width in input samples (a fraction of a millisecond for
  audio-rate blocks). Seeking renders at least one frame before the target, so a seek gives the same sound as
  playing through.
- **Sanitizing.** NaN and infinity become silence and the result is hard-clipped to `-1..1`. Use a Limiter to
  soften it.
- **Latency.** Both outputs line up at the later one's latency: the picture is held back to match the sound, or
  the other way round, so they stay in sync without the host doing anything.
- **Preview and export.** Each cached frame carries its sound, which preview playback reads. The CLI writes it to
  the `.mkv` as it renders.
- *Planned:* a volume control on the node (see the [roadmap](roadmap.md#nodes)).

## Taps and listening

A **tap** reads one connection (the output of a node) at one frame, for the editor's inspection popup. **Listening**
renders a connection's sound ahead of a position, for the listen key. Both are read-only:

- They are answered by a second renderer the service builds on demand, with its own video decoder and graph
  state, so a tap never moves the render-ahead, never touches the frame cache and is never part of the cache key.
  The second renderer is dropped when nothing asks and an edit has made it stale.
- Only the latest request is kept. `Engine::tap` returns `Pending` until the render thread answers (it calls the
  update callback), and an answer is dropped when the project is edited; the editor asks again. A request for a
  connection that doesn't feed the output (the graph prunes those) is answered `NotRendered`, and one for a graph
  that can't render likewise.
- A picture is the signal stretched over the project's shape, at most 320 pixels on its longest side, by the
  same `dsp::Stretcher` Video Output uses. Audio is kept as samples for the scope.
- Listening sends the connection's signal through the Audio Output's sink (sanitize, resample, clip), so what
  is heard is what an Audio Output wired there would play. The sound is kept for a second ahead of the position
  the caller reports with `Engine::listen`, and read through `Engine::listened_audio`, which plays in a `Mixer`
  like the rendered sound of a graph. Frames of a connection that isn't rendered are silent.
- The tap renderer renders one frame at a time on the render thread, ahead of render-ahead, so taps and
  listening make the cache wait a moment; the editor rate-limits its requests (the pointer rests on a
  connection first; while the playhead moves, a new frame is asked for at most as often as the project's
  `inspect_rate`, 5 a second by default).

## Graph layers

A project with graph items ([Graph layers](app.md#graph-layers), `Project::layer_set`) hands the engine a
`LayerSet`: the layers bottom first, each a list of non-overlapping items (graph id, position, length and start in
seconds, pre-roll, bindings), the stored graphs, and the id of the open graph, whose description the engine receives
separately through `Engine::set_graph` because it changes as it is edited. `Engine::set_layers` stores it (graphs
normalised with `render_form`, so labels and positions are not edits), it is part of what a render is built from, and
a change cancels and re-renders like any edit. Without graph items (`None`) the open graph renders over the whole
timeline exactly as before. Bypassing the whole graph drops the layers too, so the track mix shows. Offline, the
CLI's `rastersong render <project> <out>` passes the project's layer set in `RenderSettings::layers`.

**The model.** Layer *k* shows the output of the item playing at each frame (timeline time `n / fps`, half-open
spans like every item), and passes the picture of the layer below through where no item plays. The bottom layer's
*Layer below* is the track mix's picture; the top layer's output is the master. Every item gets its own compiled graph
and its own state; a graph placed twice is compiled twice. Before compiling, the item's bindings are written into
its graph: a Video Input or Audio Input reads the bound track, `@layer_below` for *Layer below*, or `@none` (zeros) for
no binding. The graph the editor has open is only a description until placed.

- **Pictures.** *Layer below* is the picture the layer below output; an input bound to nothing reads a picture of zeros.
- **Sound.** An Audio Input bound to *Layer below* reads the layer below's rendered sound (the Audio Output of the item
  playing there) when its layout is the same as the one the input was compiled for, which is the first Audio Output
  layout among the layer below's items; otherwise zeros. **Known limit:** the audio track mix is not available as a
  layer-below input yet, so the bottom layer's audio inputs bound to *Layer below* read zeros. Sound has its own host
  names (`@layer_below_audio`, `@none_audio`) because one name carries one layout and a graph can read both a picture
  and a sound from the layer below.
- **Latency.** Layers run as a pipeline. A layer's latency *L* is the largest latency of its items' graphs; at
  step *m* layer *k* processes source frame `m - (L of the layers below)`, reading tracks at that frame and the layer
  below's output of the same step, and emits output frame `source - L`. The total latency is the sum, and output frame
  *n* is ready at step `n + total`, as with one graph. An item with a latency below the layer's has its output held back
  in a small ring so every item of a layer is frame aligned, and the picture passed through where no item plays is
  held back likewise. The track readers are shared: one set of decoders serves all layers, and a second read of the
  tracks happens only for a layer whose offset differs from the one before.
- **Pre-roll and seeking.** An item with pre-roll runs, its output discarded, for its graph's warmup frames before its
  left edge (limited by **Max warmup frames**, and at least one frame when it renders sound); without pre-roll its
  state is reset at its first frame. A render that is not a continuation of the last resets every item, then warms up
  for the sum of the layers' longest warmups (limited by the same setting, as with one graph). Items already
  underway at that point simply run from there, so a seek into the middle of an item gives exactly what playing through
  gives for nodes with finite memory, as long as the memory fits the warmup.
- **Time.** An item's graph time counts from its `start` at its first frame; pre-roll frames before the start of the
  timeline count from zero.
- **Sound out.** The render's sound (`AudioSink::Rendered`) is that of the top-most layer whose item playing at the
  frame has an Audio Output on the rendered bus; every such item has its own resampler. Any item with an Audio Output
  on the bus makes the sink rendered. **Known limit:** frames where no
  item supplies sound are silent (the audio track mix is not mixed in under the layers yet), and an Audio Input wired
  straight to an Audio Output is rendered rather than played from its track as it is.
- **Taps, levels and statistics.** `tap` looks in the items that played at the last rendered frame, the open graph's
  first and then the top-most layer's down, and reads each item's graph at the last source frame it processed (which,
  with latency, is a little after the frame shown). `levels`, `costs`, `meters` and `param_levels` come from the open
  graph's item playing then, else the top-most one; frames where no item plays have none. `node_stats` and
  `compile_options` describe the open graph's first item, else the first. The tap renderer is built the same way as
  the render-ahead renderer.
- **Errors.** A graph item that cannot compile fails the build, and the failure names the graph's id. An item of a
  graph the project does not have is left out.

## Export / Offline Rendering

Export uses the same graph and engine as preview, rendering every frame in order from the start at full resolution
as fast as possible, with progress and estimated time remaining.

Initial export formats:

- **H.264 MP4** for sharing, encoded through OS/hardware encoders (Media Foundation on Windows, VideoToolbox on macOS, NVENC/QSV/AMF where available)
- **ProRes** and **FFV1** for high-quality / lossless output, using FFmpeg's built-in encoders

The CLI already renders to `.mkv` or a directory of PNG frames; the GUI has no export yet.

## Performance

- CPU only. Many of the most interesting effects (IIR filters, feedback, delays) are inherently sequential across the entire carrier stream, so they can't be spread across GPU threads anyway.
- SIMD for per-sample operations.
- Parallelism across independent graph branches and channels (`rayon`), not within a sequential stream.
- Reusable buffer pool; no allocation during `process`.
- Reduced-resolution preview is the main lever for heavy graphs.
- *Planned:* per-node cost measurement shown in the editor, so slow nodes can be found (see the
  [roadmap](roadmap.md#node-graph-editor)). Measured numbers for the nodes live in [benchmarks.md](benchmarks.md).
