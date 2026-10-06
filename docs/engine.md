# Render engine

The engine turns a compiled graph and a media source into rendered frames, in the background, into a cache. The
GUI only talks to the engine; it never decodes or schedules anything itself.

## Crate Layout

```
crates/
  rastersong-media    FFmpeg I/O only: probing, frame index, video/audio decode, encode
  rastersong-graph    Pure Rust, no FFmpeg: Signal type, Node trait, scheduler, built-in nodes
  rastersong-engine   Render service: sequential renderer, warmup, frame cache, cancellation, playback clock
  rastersong-cli      Headless file-in → file-out renderer (testing, benchmarking, batch use)
  rastersong-gui      egui application: viewer, node editor, parameters, timeline
xtask/                Developer tasks (fetch FFmpeg, generate test fixtures, packaging)
```

Dependency direction: `gui → engine → (media, graph)`. `media` and `graph` never depend on each other.

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

Nodes with infinite memory (feedback, IIR) declare a practical warmup length (the time to decay or settle to
about 0.1%, currently capped at 120 frames by `MAX_WARMUP_FRAMES` in `nodes/support.rs`). Preview after a seek is
therefore exact for finite-memory nodes and a close approximation for infinite-memory ones. Requests that continue
forward from where the renderer already is, within the warmup length, skip the reset and just keep rendering.
**Export always renders from the first frame** (or later, from a saved state snapshot), so export is exact.

> **Planned:** the cap becomes a **project setting** (*Max warmup frames*), and it limits **only the background
> pre-render** after a seek. It must never shorten a node's real memory: a delay's buffer, a feedback tail or a
> filter's state keep their true length whatever the cap is. Today the same constant clamps what nodes report; the
> work is to separate "how long the effect really is" from "how much of it we are willing to pre-render". See the
> [roadmap](roadmap.md#project-and-settings).

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

## Audio Output

A graph can have one **Audio Output** node (optional; add it from the node menu). Its sound replaces the source
audio in the preview and the export. Without one, with nothing connected to it, or with the graph bypassed, the
source audio is used untouched; a track wired straight into it is also used as it is, not re-rendered.

- **Any signal goes in.** One sample per pixel is mono, two are stereo; anything else (a picture, say) is written
  as interleaved samples to a stereo track, with a note. Nothing is converted on the way in: wire a
  video through Video to Audio first if you want its range mapped to `-1..1`.
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
