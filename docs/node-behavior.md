# Node behavior

What the built-in nodes have in common, and the conventions their behavior follows. The reference for every node
(ports, parameters with defaults, ranges and units, modulation) is [`nodes.md`](nodes.md). It is generated from the
node definitions by `cargo xtask docs`, and CI fails if it is out of date.

Roughly: inputs (`video_input`, `audio_input`), the outputs (`output`, `audio_output`), channel structure (`split`,
`combine`, `interleave`, `pack`, `flip`, `transpose`, `resample`, `stretch`), conversion (`to_audio`, `to_video`, `relabel`), generators (`beat`, `constant`, `noise`,
`oscillator`) and many effects (`three_band`, `am`, `delay`, `bitcrush`, `filter`, `compressor`, `gate`,
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
- **`bypass`**: passes the main input straight to the first output, skipping the node (Alt+click in the editor).

Hover text for the Channels setting (generalized to any channel count):

- Label: "How this node treats the channels of an interleaved signal: R, G, B of video, L, R of stereo."
- Together: "Runs R, G, B, R, G, B… (or L, R, L, R…) through the node as one stream. Channels bleed into each other, as in a low pass or a modulated delay."
- Separate: "Runs each channel through its own copy of the node. The same as Split Channels → the node once per channel → Combine Channels."

- **`layout`** (`video` or `audio`, generators only: Beat, Constant, Noise, Oscillator): what the generated signal is
  shaped like. `video` is the video's frame (RGB, rows); `audio` is one block of the audio track named `audio`, or of
  the project's first track, or mono at 48 kHz when the project has no audio. It is a node setting in the file
  (`"layout": "audio"`, written only when not `video`), not a parameter.

## Conditional parameters

Some parameters only mean something for some settings of another: a Beat's *steps* only for the step shape, its
*width* only for the pulse shape, an oscillator's *pulse width* only for the square wave. A spec says so with
`.shown_when("shape", &["step"])`: the rule names one choice parameter of the same node and the options for which
this one is used (the controlling parameter can't itself be conditional, which a registry test checks).

- While the rule fails the inspector hides the parameter. The value is kept and still saves; the node ignores it.
- If a signal is wired to it, or its pin is shown, it stays in the inspector, greyed, with a line saying why it is
  unused ("Unused: only applies when Shape is pulse."). A connection is never hidden from view.
- [nodes.md](nodes.md) adds "Used when `shape` is `pulse`." to the parameter's help.

## Range conversion and the bugged mapping

`to_audio` and `to_video` model writing to and reading from an 8-bit file, so both clip to the range. `accurate`
maps black to -1 and white to 1. `bugged` reproduces the original prototype's glitch: pixels written as signed
8-bit samples (`pixel - 127`) and read back by audio software as unsigned. That flips the sign bit, so the range
wraps at mid-gray (between 8-bit values 126 and 127): black and white sit just either side of silence, and the dark
and bright halves of the image sit at opposite extremes. Effects between the two nodes push samples across that
seam, and they come back as tears where dark turns bright and the reverse. Clipped samples come back mid-gray.
`bugged_mosh` is the prototype workflow: interleave, encode, process, decode, pack.

## Dynamics nodes

The compressor, gate and Dynamic EQ follow their input's level, or the `sidechain` when it's connected. Their times are
milliseconds of the signal's own time, so on a video carrier they span the same fraction of a frame at any
resolution.

## Ports

`?` marks optional inputs. Video In and Audio In are the graph's input ports: each reads the port its `port`
setting names (`Video` and `Audio` are the main ports). Whoever uses the graph fills the ports: in a graph placed on
a graph layer, the item's bindings, by port name, when it compiles (see [Graph layers](engine.md#graph-layers)). A
port nothing fills reads zeros.

## Split and Combine Channels

**Split and Combine Channels** take any number of channels (up to 8). Split has one output per channel of its
input (`c1`, `c2`, …; the editor names them R, G, B or L, R from the input's tag); Combine makes one channel per
input up to the last one connected, so two mono audio signals make stereo and three video channels make RGB.

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
`combine`, `interleave`, `pack`, `flip`, `transpose`, `resample`, `stretch`) changes a signal's shape and leaves its values alone; *Conversion*
(`to_audio`, `to_video`) changes the value range, `0..1` to `-1..1` and back, and `relabel` changes only what a
signal is said to be.
