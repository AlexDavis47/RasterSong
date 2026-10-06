# Graph files

Graphs are JSON. Connections are written `"node.port"`; the port can be left out to mean a node's first output
or its main input. Unknown parameters are rejected, which catches typos.

```json
{
  "version": 1,
  "nodes": [
    { "id": "video", "type": "video_input" },
    { "id": "audio", "type": "audio_input" },
    { "id": "wave", "type": "delay", "params": { "time": 1 }, "modulation": { "time": { "amount": 1.5 } },
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
`"modulation": { "time": { "amount": 0.5, "mode": "unipolar" } }` on the node saying how far; without an entry,
the amount is half the base value (or a tenth of the usual range if the base is 0), bipolar. How modulation is
applied is in [Node authoring](node-authoring.md#parameter-modulation).

Renamed nodes, ports, parameters and options are upgraded on load by `migrate.rs` and `GraphDesc::upgrade`, so old
files keep loading. **Any roadmap change that renames or reshapes a parameter must add a migration** (for example
removing `mix`, splitting nodes, or changing modulation amounts to percentages).

## Projects

Projects are JSON files with the `.rastersong` extension holding the video, the audio tracks (file, name, offset,
volume, mute), the graph, the tempo and the loop region. Media paths inside the project's folder are saved
relative to it, so a project folder can be moved or shared. Version 1 projects (one audio file) are upgraded on
load. Graphs can also be imported and exported on their own. *Planned:* project-level settings such as max warmup
frames (see the [roadmap](roadmap.md#project-and-settings)).

## Examples

Working examples live in [`examples/graphs/`](../examples/graphs): `am_bands` (the
[Basic Workflow](concepts.md#basic-workflow)), `bass_wave`, `bugged_mosh` and `packed_crush`.
