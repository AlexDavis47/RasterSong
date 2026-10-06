# Graph files

Graphs are JSON. Connections are written `"node.port"`; the port can be left out to mean a node's first output
or its main input. Unknown parameters are rejected, which catches typos.

```json
{
  "version": 4,
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
**percentage of the parameter's span** (its usual range; in octaves for frequencies): one way, 100% moves the
value across the whole span; both ways (`"mode": "bipolar"`), 100% is the swing from the lowest point to the
highest. The value stays between the slider's ends (widened to include the base value) unless the entry says
`"overshoot": true`, which allows it up to the parameter's limits. Without an entry the amount is 25%, one way,
not overshooting. How modulation is applied is in
[Node authoring](node-authoring.md#parameter-modulation).

Format version 3 introduced percentage amounts and version 4 the range limit. Older graphs (amounts in the
parameter's own unit, the default amount that depended on the base value, and values that could pass the slider's
ends) are rewritten on load, with `"overshoot": true`, so they move parameters exactly as before.

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
