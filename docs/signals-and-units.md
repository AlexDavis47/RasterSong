# Signals & units

How data flows through a graph, and what the numbers a user types mean. For the idea behind it see
[Concepts](concepts.md); for how a frame is scheduled see the [render engine](engine.md).

## Units & Timing

The timeline is **frame-based**: frames are identified by integer index, never by floating-point seconds.
Seconds only exist at the edges (display, audio alignment).

**Block = exactly one frame.** Every signal in the graph is processed one frame-sized block at a time.
Because every signal is aligned frame-to-frame, signals with different sample counts (a carrier with 43,200
samples per frame and a modulator with 1,470) can never drift out of sync.

**User-facing units** are converted to samples internally by the engine. Rows, frames, pixels, time and beats are
resolution-independent, so a half-resolution preview looks like a scaled-down version of the full render. There is
one list of units for both times and frequencies; a time multiplies by the unit's sample count, a frequency
(labelled **Cycles per**) divides by it:

| Unit | Meaning | Example use |
|---|---|---|
| pixel | One pixel of the **project's** resolution; a downscaled preview scales it with the image (`3 × scale` samples for RGB video, one frame of channels for audio) | Delay by 3 pixels, a cutoff in cycles per pixel |
| sample | One literal sample of the render in front of you; it means something different at another preview scale | Low-level experiments |
| row | One row of the signal (`width × samples_per_pixel`) | Delay by 1 row for wave effects |
| frame | One full frame of the signal (block length) | Delay by 1 frame, feedback length |
| ms | Milliseconds of the signal's own clock | Gate hold, reverb pre-delay |
| second | Seconds of the signal's own clock; as a frequency, Hertz | Three-band crossovers |
| beat, bar | Musical time at the project tempo | Compressor release of half a beat; a wobble on every beat |

All of these can be fractional.

Every node that takes a time or a frequency has one `unit` parameter built with `Unit::time_param` or
`Unit::freq_param` (`nodes/support.rs`; the second is labelled "Cycles per"). A node calls `unit.samples(ctx)` to
multiply or `unit.per_sample(value, ctx)` to divide. The `pixel` unit scales with the preview through
`CompileOptions::pixel_scale`, which the renderer sets to render width over project width. Option names that
used to differ between the two lists (`rows`, `Row`, `Hertz`, …) are upgraded by `migrate.rs`, so old graph
files keep loading.

### Tempo

The project stores a constant tempo (`Tempo`: BPM, beats per bar, and the seconds from the start of
the video to the first beat). It reaches the graph through `CompileOptions` and `PrepareContext`
(`samples_per_beat()`, `samples_per_bar()`, `beat_offset_samples()`), so changing the tempo recompiles the graph.
The timeline ruler can show Time or Tempo (bars and beats; the project's `timeline_mode`), switched with a button
in the ruler header; the tempo controls only appear in Tempo mode, and loop edges snap to the visible beat grid in
bars mode. The `beat` generator outputs phase, decay, pulse or step signals locked to the grid, and an oscillator
in Beat or Bar units starts its cycle on the first beat. Both are position-based, so seeking is exact. Tempo
changes over time (a tempo map) are not supported yet; `Tempo` is a struct so one can replace it.

## Signals

There is **one runtime data type**: `Signal`, a block of `f32` values for one frame, plus small descriptive metadata:

```rust
struct Signal {
    data: Vec<f32>,
    layout: Layout,
}

struct Layout {
    width: u32,
    height: u32,
    samples_per_pixel: u32, // 1 for one channel, 2 for interleaved stereo, 3 for interleaved RGB
    tag: Tag,               // what the samples are meant to be (advisory)
}
```

Video signals are nominally in `0.0..=1.0` (black to full intensity) and audio signals in `-1.0..=1.0`.
Nothing clamps values between nodes; only the output clamps to `0..=1` when converting back to 8-bit.

Audio modulators are also `Signal`s: one frame's worth of audio, with the track's channels interleaved as decoded
(L, R, L, R, … for stereo, `samples_per_pixel = 2`). Nothing is downmixed: a graph that wants mono sums or splits
the channels itself. Each frame's block always has the same length, `round(sample_rate / frame_rate)` samples per
channel, resampled from exactly that frame's span of time. So even when the frame rate doesn't divide the sample
rate (48 kHz at 29.97 fps is 1,601.6 samples per frame), or the video has a variable frame rate, the modulator
stays locked to the picture.

**Effect nodes don't care about layout.** An effect sees a flat `&[f32]` and passes the layout through unchanged.
Whether it receives one color channel or a whole interleaved RGB stream, it processes it the same way. Layout
is only used by:

- **Structural nodes** (Split Channels, Combine Channels, Interleave, Pack, Stretch to Match, Output), which need to know how to take a signal apart and rebuild it
- **Unit conversion**, e.g. "1 row" is `width` samples for one channel but `3 × width` for interleaved RGB. Nodes ask the engine (`ctx.samples_per_row()`) rather than reasoning about layout themselves.

Ports are untyped: any output can connect to any input. Conversions are always explicit nodes the user places,
in the spirit of Substance Designer.

> Do not confuse a signal's `Layout` with the `layout` *parameter* some generator nodes (Beat, Constant, Noise,
> Oscillator) have, which picks the shape of the signal they generate (for example "take the layout from the audio
> source"). The roadmap proposes moving that to a shared node setting; see
> [Generator layout](roadmap.md#node-settings-and-parameters).

### Tags

Tags are advisory. Each `Layout` carries a `Tag`, worked out when the graph compiles: the signal's **kind**
(video, audio or unknown), what its **channels** are (mono, stereo, RGB, or just numbered), which **part** of a
whole it is (one channel, one frequency band) and its nominal **range** (`0..1`, `-1..1` or unknown). Tags colour
wires and produce compile **notes**, such as a gate fed video being told it's tuned for `-1..1` and that levels
will act differently. Notes are suggestions, not mistakes: using a signal as something it wasn't made as is often
the effect. **Warnings** are kept for things that are lost or can't be honoured (Split dropping channels past 8,
Separate channels on a node that can't run per channel). Neither ever stops processing and never convert anything:
RGB into a stereo effect just runs as interleaved samples. Each output port has a tag rule (`OutputSpec::tag`):
most effects pass their main input's tag on, conversions set the kind and range, and nodes that set a range from
their parameters (generators, Clamp, Remap, Offset) work it out themselves. Level-based nodes (Gate, Compressor,
Limiter, Distortion) declare the range they're designed for (`NodeSpec::expects`). **Relabel** rewrites a tag on
purpose, without touching the samples.

## Rate Matching

Every node has one **main input** (usually the carrier). The main input defines the node's output length and layout.
All other inputs (modulators, parameter inputs) are resampled by the engine to the main input's length before the
node runs, one frame block at a time.

Every node has an **interpolation** setting for this resampling, available on all nodes:

- **Hold** (default): each source sample is repeated.
- **Linear**: smooth ramps between source samples. Ramps stop at the block edge rather than reading into the next frame.
- More modes (e.g. smoothed) can be added later.

Sample positions are centre-aligned, so a block covers exactly the same span at any length. Unconnected optional
inputs receive silence (zeros).

**Different-resolution video inputs** are scaled in 2D to the project resolution by the input node.
1-D resampling is never used to reconcile two images, since it would skew rows.

The other shared settings that shape rate matching (`grouping`, `channels`) are in
[Node behavior](node-behavior.md#shared-node-settings).
