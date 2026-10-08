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

Projects are JSON files with the `.rastersong` extension holding the timeline, the graphs and graph layers, the
tempo and the loop region:

- `timebase`: the project's `width`, `height` and `frame_rate` (`"30"` or `"30000/1001"`). Left out, the first
  video track's are used, or 1920×1080 at 30 fps without one. See [Timeline](engine.md#timeline).
- `resources`: the media the project uses, as the Resources panel lists them. Each has an `id` (what tracks point
  at), a `name`, a `kind` (`video` or `audio`), the linked file's `path`, and the `stream` it plays, by its index in
  the file (left out: the file's best stream of that kind).
- `tracks`: the track tree, top first, as the timeline lists it. Each track has a `name` (the port name graphs
  read it by, unique among all tracks), its `depth` (0 at the top; a folder holds the deeper tracks right after
  it), the `resource` it plays (video or audio), its `items`, `volume`, `muted`, `solo` and `master_send` (true by
  default) for the track mix, its `bus` (for a top-level track), its `height` on the timeline and its `link` group
  (tracks with the same number move their items together). A `folder` has no resource or items, may be
  `collapsed`, and may take `new_tracks` of a kind (`"video"` or `"audio"`). Left out, `items` is one item
  playing the whole resource from the start. A track with no `resource` is empty (and has no items) until a
  resource is dropped on it. A tree read from a file is repaired: the first track is at depth 0, and none is more
  than one level deeper than a folder above it.
- `graph`, `graph_id`, `graph_name` and `graphs`: the project's graphs. `graph` is the open one (the one the editor
  shows and the engine renders), named `graph_name` with id `graph_id`; `graphs` holds the others, each an `id`,
  a `name` and its `graph`, until one is opened and swaps places with the open graph.
- An item: `position` (seconds into the project), `start` and `end` (its in and out points, in seconds of the
  file; no `end` plays to the end of the file), `rate` (seconds of file per second of timeline, 1 by default) and
  `muted`, its `fx` chain and `pre_roll` (true by default: its FX warm up as if they had run before it).
- `fx` (on a track or folder), `master_fx` (on the project) and an item's `fx`: FX chains, first to last. An FX
  is the `graph` id it runs, `bypass` (false by default) and `receives`: which track fills each input port other
  than `Video` and `Audio`, by port name; a port left out reads zeros. See [Routing](engine.md#routing).
- `loop_region` (`start` and `end` in seconds, and whether it is `enabled`), `tempo` (`bpm`, `beats_per_bar`,
  `offset_secs`), `timeline_mode` (`"time"` or `"tempo"`) and `bypass_graph`.

```json
{ "version": 0,
  "timebase": { "width": 1280, "height": 720, "frame_rate": "30" },
  "resources": [ { "id": 1, "name": "clip.mp4", "kind": "video", "path": "media/clip.mp4", "stream": 0 },
                 { "id": 2, "name": "song", "kind": "audio", "path": "media/song.wav" },
                 { "id": 3, "name": "kick", "kind": "audio", "path": "media/kick.wav" } ],
  "tracks": [ { "name": "clip.mp4", "resource": 1 },
              { "name": "Music", "folder": true, "items": [], "volume": 0.8 },
              { "name": "song", "depth": 1, "resource": 2,
                "items": [ { "position": 1.5, "start": 12.0, "end": 40.0 } ] },
              { "name": "kick", "resource": 3, "bus": "Stems" } ],
  "buses": [ { "name": "Main", "channels": 2 }, { "name": "Stems", "channels": 1 } ],
  "graph": { "version": 0, "nodes": [] } }
```

Resource paths inside the project's folder are saved relative to it, so a project folder can be moved or shared.
A track pointing at a resource the project doesn't have is an error on open.
Project files have the same version-0 policy. Graphs can also be imported and exported on their own. The project
also keeps `audio_rate` (the Audio Output rate), `max_warmup_frames` and `inspect_rate` (how often connection
inspection updates while playing, 5 a second by default), each omitted from the file while at its default.
`buses` lists the output buses, master first (omitted while it is just Main in stereo), and a track's `bus` is the
one it is routed to (omitted for Main). A track's `volume` and `muted` are its level in the track mix.

## Examples

Working examples live in [`examples/graphs/`](../examples/graphs): `am_bands` (the
[Basic Workflow](concepts.md#basic-workflow)), `bass_wave`, `bugged_mosh` and `packed_crush`.
