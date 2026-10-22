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
xtask/                Developer tasks (fetch and build FFmpeg, generate test fixtures, generate node docs, formatting, packaging)
```

Dependency direction: `gui → engine → (media, graph)` and `cli → engine`; every crate except `lang` itself uses `lang`. `media` and `graph` never depend on each other.

## Timeline

The project is the clock, like a Premiere sequence: a **timebase** (width, height, frame rate) that no media file
owns. A project without one takes the first video track's, or 1920×1080 at 30 fps when it has no video; the app
sets it on the Project page of Settings. One block
of every graph is one project frame, frame `n` covering `n / fps` to `(n + 1) / fps` seconds, and the project
lasts to the end of its last item (`timeline.rs`).

Tracks hold **items** of one media file: a position on the timeline, in and out points in the file and a rate,
all in seconds so they survive a change of frame rate. Each track is read for the [composition](#composition) through a
track reader in the renderer:

- **Video tracks** are conformed to the grid: project frame `n` shows the file's frame displayed at the item's
  time for `n` (frames are held or skipped when the rates differ), scaled to the project size the way Video
  Output stretches.
- **Audio tracks** fill each frame's block with the audio of exactly that frame's time span, item by item
  (`Modulator::fill_items`), resampled when an item's rate isn't 1. The samples are read from the audio cache's
  memory map ([Media](media.md)), shared with the preview mixer and the waveform rather than copied.
- **Gaps read zeros**, as do muted items and input ports nothing fills. Where items overlap, the later one in
  the list plays (for audio, crossfading at its edges; see below).

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
behind it counting as twice as far. With a loop region active, the window wraps from the region's end to its
start, so the loop plays without waiting.

**Cache rule.** Rendered frames are keyed by `(graph_version, preview_scale, frame_index)`. Any graph edit
(parameter, node, connection) bumps `graph_version`, which invalidates everything rendered with the old version
and cancels in-flight work. Seeking never invalidates the cache.

**Cancellation.** The render thread checks between frames whether the project was edited, or the playhead moved so
the frame it is working on is outside the window. If so, it stops and picks the next job, so scrubbing or rapid
parameter tweaks never build a backlog. Playback moving the playhead forward doesn't cancel anything. A cancelled
render leaves the renderer consistent, so it can carry on later without starting over. Frames finished for an old
version are refused by the cache, so a stale frame can never be shown after an edit.

**Frame timing.** Each project frame's audio block covers exactly `n / fps .. (n + 1) / fps` of the project
grid, and video frames are picked by the source's nominal frame rate, so container timestamp rounding (Matroska's
whole milliseconds) never makes frame spans alternate or modulation wobble. Variable-frame-rate video is conformed
as if its rate were constant.

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
bus (Main by default) at its volume, and left out when it is muted. The renderer mixes the tracks itself
([Composition](#composition)); the app's real-time mixer, which plays the mix while the graphs leave the sound
alone, gets each track's level from the project (`Project::timeline`).

The **track mix** is what the timeline plays with no graph item in the way: the picture of the top video track with
an item at each frame, and for each bus the sum of the unmuted tracks routed to it at their volumes. A mono track
plays in every channel of its bus; any other track's channels go to the bus's channels in order. While any track is
**soloed**, only the soloed tracks of its kind (video or audio) are in the mix. Mute and solo only shape the mix:
graphs can still read every track on its own. A muted item reads as a gap everywhere, graphs included. Bypassing
every graph shows the track mix's picture and plays the master's track mix.

## Audio Output

A graph can have one **Audio Output** node per bus (optional; add it from the node menu), naming its bus in its
`bus` setting (Main by default). On a graph track, its sound is the master's where the item is the topmost one
playing; a graph without one is silent there ([Composition](#composition)). Where the sound is still the plain mix
(the template's `In: Audio` wired straight to its Audio Output, reading what is below), the bus plays its track mix
rather than rendering it again. The renderer renders one bus, the master by default (the CLI's `--bus` picks
another): only the Audio Output writing to it is compiled.

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
- **Preview and export.** Each cached frame carries its sound, which preview playback reads. Export writes it to
  the file as it renders.
- **Volume.** The node's `volume` (in dB, −48 to 12 on the slider, −120 to 60 typed) applies before sanitizing and
  clipping, and a peak meter on the node shows the result.

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

## Composition

A project hands the engine its **composition** (`Project::composition`, `Engine::set_composition`): the video and
audio tracks, video first, each with its volume, bus and whether it is in the mix (not muted, not left out by a
solo); the graph tracks, top first, with the items that play (muted tracks and items are left out, and bypassing
graphs leaves out all of them); and the stored graphs. The open graph's description comes separately through
`Engine::set_graph` because it changes as it is edited. Graphs are normalised with `render_form`, so labels and
positions are not edits; any other change cancels and re-renders. Offline, the CLI's `rastersong render <project>
<out>` passes the composition in `RenderSettings::composition`.

**Graph items.** A `GraphItem` places a graph on a graph track: position, length, `start` (seconds into the graph's
own time at the left edge), mute, pre-roll and a `Source` for each input port. Items on one track never overlap
(`Project::place_graph` trims what an item lands on, and loading a file repairs overlaps). Each item is compiled
with its own state, so a graph used twice is compiled twice. Items of graphs the project doesn't have are left out.

**Ports and sources.** An input node reads the port its `port` setting names (Video Input `Video`, Audio Input
`Audio` by default). Each port of an item reads its source, matched by kind (a port read as a picture gets the
source's picture, one read as sound its sound):

- `Below`: what is below the item's graph track, which is the next graph track's output, or the mix under the
  lowest. The default for the main ports, `Video` read as a picture and `Audio` read as sound.
- `Mix`: the video mix or the audio mix.
- `Track(name)`: one video or audio track on its own (its stem), before its volume and whether it is in the mix,
  so a muted track can still drive a graph.
- `Graph { track, port }`: the output of a graph track below the item's (`Video` and `Audio` are its main outputs).
  A graph track at or above the item's reads zeros, so sources only point down and there are no loops.
- `Nothing`, and the default for every other port: zeros.

A port read both as a picture and as sound gets its source's picture and sound; inside the graph the sound is
compiled under its own name (`<port>\0sound`), since one name carries one layout. With no composition (`None`), the
engine renders the open graph over the whole timeline with each port named after a track reading that track
(`single_composition`), which the CLI's `render <video> <audio> <graph>` and the tests rely on.

**No passthrough.** Every graph needs its Output (compiling one without fails). A graph with no Audio Output on the
rendered bus is silent where it plays; nothing is wired through for it. Only the one-graph form keeps the old rule
that a graph without an Audio Output leaves the sound to the mix.

**Mixing.** The renderer runs the nodes once per source frame in order: each track's stem, the mix, then the graph
tracks from the bottom up.

- A **track** reads its media: its picture conformed to the project's grid and size, or its sound in its file's
  own layout. Tracks nothing reads (out of the mix and no item's source) aren't read at all.
- The **mix** shows the picture of the top video track in the mix that has one at that frame, and sums the audio
  tracks in the mix that are routed to the rendered bus at their volumes into the mix layout: `round(48000 / fps)`
  samples a frame across the bus's channels. Each block is fitted linearly to the mix layout, and a mono track goes
  to every channel.
- A **graph track** shows the output of the item playing at that frame, or what is below it where none plays. Its
  sound is fitted to the mix layout. The top graph track's output is the master, or the mix with no graph tracks.

**Latency.** Each item lines its inputs up at the latest of what is below and its other sources' latencies, holding
the others back in small rings, and adds its own; a graph track's latency is its slowest item's, or what is below's
if that is later, and quicker items (and what is below, where no item plays) are held back to it, so the stack stays
frame aligned. Each output is kept as long as its slowest reader needs it. The master's latency is the render's:
output frame *n* is ready at source step `n + latency`, as with one graph.

**Items in time.** An item's graph counts frames from its own start: graph frame = source frame +
`round(start × fps) − first`, where `first` is the item's first frame. An item with **pre-roll** runs, its output
discarded, for its graph's warmup frames before its left edge (limited by **Max warmup frames**); without it the
graph starts cold at the edge.

**Seeking.** A render that is not a continuation of the last resets every graph and warms up for the graph tracks'
warmups added up from the bottom (each track's slowest item; limited by the same setting, and at least one frame
when sound is rendered), so a seek gives what playing through gives for nodes with finite memory, as long as the
memory fits.

**Sound out.** When the master's sound is the plain audio mix (no graph item plays, or each item's Audio Output is
wired straight from a port reading the mix, at its own volume, directly or through what is below), the app's mixer
plays the mix (`AudioSink::TrackMix`), which keeps playback real-time. Otherwise the render's sound is the master's
(`AudioSink::Rendered`), through the same sink as one graph's (resample, sanitize, clip).

**Taps, levels and statistics.** `tap` looks in the graphs that ran in the last source step, those of the open graph
first, the top graph track's first. `levels`, `costs`, `meters` and `param_levels` come from the first graph that
ran playing the open graph, else the first that ran; `node_stats` and `compile_options` describe the open graph's
first item, else the first. The tap renderer is built the same way as the render-ahead renderer.

**Errors.** A graph that cannot compile fails the build (`EngineError::Fx`), and the failure names the graph's id
and the node at fault.

## Export / Offline Rendering

Export uses the same graph and engine as preview, rendering every frame in order from the start at full resolution
as fast as possible. `offline::render` reports the frames done and the total after each frame to a `progress`
callback, which cancels the render (`EngineError::Cancelled`) by returning `false`.

`export::export` is the one export path, used by the CLI and the GUI:

- `ExportJob::from_project` takes the timeline, the render form of the open graph, the composition, tempo and audio
  rate. `ExportJob::load` decodes each audio track (through the audio cache) into the render's tracks and the
  sources of the track mix.
- `FileSink` writes the frames through `MediaWriter` (see [Media](media.md)) with the bus's sound: the graph's
  Audio Output as it renders, or else the track mix of the tracks routed to the bus.
- A failed or cancelled export removes the file.

Formats: **H.264 MP4** with AAC for sharing, through GPU or OS encoders (NVENC, AMF, QSV, VideoToolbox, Media
Foundation); **ProRes** in QuickTime and lossless **FFV1** in Matroska, through FFmpeg's own encoders. The CLI picks
the format by extension (`.mp4`, `.mov`, `.mkv`); any other path is a directory of PNG frames.

## Performance

- CPU only. Many of the most interesting effects (IIR filters, feedback, delays) are inherently sequential across the entire carrier stream, so they can't be spread across GPU threads anyway.
- No allocation during `process`.
- Reduced-resolution preview is the main lever for heavy graphs.
- Graph tracks run in order on the render thread; running them as a pipeline across frames, as graph layers did, is
  not rebuilt yet.
- **Per-node cost** is measured around each node's `process` (`Graph::costs`, smoothed, never part of the cache key)
  and shown in the editor as a badge and a tint (View → Show performance). Measured numbers for the nodes live in
  [benchmarks.md](benchmarks.md).
- *Planned:* SIMD for per-sample operations; parallelism across independent graph branches and channels (not
  within a sequential stream); a reusable buffer pool.
