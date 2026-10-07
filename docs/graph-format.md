# Graph files

Graphs are JSON. Connections are written `"node.port"`; the port can be left out to mean a node's first output
or its main input. Unknown parameters are rejected, which catches typos.

```json
{
  "version": 0,
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

**The format is version 0 until 1.0.** It changes freely: there are no migrations, and a graph or project saved
by another version is rejected with a message. When 1.0 ships the version becomes 1, and from then on each format
change bumps it and adds one step file in `crates/rastersong-graph/src/migrate/` (`v1_to_v2.rs`, ...), run in
order by `GraphDesc::upgrade`; the rename helpers in `migrate/tooling.rs` are ready for them. See
[Decisions](decisions.md#no-migrations-before-10-october-2026).

## Projects

Projects are JSON files with the `.rastersong` extension holding the video, the audio tracks (file, name, offset,
volume, mute), the graph, the tempo and the loop region. Media paths inside the project's folder are saved
relative to it, so a project folder can be moved or shared. Project files have the same version-0 policy. Graphs can also be imported and exported on their own. The project also keeps the Audio Output rate and the
*max warmup frames* limit (omitted from the file while at their defaults).

## Examples

Working examples live in [`examples/graphs/`](../examples/graphs): `am_bands` (the
[Basic Workflow](concepts.md#basic-workflow)), `bass_wave`, `bugged_mosh` and `packed_crush`.
