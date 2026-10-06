# Node behavior

What the built-in nodes have in common, and the conventions their behavior follows. The reference for every node
(ports, parameters with defaults, ranges and units, modulation) is [`nodes.md`](nodes.md). It is generated from the
node definitions by `cargo xtask docs`, and CI fails if it is out of date.

Roughly: inputs (`video_input`, `audio_input`), the `output`, channel structure (`split`, `combine`, `interleave`,
`pack`, `stretch`), conversion (`to_audio`, `to_video`, `relabel`), generators (`beat`, `constant`, `noise`,
`oscillator`) and many effects (`three_band`, `am`, `delay`, `bitcrush`, `lowpass`, `filter`, `compressor`, `gate`,
`distortion`, and more).

## Shared node settings

Every node has these shared settings:

- **`interpolation`** (shown as *Resampling*: `hold` or `linear`): how secondary inputs are stretched or shrunk
  to the main input's length. See [Rate matching](signals-and-units.md#rate-matching).
- **`grouping`** (`pixels` or `samples`): how a one-channel secondary input is spread over a main input with
  several channels. With `pixels` (the default) each source sample covers whole pixels, so a mono modulator
  moves a pixel's R, G and B (or L and R) together; with `samples` it is spread over every value. The **Stretch
  to Match** node does the same stretch on purpose, so the stretched signal itself can go on.
- **`channels`** (`together` or `separate`, effects only): with `separate`, an interleaved signal (RGB, stereo,
  any channel count) is split into its channels, each processed by its own copy of the node (with its own state,
  and identical settings), and recombined. Exactly equivalent to Split Channels → one node per channel → Combine
  Channels, without the wiring. Modulation inputs are shared by all channels. Units stay the same: a row is a
  row of the picture either way. On a node that can't run per channel it falls back to `together` with a warning;
  on a one-channel signal it simply has no effect (a note).

Hover text for the Channels setting (generalized to any channel count):

- Label: "How this node treats the channels of an interleaved signal: R, G, B of video, L, R of stereo."
- Together: "Runs R, G, B, R, G, B… (or L, R, L, R…) through the node as one stream. Channels bleed into each other, as in a low pass or a modulated delay."
- Separate: "Runs each channel through its own copy of the node. The same as Split Channels → the node once per channel → Combine Channels."

*Planned:* the generator `layout` parameter joins this list; see the [roadmap](roadmap.md#node-settings-and-parameters).

## Conditional parameters

*Planned.* Some parameters only mean something for some settings of another (a Beat node's *steps* only in step
mode, an oscillator's *pulse width* only for the pulse wave). Today they are always shown. The
[roadmap](roadmap.md#node-settings-and-parameters) adds a declarative "shown when" rule on `ParamSpec` so the
inspector (and generated docs) hide them.

## Range conversion and the bugged mapping

`to_audio` and `to_video` model writing to and reading from an 8-bit file, so both clip to the range. `accurate`
maps black to -1 and white to 1. `bugged` reproduces the original prototype's glitch: pixels written as signed
8-bit samples (`pixel - 127`) and read back by audio software as unsigned. That flips the sign bit, so the range
wraps at mid-gray (between 8-bit values 126 and 127): black and white sit just either side of silence, and the dark
and bright halves of the image sit at opposite extremes. Effects between the two nodes push samples across that
seam, and they come back as tears where dark turns bright and the reverse. Clipped samples come back mid-gray.
`bugged_mosh` is the prototype workflow: interleave, encode, process, decode, pack.

## Dynamics nodes

The compressor and gate follow their input's level, or the `sidechain` when it's connected. Their times are
milliseconds of the signal's own time, so on a video carrier they span the same fraction of a frame at any
resolution.

## Ports

`?` marks optional inputs. Audio inputs read the project's audio track named by `source`; a track that doesn't
exist reads as silence.

## Split and Combine Channels

**Split and Combine Channels** take any number of channels (up to 8). Split has one output per channel of its
input (`c1`, `c2`, …; the editor names them R, G, B or L, R from the input's tag); Combine makes one channel per
input up to the last one connected, so two mono audio signals make stereo and three video channels make RGB.
Graphs written for the RGB-only versions (ports `r`, `g`, `b`) load with their ports renamed.

## Channels and interleaving

An RGB signal *is* the interleaved stream R, G, B, R, G, B, …, and effects process it sample by sample. That is
[Approach 1](concepts.md#core-concept): a low pass on RGB bleeds each channel into the next, and a *modulated*
delay on RGB resamples the stream, scrambling channels into rainbow noise. For clean spatial effects such as
bass-driven waves, `split` first and process each channel ([Approach 2](concepts.md#core-concept)).
`interleave` and `pack` don't change any samples; they relabel RGB (or any channel count) as one 3×-wide mono
carrier and back. The difference shows in rate matching: a mono modulator moves a pixel's R, G and B together on an
RGB signal, but varies across them on the packed carrier.

## Categories

Categories stay as they are (see [Decisions](decisions.md#categories-stay-as-they-are)): *Channels* (`split`,
`combine`, `interleave`, `pack`, `stretch`) changes a signal's shape and leaves its values alone; *Conversion*
(`to_audio`, `to_video`) changes the value range, `0..1` to `-1..1` and back, and `relabel` changes only what a
signal is said to be.
