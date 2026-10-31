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
affect rendering. These do: `"bypass": true` passes the main input straight to the first output, skipping the
node; `"integer"` lists number parameters rounded to whole values (the value and every modulated sample); and the
shared settings `"interpolation"`, `"grouping"`, `"channels"` and `"layout"` (see
[Node behavior](node-behavior.md#shared-node-settings)), each written only when not at its default. A signal connected to a parameter (`{ "from": "audio", "to": "wave.@time" }`) modulates it, with
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

Projects are JSON files with the `.rastersong` extension holding the timeline, the graphs, the
tempo and the loop region:

- `timebase`: the project's `width`, `height` and `frame_rate` (`"30"` or `"30000/1001"`). Left out, the first
  video track's are used, or 1920×1080 at 30 fps without one. See [Timeline](engine.md#timeline).
- `resources`: the media the project uses, as the Resources panel lists them. Each has an `id` (what tracks point
  at), a `name`, a `kind` (`video` or `audio`), the linked file's `path`, and the `stream` it plays, by its index in
  the file (left out: the file's best stream of that kind), and its label `color` (`"red"`, `"orange"`, `"yellow"`,
  `"green"`, `"teal"`, `"blue"`, `"violet"` or `"pink"`; left out, its items are drawn in their track's colour).
- `tracks`: the Video and Audio sections, video tracks first, each section top first, as the timeline lists them.
  Each track has a `name` (what graph sources read it by, unique among all tracks), its `kind` (`video` or
  `audio`), the `resource` it plays, its `items`, `volume`, `muted` and `solo` for the mix, its `bus` (audio only)
  and its `height` on the timeline. Left out, `items` is one item playing the whole resource from the start. A
  track with no `resource` is empty (and has no items) until a resource is dropped on it.
- `graph_tracks`: the Graphs section, top first. Each has a `name` (unique among all tracks), `items`, `muted` and
  `height`. A graph item has the `graph` id it runs, `position` and `length` (seconds on the timeline), `start`
  (seconds into the graph's own time at its left edge), `muted`, `pre_roll` (true by default: the graph warms up as
  if it had run before the item). What the graph reads is set on its own input nodes, not on the item: a
  `video_source` or `audio_source` node's `source` is `"mix"`, `"track:<name>"` (a track's stem),
  `"graph:<graph track>"` (an output of that graph track, which must be below the item's) or empty (nothing), and
  its `output` names a `named_output` of the graph track's graphs (empty: the main output of its kind). Items on
  one track never overlap; a file with overlaps is repaired on load. See [Composition](engine.md#composition).
- `graph`, `graph_id`, `graph_name` and `graphs`: the project's graphs. `graph` is the open one (the one the editor
  shows), named `graph_name` with id `graph_id`; `graphs` holds the others, each an `id`, a `name` and its
  `graph`, until one is opened and swaps places with the open graph.
- An item on a video or audio track: `position` (seconds into the project), `start` and `end` (its in and out
  points, in seconds of the file; no `end` plays to the end of the file), `rate` (seconds of file per second of
  timeline, 1 by default), `muted`, its `group` (items with the same number move, trim, split and delete
  together; a group of one is dropped on load).
- `loop_region` (`start` and `end` in seconds, and whether it is `enabled`), `tempo` (`bpm`, `beats_per_bar`,
  `offset_secs`), and `timeline_mode` (`"time"` or `"tempo"`).

```json
{ "version": 0,
  "timebase": { "width": 1280, "height": 720, "frame_rate": "30" },
  "resources": [ { "id": 1, "name": "clip.mp4", "kind": "video", "path": "media/clip.mp4", "stream": 0 },
                 { "id": 2, "name": "song", "kind": "audio", "path": "media/song.wav" },
                 { "id": 3, "name": "kick", "kind": "audio", "path": "media/kick.wav" } ],
  "tracks": [ { "name": "clip.mp4", "kind": "video", "resource": 1 },
              { "name": "song", "kind": "audio", "resource": 2,
                "items": [ { "position": 1.5, "start": 12.0, "end": 40.0 } ], "volume": 0.8 },
              { "name": "kick", "kind": "audio", "resource": 3, "bus": "Stems" } ],
  "graph_tracks": [ { "name": "Graphs 1",
                      "items": [ { "graph": 1, "position": 0.0, "length": 20.0,
                                   "sources": { "Video": "below", "Audio": "below", "kick": { "track": "kick" } } } ] } ],
  "buses": [ { "name": "Main", "channels": 2 }, { "name": "Stems", "channels": 1 } ],
  "graph": { "version": 0, "nodes": [] } }
```

Resource paths inside the project's folder are saved relative to it, so a project folder can be moved or shared.
A track pointing at a resource the project doesn't have is an error on open.
Project files have the same version-0 policy. Graphs can also be imported and exported on their own. The project
also keeps `audio_rate` (the Audio Output rate), `max_warmup_frames` and `inspect_rate` (how often connection
inspection updates while playing, 5 a second by default), each omitted from the file while at its default.
`buses` lists the output buses, master first (omitted while it is just Main in stereo), and a track's `bus` is the
one it is routed to (omitted for Main). A track's `volume` and `muted` are its level in the mix.

## Examples

Working examples live in [`examples/graphs/`](../examples/graphs): `am_bands` (the
[Basic Workflow](concepts.md#basic-workflow)), `bass_wave`, `bugged_mosh` and `packed_crush`.
