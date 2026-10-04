# RasterSong Plan

RasterSong is a unique video editing tool that allows video and audio to merge,
creating interesting or glitchy video effects that are directly tied to audio.

> **Status (October 2026):** RasterSong is being rebuilt from the ground up. The November 2025 prototype
> (FFmpeg decoding + egui viewer) is preserved under the `prototype-2025` tag for reference only and is not
> being built upon. This document is the source of truth for the new architecture.

## Core Concept

Video files are typically made up of frames, which are made up of pixels.
Each pixel is typically three or four color channels, with 8 bits per channel allowing 256 values for each channel.

Similarly, audio files are made up of samples, values that represent the amplitude of the audio at a given time.
Typically these files are sampled at 44.1kHz (44,100 samples per second), and 16 bits per sample allowing 65,536 amplitude values.

RasterSong works by taking a video file and unraveling its frames pixel by pixel into sequences of values.
Each channel is just a sequence of values, and we interpret them as audio samples.

To keep things simple, we'll call the converted video signal the "carrier" and any additional audio signals the "modulators".
At this point, all of our data is being treated as audio.

We want to lay the carrier and the modulators on top of each other.
There are two approaches to handling color channels:

**Approach 1: Sequential Packing**
Concatenate all color channels into a single carrier stream (e.g., R, G, B, R, G, B, ...).
This results in one carrier signal with all channel data interleaved.

**Approach 2: Separate Carriers**
Keep each color channel as a separate carrier signal. This gives us three carriers and one modulator (the original audio):

- Red carrier (each pixel's red channel value)
- Green carrier (each pixel's green channel value)
- Blue carrier (each pixel's blue channel value)
- Modulator (the audio track)

Each approach produces vastly different visual effects. RasterSong is graph-based, so both are available:
the video input provides an RGB signal, and explicit **Split** / **Combine** / **Interleave** nodes let the user
choose how channels are separated, packed, and in which order.

### Synchronization Challenge

The modulator was recorded at 44.1kHz, meaning for every real time second there are **44,100** values.

But our video is made up of frames. Say 30fps, 240×180 pixels, 8-bit color depth, and no alpha channel:

**Approach 1: Sequential Packing**
For every real time second: 30 × (240 × 180) × 3 = **3,888,000** values.

- Ratio: 3,888,000 ÷ 44,100 ≈ **88×** more samples per second than the modulator

**Approach 2: Separate Carriers**
For every real time second, each carrier has: 30 × (240 × 180) = **1,296,000** values.

- Ratio: 1,296,000 ÷ 44,100 ≈ **29.4×** more samples per second per carrier

**Performance challenge:** a preview needs to process millions of values per second, and the count scales with
resolution (1080p30 RGB is ~187M values per second). RasterSong does not need to be strictly real time; it renders
ahead into a cache, and reduced-resolution previews cut the cost dramatically (see [Preview Resolution](#preview-resolution)).

## How It Works

Imagine you're making a music video. You've got your video, and you've got your audio. Wouldn't it be cool if the song _itself_ could interact with your visuals?

### Basic Workflow

1. **Import your video file** - It becomes an RGB input node in the effects graph
2. **Import your music file** - It becomes your modulator input node
3. **Split the carrier** - A Split node turns the RGB signal into red, green, and blue signals
4. **Split the modulator** - A three-band splitter outputs:
   - Bass track
   - Mids track
   - Treble track
5. **Apply amplitude modulation** - Create an AM node and connect:
   - Red carrier → carrier input
   - Bass track → modulator input
   - Repeat for green (mids) and blue (treble)
6. **Combine** - A Combine node rebuilds the RGB signal and feeds the Output node

### What Happens

When amplitude modulation is applied, the amplitude of each carrier is modulated by its corresponding frequency band:

- **Positive waveform** → Carrier amplitude increases
- **Negative waveform** → Carrier amplitude decreases

The brightness of each color channel now follows its frequency band. When a kick drum hits, the red channel will begin waving around!

### Advanced Effects

Using utility and effect nodes, you can create a variety of effects:

- **Delay node** - Offsets each line of video slightly from the next, creating a wave effect that follows your bass notes. A rising bass note causes the waving to morph and change rates.
- **Bit crush node** - Reduces the bit depth of the carrier, creating a sort of posterization effect that can be tied to a modulator.
- **Low pass filter node** - Smooths the carrier signal, creating a sort of blur effect that can be tied to a modulator.

### What Makes It Unique

This isn't the same as having a video processing effect automated by amplitude. The coolest part is that you're seeing the **literal audio interacting with every single pixel** in the video at the most base level.

While there are many programs that can make interesting visuals reacting to audio, RasterSong has a sort of sentimental value to it, and as far as I know, the visual effects produced are unique to itself.

## Design Principles

1. **Correctness is testable.** Every layer can be tested without the layers above it, and most of the system can be tested without FFmpeg at all.
2. **The graph never sees media formats, and the media layer never sees the graph.** Frames go in, frames come out.
3. **Users never see samples.** Every user-facing parameter is in normalized units (frames, rows, fractions of a row), so results look the same at any preview resolution.
4. **Simple over clever.** One signal type, one processing order (sequential), one cache rule.
5. **CPU only.** No GPU compute. Real time is a goal for preview, not a requirement.
6. **Nothing for the user to install.** All media dependencies ship with the app.

## System Architecture

### Crate Layout

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
The GUI only talks to the engine; it never decodes or schedules anything itself.

### Units & Timing

The timeline is **frame-based**: frames are identified by integer index, never by floating-point seconds.
Seconds only exist at the edges (display, audio alignment).

**Block = exactly one frame.** Every signal in the graph is processed one frame-sized block at a time.
Because every signal is aligned frame-to-frame, signals with different sample counts (a carrier with 43,200
samples per frame and a modulator with 1,470) can never drift out of sync.

**User-facing units** are normalized and converted to samples internally by the engine:

| Unit | Meaning | Example use |
|---|---|---|
| Frames | One full frame of the signal (block length) | Delay by 1 frame, feedback length |
| Rows | One row of the signal (`width × samples_per_pixel`) | Delay by 1 row for wave effects |
| Fraction of a row | Horizontal offsets | Shift by 0.1 row |
| Cycles per row | Frequency of filters and oscillators | Low pass cutoff |
| Hz | Frequencies of audio-domain nodes, relative to the input signal's own sample rate | Three-band crossovers |

All of these can be fractional. Because none of them are in samples, a half-resolution preview looks like a
scaled-down version of the full render rather than a different effect.

### Signals

There is **one runtime data type**: `Signal`, a block of `f32` values for one frame, plus small descriptive metadata:

```rust
struct Signal {
    data: Vec<f32>,
    layout: Layout,
}

struct Layout {
    width: u32,
    height: u32,
    samples_per_pixel: u32, // 1 for a single channel, 3 for interleaved RGB
}
```

Video signals are nominally in `0.0..=1.0` (black to full intensity) and audio signals in `-1.0..=1.0`.
Nothing clamps values between nodes; only the output clamps to `0..=1` when converting back to 8-bit.

Audio modulators are also `Signal`s: one frame's worth of mono audio. Each frame's block always has the same
length, `round(sample_rate / frame_rate)` samples, resampled from exactly that frame's span of time. So even when
the frame rate doesn't divide the sample rate (48 kHz at 29.97 fps is 1,601.6 samples per frame), or the video has
a variable frame rate, the modulator stays locked to the picture.

**Effect nodes don't care about layout.** An effect sees a flat `&[f32]` and passes the layout through unchanged.
Whether it receives one color channel or a whole interleaved RGB stream, it processes it the same way. Layout
is only used by:

- **Structural nodes** (Split, Combine, Interleave, Pack, Output), which need to know how to take a signal apart and rebuild it
- **Unit conversion**, e.g. "1 row" is `width` samples for one channel but `3 × width` for interleaved RGB. Nodes ask the engine (`ctx.samples_per_row()`) rather than reasoning about layout themselves.

Ports are untyped: any output can connect to any input. Conversions are always explicit nodes the user places,
in the spirit of Substance Designer.

### Rate Matching

Every node has one **main input** (usually the carrier). The main input defines the node's output length and layout.
All other inputs (modulators, parameter inputs) are resampled by the engine to the main input's length before the
node runs, one frame block at a time.

Every node has an **interpolation** setting for this resampling, available on all nodes:

- **Hold** (default): each source sample is repeated. A mono modulator stretched across interleaved RGB affects R, G and B of one pixel equally.
- **Linear**: smooth ramps between source samples. Ramps stop at the block edge rather than reading into the next frame.
- More modes (e.g. smoothed) can be added later.

Sample positions are centre-aligned, so a block covers exactly the same span at any length. Unconnected optional
inputs receive silence (zeros).

**Different-resolution video inputs** are scaled in 2D to the project resolution by the input node.
1-D resampling is never used to reconcile two images, since it would skew rows.

### Node Contract

```rust
pub trait Node: Send {
    /// Input ports; the first is the main input. Source nodes have none.
    fn inputs(&self) -> &'static [InputSpec] { &[] }
    fn outputs(&self) -> &'static [&'static str] { &["out"] }

    /// For source nodes, the name of the host-supplied signal they read ("video", "audio").
    fn source(&self) -> Option<&str> { None }

    /// Output layouts for the given input layouts, or an error if the inputs don't fit.
    /// Defaults to passing the main input's layout through.
    fn output_layouts(&self, ctx: &LayoutContext) -> Result<Vec<Layout>, String>;

    /// Called once the graph is compiled and layouts are known. Allocate buffers here.
    fn prepare(&mut self, ctx: &PrepareContext) {}

    /// Process exactly one frame. Inputs are already rate-matched; outputs are pre-sized.
    fn process(&mut self, ctx: &ProcessContext, inputs: &[&Signal], outputs: &mut [Signal]);

    /// Clear all internal state, as if no frame had ever been processed.
    fn reset(&mut self) {}

    /// Samples of delay this node adds (non-zero for nodes that need lookahead).
    fn latency(&self, ctx: &PrepareContext) -> usize { 0 }

    /// How many frames of history this node needs before its output is valid (stateful nodes).
    fn warmup_frames(&self, ctx: &PrepareContext) -> u32 { 0 }
}
```

Rules every node must satisfy (enforced by tests, see [Testing Strategy](#testing-strategy)):

- **Deterministic:** the same inputs after `reset()` always give the same outputs.
- **Block-size independent:** a stateful node keeps its own history (e.g. a ring buffer). It never re-reads
  past samples from upstream, and never processes the same sample twice.
- **No allocation in `process`.**

Each node type is registered with a `NodeSpec`: a label, a category, a one-line description and a list of
`ParamSpec`s (name, label, help text, and a number range, choice list or text default). Constructors read their
parameters through those specs, so defaults and ranges live in one place, and the editor builds its parameter
panels from the same specs.

The graph compiler validates the graph (unknown nodes, ports or parameters, missing inputs, cycles, exactly one
output, layout mismatches), drops nodes that don't feed the output, orders the rest, and computes latency
compensation. Each frame, it runs the nodes in that order with no allocation.

### Graph Files

Graphs are JSON. Connections are written `"node.port"`; the port can be left out to mean a node's first output
or its main input. Unknown parameters are rejected, which catches typos.

```json
{
  "version": 1,
  "nodes": [
    { "id": "video", "type": "video_input" },
    { "id": "audio", "type": "audio_input" },
    { "id": "wave", "type": "delay", "params": { "time": 1, "depth": 1.5 }, "interpolation": "linear" },
    { "id": "out", "type": "output" }
  ],
  "connections": [
    { "from": "video", "to": "wave" },
    { "from": "audio", "to": "wave.modulation" },
    { "from": "wave", "to": "out" }
  ]
}
```

Nodes may also carry `"position": [x, y]` (their place in the editor) and `"label"` (a name shown instead of
the node type's). Neither affects rendering.

Working examples live in [`examples/graphs/`](../examples/graphs): `am_bands` (the [Basic Workflow](#basic-workflow)),
`bass_wave`, `bugged_mosh` and `packed_crush`.

### Built-in Nodes

| Type | Inputs → Outputs | Parameters (default) |
|---|---|---|
| `video_input` | → `out` (RGB, `0..1`) | `source` (`"video"`) |
| `audio_input` | → `out` (mono audio block) | `source` (`"audio"`) |
| `output` | `in` (RGB or mono at project size) → | |
| `split` | `in` (RGB) → `r`, `g`, `b` | |
| `combine` | `r`, `g`, `b` (mono, same size) → `out` (RGB) | |
| `interleave` | `in` (RGB) → `out` (mono, 3× wide) | |
| `pack` | `in` (mono, width divisible by 3) → `out` (RGB) | |
| `to_audio` | `in` (`0..1`) → `out` (`-1..1`) | `mapping` (`accurate` or `bugged`) |
| `to_video` | `in` (`-1..1`) → `out` (`0..1`) | `mapping` (`accurate` or `bugged`) |
| `three_band` | `in` → `low`, `mid`, `high` | `low_hz` (250), `high_hz` (4000) |
| `am` | `carrier`, `modulator` → `out` | `depth` (1): `carrier × (1 + depth × modulator)` |
| `delay` | `in`, `modulation`? → `out` | `time` (1), `depth` (0), `unit` (`rows` or `frames`), `feedback` (0), `mix` (1) |
| `bitcrush` | `in`, `modulation`? → `out` | `bits` (4), `depth` (0, bits per unit of modulation) |
| `lowpass` | `in`, `modulation`? → `out` | `cutoff` (40 cycles per row), `depth` (0, octaves per unit of modulation) |

**Range conversion and the bugged mapping.** `to_audio` and `to_video` model writing to and reading from an 8-bit
file, so both clip to the range. `accurate` maps black to -1 and white to 1. `bugged` reproduces the original
prototype's glitch: pixels written as signed 8-bit samples (`pixel - 127`) and read back by audio software as
unsigned. That flips the sign bit, so the range wraps at mid-gray (between 8-bit values 126 and 127): black and
white sit just either side of silence, and the dark and bright halves of the image sit at opposite extremes.
Effects between the two nodes push samples across that seam, and they come back as tears where dark turns bright
and the reverse. Clipped samples come back mid-gray. `bugged_mosh` is the prototype workflow: interleave, encode,
process, decode, pack.

`?` marks optional inputs. Audio inputs read the project's audio track named by `source`; a track that doesn't
exist reads as silence.

Every node also has two shared settings:

- **`interpolation`** (shown as *Resampling*: `hold` or `linear`): how secondary inputs are stretched or shrunk
  to the main input's length.
- **`channels`** (`together` or `separate`, effects only): with `separate`, an RGB signal is split into R, G and
  B, each processed by its own copy of the node (with its own state), and recombined. Exactly equivalent to
  Split → three nodes → Combine, without the wiring. Modulation inputs are shared by all three channels.

**Channels and interleaving.** An RGB signal *is* the interleaved stream R, G, B, R, G, B, …, and effects process
it sample by sample. That is [Approach 1](#core-concept): a low pass on RGB bleeds each channel into the next, and a
*modulated* delay on RGB resamples the stream, scrambling channels into rainbow noise. For clean spatial effects
such as bass-driven waves, `split` first and process each channel ([Approach 2](#core-concept)). `interleave` and
`pack` don't change any samples; they relabel RGB as one 3×-wide mono carrier and back. The difference shows
in rate matching: a mono modulator moves a pixel's R, G and B together on an RGB signal, but varies across them on
the packed carrier.

### Render Engine

The graph is compiled into a **topologically sorted schedule** and rendered **sequentially**, one frame at a time,
in the style of an audio DAW. There is no pull-based random access inside the graph.

**Lookahead as latency.** A node that needs future samples (e.g. a symmetric blur) reports it as latency and
delays its own output. The engine sums latency along each path and inserts compensating delays on shorter branches
so that signals meet in sync at Combine and other multi-input nodes (like plugin delay compensation in a DAW).
The engine pre-rolls enough frames to cover total latency.

**Seeking and warmup.** The graph reports `K = max(warmup_frames)` over all nodes. When rendering starts at frame N:

1. `reset()` all nodes
2. Render frames `N-K .. N-1` and discard the output
3. Render from frame N onward, keeping the output

Nodes with infinite memory (feedback, IIR) declare a practical warmup length (the time to decay or settle to
about 0.1%, capped at 120 frames); letting the user adjust it per node is planned. Preview after a seek is therefore
exact for finite-memory nodes and a close approximation for infinite-memory ones. Requests that continue forward
from where the renderer already is, within the warmup length, skip the reset and just keep rendering.
**Export always renders from the first frame** (or later, from a saved state snapshot), so export is exact.

**Always rendering ahead.** A background render thread continuously renders forward from the playhead into the
frame cache while the app is open, whether playback is running or not. It fills the first missing frame in a window
ahead of the playhead: 10 seconds by default, limited to ¾ of the cache budget (1 GiB by default) so some frames
behind the playhead survive for scrubbing back. When the cache is over budget, the frame farthest from the playhead
is evicted, with frames behind it counting as twice as far.

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

### Dynamic Playback

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

### Preview Resolution

The user can choose a preview scale (e.g. full, ½, ¼). Processing cost scales with pixel count, so ¼ scale is
roughly 16× cheaper. Because all parameters are in normalized units, a reduced-resolution preview is a faithful
rough version of the full-resolution render. Export always renders at full project resolution.

### Media Layer

FFmpeg is used for decoding and encoding, and it lives entirely inside `rastersong-media` behind a small trait.
No FFmpeg type appears in any other crate. Tests for the engine and graph use a fake backend that produces
synthetic frames.

- **Open/probe:** builds a frame index from **packets only** (no full decode), sorted by presentation time.
  Frames that can't be decoded from a clean start are left out of the index: packets before the first keyframe,
  the leading frames of an open GOP at the start of a cut stream, and packets the demuxer flags as corrupt
  (e.g. the cut-off end of a truncated file).
- **API by frame index:** `frame(i) -> Arc<VideoFrame>` (packed 8-bit RGB). Callers never see GOPs, keyframes
  or timestamps. Requesting the same frame twice is free. `frame_time(i)` gives each frame's presentation time,
  which is how variable-frame-rate sources are placed on the timeline.
- **Sequential fast path:** the decoder keeps going forward when it is already at or before the keyframe a seek
  would land on. It only seeks on a jump.
- **Seeking:** each frame records which keyframe decoding must start from. For the leading B-frames of an open GOP
  this is the *previous* keyframe, since their references are in the previous GOP. Demuxers seek imprecisely, so
  every seek is verified: if decoding would start after the needed keyframe, it retries by decode timestamp
  (MPEG-TS seeks by DTS), then at earlier keyframes, and finally reopens the file. The source remembers which
  kind of seek works. The decoder is drained at end of stream so trailing frames are never lost.
- **Conversion:** one reused scaler per source using FFmpeg's `sws_scale_frame`, which picks the color matrix
  and range from each frame. Output is at the requested size (project/preview resolution). Display-matrix
  rotation (phone video) is applied so frames come out upright.
- **Audio:** decoded once on load, in full, to interleaved `f32` via swresample, optionally resampled and remixed.
  Encoder priming samples are trimmed using the container's edit list. Audio is small enough that this is the
  simplest correct approach.
- **Latest-wins requests:** a video source is a synchronous object. Stale work is dropped by the engine's render
  thread (see [Cancellation](#render-engine)).

### Desktop App

Two rows: the **timeline** along the bottom; above it three columns: the **preview** with its playback controls
underneath, the **node graph**, and the **inspector**. The app only holds UI state; everything is decoded,
rendered and cached by the engine on its render thread.

- **Preview controls:** play/pause, timecode and frame, how far ahead is rendered (and the playback speed when
  rendering can't keep up), preview resolution (full, ½, ¼; ½ by default), the selected audio track's offset, and
  playback volume.
- **Node graph** (our own editor, drawn on a pannable, zoomable canvas):
  - Scroll wheel zooms around the pointer; middle- or right-drag pans; F frames the whole graph.
  - Left-drag on empty space box-selects (Shift adds). Click a node to select it and show it in the inspector;
    drag to move the selection.
  - Drag from a pin to connect. An input takes one connection; a new one replaces the old. Dragging a connected
    input picks its wire up to move it. Dropping a wire on empty space opens the node search, connected.
  - Right-click empty space to add a node there: the search box has focus immediately; type, use ↑/↓, and press
    Enter (or click). Right-click a node to duplicate or delete it; Delete removes the selection, Ctrl+D
    duplicates it.
  - Wire thickness follows the RMS level of the signal at the playhead, so modulation is visible: a kick drum
    through a band split shows as the bass wire pulsing.
  - When the graph can't render, a bar along the bottom of the graph says why and outlines the node at fault in
    red; clicking the bar shows the node.
  - Moving or renaming nodes doesn't re-render; any other edit does.
- **Inspector:** the node's name (shown on the node instead of its type), its shared settings (Resampling,
  Channels) and its parameters, with units, sliders (logarithmic for wide ranges like cutoff) and
  reset-to-default buttons. Audio inputs pick their track from a list. Values left at their default aren't
  written to files.
- **Timeline:** a ruler, the video track with rendered frames marked in green, and any number of **audio tracks**,
  each with a name (which audio inputs select it by; renaming a track updates them), mute and remove. Click or drag
  to seek; drag a track's block to move it against the video.
- **Preview audio** mixes the unmuted tracks and follows the playhead. When playback slows because rendering can't
  keep up, the audio is time-stretched (WSOLA: slowed without lowering the pitch) to stay with the picture, and
  fades out when playback all but stops. Volume and mute only affect playback, never rendering.
- **Keys:** Space plays/pauses, ←/→ step one frame, Home jumps to the start, Ctrl+S saves.
- **Projects** are JSON files with the `.rastersong` extension holding the video, the audio tracks (file, name,
  offset, volume, mute) and the graph. Media paths inside the project's folder are saved relative to it, so a
  project folder can be moved or shared. Version 1 projects (one audio file) are upgraded on load. Graphs can also
  be imported and exported on their own.
- **About** credits FFmpeg and its LGPL license and lists the loaded FFmpeg libraries and build configuration.

### Export / Offline Rendering

Export uses the same graph and engine as preview, rendering every frame in order from the start at full resolution
as fast as possible, with progress and estimated time remaining.

Initial export formats:

- **H.264 MP4** for sharing, encoded through OS/hardware encoders (Media Foundation on Windows, VideoToolbox on macOS, NVENC/QSV/AMF where available)
- **ProRes** and **FFV1** for high-quality / lossless output, using FFmpeg's built-in encoders

### Performance

- CPU only. Many of the most interesting effects (IIR filters, feedback, delays) are inherently sequential across the entire carrier stream, so they can't be spread across GPU threads anyway.
- SIMD for per-sample operations.
- Parallelism across independent graph branches and channels (`rayon`), not within a sequential stream.
- Reusable buffer pool; no allocation during `process`.
- Reduced-resolution preview is the main lever for heavy graphs.

## Testing Strategy

Testing is a first-class part of the project. Every phase has tests that must pass before the next phase depends on it.

| Layer | What it proves | Tooling |
|---|---|---|
| Node property tests | Processing in blocks of any size gives the same output as one large block; `reset()` + re-render is identical; no NaN/inf for valid inputs | `proptest` |
| Engine | Seek with warmup matches a render from frame 0 (exact for finite-memory nodes, within tolerance otherwise); graph edits and cancellation never serve a stale frame; output is deterministic; latency compensation aligns branches | fake media backend, no FFmpeg |
| Media correctness | Random-access decode of frame *i* is byte-identical to sequential decode of frame *i*, for every fixture. This works for any codec without hand-made expected outputs | generated fixtures |
| Fixtures | Small generated clips: B-frames, open GOP, variable frame rate, odd dimensions, rotation metadata, frame index encoded into lossless frames, audio-only, video-only, truncated files | `cargo xtask fixtures` (uses the `ffmpeg` CLI) |
| End to end | CLI renders the example graphs; selected frames compared to golden images in `crates/rastersong-cli/tests/golden/` with tolerances for cross-platform floating-point differences. After an intended change, inspect the new frames and update them with `RASTERSONG_BLESS=1 cargo test -p rastersong-cli --test golden` | snapshot tests |
| Performance | Samples/sec per node, decode fps, full-graph fps, recorded in [benchmarks.md](benchmarks.md). Automated regression checks in CI are planned; shared CI runners are too noisy for tight thresholds | `criterion` |
| GUI | Graph ↔ editor conversion and timeline math are unit tested; a few headless interaction tests drive the real UI on the fake backend. `cargo test -p rastersong-gui --test screenshots -- --ignored` renders the UI offscreen (needs a GPU) to `target/tmp/screenshots/` for checking layout changes | `egui_kittest` |
| Robustness (later) | Malformed media never crashes the app | `cargo-fuzz` |

CI runs `fmt`, `clippy`, tests and `cargo-deny` on Windows, macOS and Linux.

## Dependencies, Licensing & Distribution

> This section reflects our understanding of the licenses involved and is not legal advice.
> Have the RasterSong license and FFmpeg compliance reviewed before any paid release.

### RasterSong License

RasterSong is **source-available**, not open source. The code is public, so it can be read, studied, and built for
personal use, and the project stays attributable to its author. Redistribution and commercial use are not permitted
without permission. This keeps the option of selling prebuilt binaries later (similar to Aseprite's model).
The terms are in [LICENSE](../LICENSE). It is a custom license and should be reviewed (see [Open Questions](#open-questions)).

Because the code isn't open source, outside contributions will require a contributor license agreement (CLA).

### FFmpeg

- Use an **LGPL** build of FFmpeg only. Never configure with `--enable-gpl` (this rules out x264/x265) or `--enable-nonfree`.
- **Dynamically link** FFmpeg and ship its shared libraries alongside the app. Static linking under the LGPL would require providing relinkable object files.
- LGPL obligations we meet:
  - Ship FFmpeg as separate, replaceable shared libraries
  - Credit FFmpeg and include the LGPL license text in the app (About / licenses screen) and installer
  - Publish the exact FFmpeg source version and configure line used for each release
  - Don't restrict users from replacing the FFmpeg libraries or reverse engineering for that purpose
- Users never download or install FFmpeg themselves. Downloading FFmpeg at runtime was considered and rejected: it doesn't help with licensing and hurts reliability (offline use, firewalls, antivirus, version drift).

### Codec Patents

Codec patents are separate from copyright licenses. H.264 export goes through OS and hardware encoders, which are
licensed by the platform vendor. ProRes and FFV1 export use FFmpeg's own LGPL encoders.

### Other Dependencies

All Rust dependencies must have permissive licenses (MIT, Apache-2.0, BSD, Zlib, MPL-2.0 or similar).
`cargo-deny` enforces this in CI and also checks security advisories.

### Developer Setup

- One pinned FFmpeg version (currently **9.0.2**), matching the major version of the `ffmpeg-next` bindings (9.x). The pins live in `xtask/src/ffmpeg.rs`.
- `cargo xtask fetch-ffmpeg` installs it into `third_party/ffmpeg/`:
  - **Windows, Linux:** downloads a pinned LGPL shared build from [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) and verifies its SHA-256. Only month-end autobuilds are pinned, because those are kept long-term.
  - **macOS:** no trustworthy prebuilt LGPL shared build exists, so it downloads the official source release, verifies it and builds it (several minutes, once).
- `.cargo/config.toml` points `FFMPEG_DIR` at `third_party/ffmpeg/`, and `rastersong-media`'s build script copies the shared libraries next to the binaries and tests Cargo builds, so `cargo run` and `cargo test` need no PATH changes.
- Release builds use FFmpeg built from source in CI, with a minimal LGPL configure line (broad demuxers and decoders, only the encoders we ship).
- The BtbN development builds are configured with `--enable-version3`, which makes them LGPL **v3**. Release builds should leave it off (LGPL v2.1 or later) unless a v3-only component is needed.
- Tests in `rastersong-media` fail if the loaded FFmpeg reports any non-LGPL license, was configured with `--enable-gpl` or `--enable-nonfree`, or doesn't match the major version the bindings were built against.

Getting started:

```sh
cargo xtask fetch-ffmpeg   # once, and again whenever the pin changes
cargo xtask fixtures       # generate media test fixtures into fixtures/
cargo test --workspace
cargo run -p rastersong-gui

# Render a video through a graph, modulated by an audio file, to a lossless .mkv (with the
# audio as its soundtrack) or to a directory of PNG frames
cargo run --release -p rastersong-cli -- render video.mp4 song.wav examples/graphs/am_bands.json out.mkv
cargo run --release -p rastersong-cli -- render video.mp4 song.wav graph.json frames/ --size 320x180 --frames 60
cargo run --release -p rastersong-cli -- render video.mp4 song.wav graph.json out.mkv --audio-offset -1.5

# Open the app on a project or a video
cargo run --release -p rastersong-gui -- my-project.rastersong
```

Prerequisites: the Rust toolchain (pinned by `rust-toolchain.toml`) and **libclang**, which the FFmpeg bindings
use to generate their headers. On Windows, install LLVM (`winget install LLVM.LLVM`) or set `LIBCLANG_PATH`;
Linux and macOS usually have it already (`apt install libclang-dev` otherwise). On macOS, building FFmpeg also needs
the Xcode command line tools.

### Packaging

Using `cargo-packager` or `cargo-dist`:

- **Windows:** installer, FFmpeg DLLs next to the executable
- **macOS:** `.app` bundle, FFmpeg libraries in `Frameworks/` via rpath
- **Linux:** AppImage

## Roadmap

Each phase ends with its tests passing in CI.

**Phase 0: Foundation** (done)
- Tag the prototype as `prototype-2025`, start a fresh workspace with the crate layout above
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

**Phase 4: GUI**
- egui viewer driven only by the engine
- Node editor and parameter panels
- Minimal timeline: one video track, one audio track, with an offset
- Buffer health and playback speed indicators

**Phase 5: Export & Polish**
- Export (H.264 via OS/hardware encoders, ProRes, FFV1) with progress
- State snapshots for faster seeking with stateful graphs
- SIMD and parallelism work guided by benchmarks
- Packaging and installers

**Decisions waiting on the GUI** (from Phase 2, to settle by trying them in the app)
- ~~Effects on RGB signals scramble channels when modulated~~: effects now have the `channels` setting.
- What `interleave` and `pack` should mean (currently a relabel between RGB and a 3×-wide mono carrier)
- Value ranges (video `0..1`, audio `-1..1`) and mono-only modulators
- Undo/redo in the editor; asking before closing with unsaved changes

**Later**
- Multi-clip timeline
- More nodes and interpolation modes
- Fuzzing

## Open Questions

- **License review:** the custom LICENSE is a first draft. Have it reviewed, or switch to an established source-available license (e.g. PolyForm Strict), before any paid release.
- **Snapshots:** memory budget and spacing for state snapshots.
- **Hardware encoder fallback:** what to offer for H.264 export on machines with no usable OS/hardware encoder.
