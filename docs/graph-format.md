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
type's), `"description"` (the user's note on what the node does, shown under it) and `"exposed"` (which parameter
pins show, when that differs from the type's defaults). None of these affect rendering. A graph may carry a
`"description"` of its own, shown when it is offered as a [preset](app.md#presets).

**Comments and notes** are nodes of type `"comment"` and `"note"`, with no ports or parameters; they are taken out
before a graph is compiled, so they never affect rendering. A comment's title is its `"label"`, its text its
`"description"`, and `"size": [width, height]` is its box (its `"position"` is the box's top-left corner; the text
sits above it). A note's text is its `"description"`; `"attached": "node_id"` attaches it to a node, which it
then moves with and points at:

```json
{ "id": "c", "type": "comment", "label": "Listen to the bass", "description": "The lows, then their level.",
  "position": [-30, -30], "size": [420, 300] },
{ "id": "n", "type": "note", "description": "Lower Low / mid to follow only the kick.", "attached": "bands",
  "position": [190, -200] }
``` These do: `"bypass": true` passes the main input straight to the first output, skipping the
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

Projects are JSON files with the `.rastersong` extension holding the timeline, its resources (media and graphs),
the tempo and the loop region:

- `timebase`: the project's `width`, `height` and `frame_rate` (`"30"` or `"30000/1001"`). Left out, the first
  video track's are used, or 1920×1080 at 30 fps without one. See [Timeline](engine.md#timeline).
- `resources`: the media and graphs the project uses, as the Resources panel lists them. Each has an `id` (what
  items point at), a `name` (unique among resources of every kind), its label `color` (`"red"`,
  `"orange"`, `"yellow"`, `"green"`, `"teal"`, `"blue"`, `"violet"` or `"pink"`; left out, its items are drawn in
  their track's colour) and its `content`, whose `kind` says what it is:
  - `"video"` or `"audio"`: one stream of a linked file, with the file's `path` and the `stream` it plays, by its
    index in the file (left out: the file's best stream of that kind);
  - `"graph"`: a graph, with its `graph` in the format above.

  Which graph the editor shows isn't saved: a project opens on its first graph, and gets a passthrough graph when
  it has none.
- `tracks`: the Graphs, Video and Audio sections, in that order, each top first, as the timeline lists them. Each
  track has a `name` (what graph sources read it by, unique among all tracks), its `kind` (`graph`, `video` or
  `audio`), its `items`, `volume` (audio only), `muted` and `solo` for the mix, its `bus` (audio only) and its
  `height` on the timeline. A track with no `items` is empty. A muted graph track is skipped: what is below it
  shows through.
- An item: the `resource` it plays (one of the track's kind), `position` (seconds into the project), `start` and
  `end` (its in and out points, in seconds of the resource; no `end` plays to the end of the file, and a graph
  item always has one, as a graph has no end of its own), `rate` (seconds of file per second of timeline, 1 by
  default; always 1 for a graph), `muted`, its `group` (items with the same number move, trim, split and delete
  together; a group of one is dropped on load) and, for a graph, `pre_roll` (true by default: the graph warms up
  as if it had run before the item). A track's items can play different resources of its kind. Items on a
  track never overlap: a file with overlaps is put in order and repaired on load, where their lengths are known
  without reading the files (a graph item's always are, a file's item without an `end` isn't).

  What a graph reads is set on its own input nodes, not on its item: a `video_input` or `audio_input` node's
  `source` is `"below"` (the default), `"mix"`, `"track:<name>"` (a track's stem), `"graph:<graph track>"` (the
  main output of that graph track, which must be below the item's) or empty (nothing); a `named_input` node's
  `track` and `name` name a `named_output` of that graph track's graphs. See [Composition](engine.md#composition).
- `loop_region` (`start` and `end` in seconds, and whether it is `enabled`), `tempo` (`bpm`, `beats_per_bar`,
  `offset_secs`), and `timeline_mode` (`"time"` or `"tempo"`).

```json
{ "version": 0,
  "timebase": { "width": 1280, "height": 720, "frame_rate": "30" },
  "resources": [
    { "id": 1, "name": "clip.mp4", "content": { "kind": "video", "path": "media/clip.mp4", "stream": 0 } },
    { "id": 2, "name": "song", "content": { "kind": "audio", "path": "media/song.wav" } },
    { "id": 3, "name": "kick", "color": "teal", "content": { "kind": "audio", "path": "media/kick.wav" } },
    { "id": 4, "name": "Graph", "content": { "kind": "graph", "graph": { "version": 0, "nodes": [] } } } ],
  "tracks": [ { "name": "Graphs 1", "kind": "graph",
                "items": [ { "resource": 4, "position": 0.0, "end": 20.0 } ] },
              { "name": "clip.mp4", "kind": "video", "items": [ { "resource": 1, "position": 0.0 } ] },
              { "name": "song", "kind": "audio", "volume": 0.8,
                "items": [ { "resource": 2, "position": 1.5, "start": 12.0, "end": 40.0 } ] },
              { "name": "kick", "kind": "audio", "bus": "Stems",
                "items": [ { "resource": 3, "position": 0.0 } ] } ],
  "buses": [ { "name": "Main", "channels": 2 }, { "name": "Stems", "channels": 1 } ] }
```

Resource paths inside the project's folder are saved relative to it, so a project folder can be moved or shared.
An item pointing at a resource the project doesn't have, or at one of another kind than its track's, is an error
on open. Every graph resource is brought up to date on open, as a graph file is (see Versioning).
Project files have the same version-0 policy. Graphs can also be imported and exported on their own. The project
also keeps `audio_rate` (the Audio Output rate), `max_warmup_frames` and `inspect_rate` (how often connection
inspection updates while playing, 5 a second by default), each omitted from the file while at its default.
`buses` lists the output buses, master first (omitted while it is just Main in stereo), and a track's `bus` is the
one it is routed to (omitted for Main). A track's `volume` and `muted` are its level in the mix.

## Examples

Working examples live in [`examples/graphs/`](../examples/graphs): `am_bands` (the
[Basic Workflow](concepts.md#basic-workflow)), `bass_wave`, `bugged_mosh` and `packed_crush`.
