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

Every track holds **items** (`Item`) of resources of its kind, a graph, a video stream or an audio stream: the
resource it plays, a position on the timeline, in and out points in the resource and a rate, all in seconds so they
survive a change of frame rate. What differs between kinds is a capability of the kind (`TrackKind`), which the
one code path checks: whether its resources have a length of their own (`has_duration`), stretch to another rate
(`stretches`), read a file (`reads_file`), whether its tracks have a level in the mix (`has_level`), and whether
its items pre-roll (`pre_rolls`). Each video and audio track is read for the [composition](#composition) through
a track reader in the renderer, which reads each item from its own resource:

- **Video tracks** are conformed to the grid: project frame `n` shows the frame of the item playing at `n`,
  from that item's file, displayed at the item's time for `n` (frames are held or skipped when the rates differ),
  scaled to the project size the way Video Output stretches. Each track opens each video it plays once.
- **Audio tracks** fill each frame's block with the audio of exactly that frame's time span, adding each item
  that plays in it from its own file (`Modulator::add_item`), resampled when an item's rate isn't 1. A track's
  sound (its stem) takes the most channels and the longest block among its files; an item of another layout is
  fitted to it the way the mix fits a track (`mix_into`). The samples are read from the audio cache's memory map
  ([Media](media.md)), shared with the preview mixer and the waveform rather than copied.
- **Gaps read zeros**, as do muted items, items of a file that is missing and input ports nothing fills. Items on
  a track never overlap once edited; where a file holds overlapping items whose length isn't known on load, the
  later one in the list shows (audio items are summed).

**Editing** goes through one API for every kind (`project/editing.rs`), naming items by track and place
(`ItemRef`) and taking the lengths it needs as `ItemLengths` (each file's length once read, and how long a new
graph item lasts). Whatever an edit puts down trims what it lands on, cutting an item it lands inside in two
(`place_resource`, `move_items`, `paste_items`); a trim stops at the items either side and at the resource's ends,
and stretches only kinds that stretch (`trim_item`). An item whose length isn't known yet neither trims nor is
trimmed. Items move by time and across tracks within their section; a move that would leave its section moves
nothing. A track takes items of any resource of its kind. Grouped items move, trim, split and delete together, on
every kind of track. Removing a resource removes its items; its tracks stay. The engine decodes every audio
resource of the composition, placed or not, so its length is known before it lands on a track
(`Engine::loaded_audio`); a file an item plays that can't be decoded fails the build, one no item plays is left
out.

**Audio item edges fade** over a fixed 5 ms (`EDGE_FADE`, at most half the item), linearly, so cuts don't click
(`Item::gain_at`). Items never overlap, so a track's items are added up: at a cut the item before fades out and the
item after fades in, with no crossfade between them. The first sample of an item that starts at the beginning of
its file fades in too. The preview mixer applies the same gains, each item reading its own file (`MixTrack`).

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
start, so the loop plays without waiting. `Engine::cache_use` gives the memory the frames take and the budget,
which the app's status bar shows.

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

Every graph has one **Audio Output**, its main sound, which writes to the master bus; an **Audio Bus Output**
(add it from the node menu) writes to another bus named in its `bus` setting. On a graph track, the Audio Output's
sound is the master's where the item is the topmost one playing; a graph without one is silent there
([Composition](#composition)). Where the sound is still the plain mix (the template's `In: Audio` wired straight to
its Audio Output, reading what is below), the bus plays its track mix rather than rendering it again. The renderer
renders one bus, the master by default (the CLI's `--bus` picks another): on the master (`Bus::master`, passed to
the compiler as `CompileOptions::master`) only the Audio Output is compiled, on another bus only the Audio Bus
Output naming it; one naming the master is never rendered.

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
- Only the latest request is kept. `Engine::tap` returns `Pending` until the probe thread answers (it calls the
  update callback), and an answer is dropped when the project is edited; the editor asks again. A request for a
  connection that doesn't feed the output (the graph prunes those) is answered `NotRendered`, and one for a graph
  that can't render likewise.
- A picture is the signal stretched over the project's shape, at most 320 pixels on its longest side, by the
  same `dsp::Stretcher` Video Output uses. Audio is kept as samples for the scope.
- Listening sends the connection's signal through the Audio Output's sink (sanitize, resample, clip), so what
  is heard is what an Audio Output wired there would play. The sound is kept for a second ahead of the position
  the caller reports with `Engine::listen`, and read through `Engine::listened_audio`, which plays in a `Mixer`
  like the rendered sound of a graph. Frames of a connection that isn't rendered are silent.
- The tap renderer runs on its own thread (`rastersong-probe`) beside the render thread, so taps and listening
  never hold the render-ahead up, and the render-ahead never holds them up. The two threads share the decoded
  audio, so a song is decoded once. The editor rate-limits its requests (the pointer rests on a connection first;
  while the playhead moves, a new frame is asked for at most as often as the project's `inspect_rate`, 5 a second
  by default).

## Composition

A project hands the engine its **composition** (`Project::composition`, `Engine::set_composition`), the one thing
the renderer, the preview service, export and the CLI render: the timebase, the buses, every track top first (the
graph tracks, then the video, then the audio; `ComposedTrack`: its name, kind, items, level, bus and whether it is
in the mix, that is not muted and not left out by a solo), and the resources their items play (`ComposedResource`:
id, name and content, a file's stream or a graph). The app leaves out resources whose files are missing
(`Project::composition_with`), so their items read as gaps; bypassing graphs leaves out the graph tracks
(`Composition::bypassed`). A project whose items play nothing (`Composition::is_empty`) is idle; one with only
graph items renders them, its length the end of its last item (`Composition::end`, which counts graph items as long
as they are placed for and files' items as long as their files). Graphs are normalised with `render_form`, so labels
and positions are not edits; any other change cancels and re-renders, except audio levels and routing while no graph
item plays (`Composition::renders_like`): they only shape the track mix, which the app's mixer plays. Which graph the
editor shows is not part of the composition:
`Engine::set_inspected` names the graph resource whose taps, levels and statistics the editor wants. Inspecting
another graph renders the same frames, so it keeps the cache and only rebuilds the renderer for what is inspected.
A kept frame records the graph inspected when it was rendered (`Frame::inspected`), and the app shows its levels,
parameters, costs and meters only while that graph is still the open one. Offline, `offline::render` takes the
composition and its decoded audio (`AudioResources`, by resource) beside the `RenderSettings`.

**Graph items.** An item on a graph track places a graph: its position, its in and out points in the graph's own
time (`start` at the left edge, `end`, always set, as a graph has no end of its own), mute and pre-roll. What its
graph reads is set on the graph's input nodes. Items on one track never overlap (see [Editing](#timeline)). Each
item is compiled with its own state, so a graph used twice is compiled twice. Items of graphs the project doesn't
have are left out.
**The beat grid is the project's.** Beat-locked nodes follow the same grid as the metronome and the ruler's ticks
(the project's tempo and first beat), never one that starts with the item. A graph counts its frames from the item's
`start`, so its compile options carry the first beat moved into that count; where an item is placed or trimmed
doesn't move its beats. The options also carry the item's `Span` (its left edge in the graph's own time and its
length, in whole frames), which Graph Progress reads; a graph rendered over the whole project gets the project's.

**Inputs and sources.** Each input node of a graph reads a `Source`, matched by its kind (a node reading pictures
gets the source's picture, one reading sound its sound). A Video Input or Audio Input reads what its `source`
setting names (`below` by default); a Named Input reads a Named Output by its `track` and `name`:

- `Below`: what is below the item's graph track, which is the next graph track's output, or the mix under the
  lowest.
- `Mix` (`mix`): the video mix or the audio mix.
- `Track(name)` (`track:<name>`): whatever plays on that track: a video or audio track on its own (its stem),
  before its volume and whether it is in the mix, so a muted track can still drive a graph.
- `Graph(track)` (`graph:<track>`): the main output of a graph track below the item's, where nothing plays on it
  (or the track is muted or left out by a solo) what is below it.

  Both find the track by name in the one track list, whatever its kind: `track:` naming a graph track reads its
  output, `graph:` naming a video or audio track its stem.
- `Named { track, name }` (a Named Input): a **Named Output** of a graph track below the item's. A graph track's
  named outputs are the names its items' graphs offer, each in the layout of the first item that has it; where no
  item plays, or the playing one doesn't offer that name in that layout, it reads zeros. A graph track at or above
  the item's reads zeros, so inputs only point down and there are no loops.
- `Nothing` (empty), or a track that doesn't exist: zeros.

The graph is filled by name: `Video` and `Audio` for inputs reading what is below, and for the others a name
built from their kind and settings (`source_key`, `named_key`), so nodes reading the same thing share one signal and a picture and a sound of
one track never mix. There is no other way in: the CLI's `render <video> <audio> <graph>` is a project too
(`Project::from_files`: the two files on a video and an audio track, and the graph on a graph track for as long as
either file plays).

**No passthrough.** Every graph needs its Output (compiling one without fails). A graph with no Audio Output on the
rendered bus is silent where it plays; nothing is wired through for it, in the app, the export and the CLI alike.

**Mixing.** The renderer runs the nodes once per source frame in order: each track's stem, the mix, then the graph
tracks from the bottom up.

- A **track** reads its media: its picture conformed to the project's grid and size, or its sound in its file's
  own layout. Tracks nothing reads (out of the mix and no item's source) aren't read at all.
- Every track goes over what is below it by **one stacking rule**, its kind's `Stack` (`TrackKind::stacks`,
  applied by `stack_over`): where it plays something it covers what is below, and elsewhere what is below shows
  through. A video track covers the picture (`Picture`), a graph track the picture and the sound (`PictureAndSound`),
  and an audio track adds its sound at its level (`Sum`).
- The **mix** stacks the video and audio tracks in the mix: the picture of the top video track that has one at
  that frame, and the sum of the audio tracks routed to the rendered bus at their volumes, in the mix layout:
  `round(48000 / fps)` samples a frame across the bus's channels. Each block is fitted linearly to the mix layout,
  and a mono track goes to every channel.
- Each **graph track** stacks on what is below it: the output of the item playing at that frame, or what is below
  where none plays. Its sound is fitted to the mix layout. A muted graph track (or one left out by a solo) plays
  nothing, so it passes what is below through, also to graphs reading it. The top graph track's output is the
  master, or the mix with no graph tracks.

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

**Taps, levels and statistics.** `tap` looks in the graphs that ran in the last source step, those of the inspected
graph first, the top graph track's first. `levels`, `costs`, `meters` and `param_levels` come from the first graph
that ran playing the inspected graph, else the first that ran; `node_stats` and `compile_options` describe the
inspected graph's first item, else the first. The tap renderer is built the same way as the render-ahead renderer.

**Errors.** A graph that cannot compile fails the build (`EngineError::Fx`, always, since every graph is a
resource): the message names the graph resource, and the failure carries its id (`Failure::graph`) and the node at
fault.

## Export / Offline Rendering

Export uses the same graph and engine as preview, rendering every frame in order from the start at full resolution
as fast as possible. `RenderSettings::range` writes only some frames (the GUI's loop region); the renderer warms up
before the range as far as the graph's warmup needs, uncapped, or renders from frame 0 when the graph's history is
unbounded, so a range is exact too. `offline::render` reports the frames rendered and the total after each frame to
a `progress` callback, which cancels the render (`EngineError::Cancelled`) by returning `false`.

`export::export` is the one export path, used by the CLI and the GUI:

- `ExportJob::from_project` (or `from_project_with`, leaving out missing files as the preview does) takes the
  composition with every graph in its render form, the tempo and the audio rate. `ExportJob::load` decodes the audio
  each audio track plays (through the audio cache), once per resource, into the render's `AudioResources` and the
  sources of the track mix. The CLI's two routes both build an `ExportJob` from a project.
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
