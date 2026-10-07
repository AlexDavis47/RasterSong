# Graph files

Graphs are JSON. Connections are written `"node.port"`; the port can be left out to mean a node's first output
or its main input. Unknown parameters are rejected, which catches typos.

```json
{
  "version": 12,
  "nodes": [
    { "id": "video", "type": "video_input" },
    { "id": "audio", "type": "audio_input" },
    { "id": "wave", "type": "delay", "params": { "time": 1 }, "modulation": { "time": { "amount": 25 } },
      "interpolation": "linear" },
    { "id": "out", "type": "output" }
  ],
  "connections": [
    { "from": "video", "to": "wave" },
    { "from": "audio", "to": "wave.@time" },
    { "from": "wave", "to": "out" }
  ]
}
```

Nodes may also carry `"position": [x, y]` (their place in the editor), `"label"` (a name shown instead of the node
type's) and `"exposed"` (which parameter pins show, when that differs from the type's defaults). None of these
affect rendering. A signal connected to a parameter (`{ "from": "audio", "to": "wave.@time" }`) modulates it, with
`"modulation": { "time": { "amount": 25, "mode": "unipolar" } }` on the node saying how far. The amount is a
**percentage of the slider's range** (the node's `"ranges": { "param": [min, max] }` entry when the user set one,
otherwise the node type's usual range), from −100 to 100. A full-scale signal moves the value that far from where it
is: one way (`"mode": "unipolar"`) up by |signal| (down for a negative amount), both ways (`"mode": "bipolar"`,
the default) by the signal either way. The value never leaves the slider's range (widened to include the base value,
and within the parameter's limits). Without an entry the amount is 25%, one way. How modulation is applied is in
[Node authoring](node-authoring.md#parameter-modulation).

Format version 3 introduced percentage amounts, version 4 the range limit and version 5 narrower usual ranges for
some frequencies, version 6 linear frequency modulation (octave amounts are converted at the base value) version 7 the generator `layout` setting, version 8 the `ranges` a user sets on sliders and version 9 the single
modulation rule above (both-ways amounts used to be peak to peak, and an `"overshoot"` flag let values pass the
slider; the flag is read from old files and never written). Older graphs are rewritten on load so they move
parameters as before: both-ways amounts are halved, and a modulation that overshot gets its slider range widened to
where it reached, within the limits. An amount that came to more than the whole range is held to 100%. Version 10
made every `mix` start at 1; Reverb, Phaser, Flanger and Chorus, which started lower, get their old mix written out
when an older graph that never set it is loaded. Version 11 removed the Low Pass node: an old one loads as a Filter
set to a 6 dB/oct slope, the same one-pole filter with the same cutoff, unit and modulation. Version 12 split Transpose out of Flip and removed the oscillator's Ramp wave: a
Flip in transpose mode loads as a Transpose node, and a ramp as a saw with the amplitude (and its slider range and
modulation amount) negated.

Renamed nodes, ports, parameters and options are upgraded on load by `migrate.rs` and `GraphDesc::upgrade`, so old
files keep loading. **Any roadmap change that renames or reshapes a parameter must add a migration** (for example
removing `mix` or splitting nodes; the move to percentage modulation amounts is the worked example in
`migrate.rs`).

## Projects

Projects are JSON files with the `.rastersong` extension holding the video, the audio tracks (file, name, offset,
volume, mute), the graph, the tempo and the loop region. Media paths inside the project's folder are saved
relative to it, so a project folder can be moved or shared. Version 1 projects (one audio file) are upgraded on
load. Graphs can also be imported and exported on their own. The project also keeps the Audio Output rate and the
*max warmup frames* limit (omitted from the file while at their defaults).

## Examples

Working examples live in [`examples/graphs/`](../examples/graphs): `am_bands` (the
[Basic Workflow](concepts.md#basic-workflow)), `bass_wave`, `bugged_mosh` and `packed_crush`.
