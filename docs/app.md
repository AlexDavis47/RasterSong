# Desktop app

Two rows: along the bottom, the **Resources** panel and the **timeline** beside it; above them three columns: the
**preview** with its playback controls underneath, the **node graph**, and the **inspector**. The app only holds UI
state; everything is decoded, rendered and cached by the [engine](engine.md) on its render thread.

This describes the app as it is. Changes planned from hands-on testing, among them replacing graph layers with
folder tracks and graphs as FX, are in the [roadmap](roadmap.md#timeline-resources-and-routing).

## Preview

Play/pause, the **Loop** button, timecode and frame, how far ahead is rendered (and the playback speed when
rendering can't keep up), preview resolution (full, ½, ¼, ⅛, 1/16; ½ by default, with the size it renders at shown
beside it) and playback volume. **Unprocessed** shows the original video instead of the processed one (processing
carries on); **Split** shows both side by side with a draggable divider. The scroll wheel zooms the picture,
middle-drag pans it and F fits it again. With no video loaded, the preview offers an Open Video button.

## Node graph

Our own editor, drawn on a pannable, zoomable canvas (see [Decisions](decisions.md#own-node-graph)).

- Scroll wheel zooms around the pointer; middle- or right-drag pans; F frames the whole graph.
- Left-drag on empty space box-selects (Shift adds). Click a node to select it and show it in the inspector;
  Ctrl+ or Shift+click adds or removes it; drag to move the selection. Ctrl+A selects every node, Esc clears the
  selection.
- **Alt+click** a node bypasses it (or the whole selection, if it is selected): a bypassed node passes its main
  input through. **Bypass graph**, on the canvas, skips every node and shows and plays the track mix, to compare
  with the original.
- Drag from a pin to connect. An input takes one connection; a new one replaces the old. Dragging a connected
  input picks its wire up to move it. Dropping a wire on empty space opens the node search, connected.
- Exposed parameters show as diamond pins under a node's inputs; a wire into one modulates that parameter.
  Each node type exposes its main parameters by default (e.g. Delay's time and feedback); the inspector's
  diamond toggles show or hide the others. Hiding a connected parameter disconnects it.
- The project's inputs and output are **linked nodes**: opening a video adds its Video node, adding an audio
  track adds an Audio node named after the track, and removing the track removes it. They're titled after
  what they read (the video's file name, `♪ track`), can't be deleted, copied or added from the search,
  and the graph always has its Output. An Audio node whose track doesn't exist (as in the starter graph
  before any audio is added) is taken over by the first track added.
- Right-click empty space to add a node there: the search box has focus immediately; type, use ↑/↓, and press
  Enter (or click). Right-click a node to copy, duplicate, bypass or delete it; Delete removes the selection,
  Backspace removes it and reconnects the wires around it, Ctrl+D duplicates it.
- **Inspecting a connection.** Resting the pointer on a wire or an output pin shows what it carries at the
  playhead: a meter and the mean, min, max and RMS, with a view above them and the list of views beside. The
  view is a **picture** (the signal stretched over the project's shape, the way Video Output does), a **scope**
  (the waveform), a **spectrum** (an FFT on a logarithmic frequency axis, smoothed between updates) or
  **readings only**, and any signal can be seen any way: audio as a picture, video as a spectrum. Hold **Alt**
  over a connection and scroll to go through the views, one click of the wheel to a step; the popup fades and
  resizes from one view to the next, and the wheel stops zooming meanwhile. The view is remembered separately for
  audio and for other signals (File → Settings). Hold **Shift** to hear the connection through the speakers,
  turning the playback down while the pointer stays on it. It plays along with the transport, so it is silent
  while playback is stopped: sound and picture always match. How often the view asks for a new frame while the playhead moves is the project's **update
  rate** (File → Settings → Inspecting connections). It all reads through the engine's
  [taps](engine.md#taps-and-listening), so it never changes the render. The picture, scope, spectrum and meter are
  shared widgets (`widgets/`) that take plain samples and pixels, so they can be used anywhere.
- Wire thickness follows the RMS level of the signal at the playhead, so modulation is visible: a kick drum
  through a band split shows as the bass wire pulsing.
- Wire colour shows what a wire carries, from the tag the last compile gave each output (before that, from
  the output's tag rule). A wire has a base colour for its kind (video neutral, audio teal) and, if it carries
  part of a signal, a second colour for that part: red, green or blue, or left or right, from Split Channels;
  low, mid or high from Three-Band Split. Effects keep the colours of their main input, so a delay on the red
  channel is still red video; Video to Audio turns it into red audio. **Wires** in File → Settings → Application
  picks how the two show: solid (the part's colour, or the kind's), outlined (the kind's colour outlined in the
  part's), gradient (the part's colour down the centre, fading to the kind's at the edges) or glow (the kind's
  colour as a crisp line, glowing in the part's).
- Compile notes and warnings show as a badge on the node, with the message in its tooltip and in the inspector,
  which also lists what each output carries. Notes (a signal whose tag doesn't suit the node, say) get a quiet
  "i" badge; warnings (something lost or ignored) a yellow "!". Neither stops the render. With several inputs,
  the main input (which sets the output's length and layout) has a ring around its pin.
- When the graph can't render, a bar along the bottom of the graph says why and outlines the node at fault in
  red; clicking the bar shows the node.
- Moving or renaming nodes doesn't re-render; any other edit does.

## Inspector

The node's name (shown on the node instead of its type), its shared settings (Resampling, Grouping, Channels, and
Layout for generators; see
[Node behavior](node-behavior.md#shared-node-settings)) and its parameters, with units, sliders (always linear) reset-to-default buttons, and an `int` toggle on numbers that are not whole-only (which rounds the value and, when a signal modulates it, every sample). Parameters that only make sense whole (Beat division and steps, Pack channels, Chorus voices, Phaser stages, Noise seed, Sample & Hold levels, Resample width and height) are declared `.integer()` in their spec: the slider handle jumps between whole values, typed numbers round, and they have no toggle. Each slider covers the parameter's
usual range; typing (or dragging the value box) past it, up to the node's limits, widens the slider to match. Right-click a slider to set its range; the range is saved with the node in the graph (`"ranges"`), and it is also what a modulating signal is held to. Each
parameter takes two lines: its pin toggle, name and reset button, then the slider and value box. Values left at
their default aren't written to files.

A connected (modulated) parameter shows, in the wire's colour:

- the range the signal moves it over, outlined on the slider with a tick at each end;
- a see-through ghost handle at its live value at the playhead (the middle of the frame, from the last rendered
  frame), while the solid handle stays the base value and can still be dragged;
- a small knob between the slider and the value box for the amount: drag it (Shift for fine), double-click to
  reset, right-click for the modulator's settings: both ways or one way, the amount, and disconnecting the signal.
  **One rule:** the amount is a percentage of the slider's range (the one you set, or the node's usual one), and a
  full-scale signal moves the value that far from where it is: up one way (a negative amount turns it down), either
  way for both ways. The value never leaves the slider's range; widen the slider for more room. The knob, the
  percentage box and the distance box ("Moves by" / "Either side", in the parameter's own unit, such as 200 Hz) are
  three views of the one percentage: all run from −100% to 100% and the knob stops at the ends. Typing a
  distance sets the percentage to distance ÷ range. A new connection starts at 25%, one way.

## Resources

The **Resources** panel, left of the timeline, holds everything the project uses, in two tabs: **Media** and **Graphs**. Both show their resources as a grid of cards, drawn by one shared component (`resource_card`) so media and graphs look, drag and open menus the same way. The Media tab lists the media the project uses. Each resource is one stream of a
linked file (embedding comes later on the [roadmap](roadmap.md#timeline-resources-and-routing)): a video stream
or an audio stream with any number of channels. **Import…** (or File → Import Media…, or dropping files onto the
window) adds files. A file with one video or audio stream becomes a resource at once; a file with more, such as a
video with its sound, opens **Found multiple tracks in this media** with a checkbox per stream (kind, title,
language, codec and shape, all ticked), and each chosen stream becomes its own resource. Cover pictures, subtitles
and data streams aren't listed.

Resources are named after their file (video keeps the extension, audio drops it, and a stream's title is added),
made unique with `_2`, `_3`, …. Nothing reaches the timeline until it is placed: **drag a resource onto the
timeline** to add a track playing it from where it is dropped, or double-click it (or right-click → Add to
Timeline) to add one at the playhead. The track is named after the resource, and an audio track gets its Audio
Input node as before. Right-click a card renames a resource (its tracks keep their names, since graphs select tracks by
name) or removes it; removing a resource that tracks play asks first and removes those tracks with it. A resource
whose file is missing is drawn in red, with its path in the tooltip and a **Relocate…** button under the card, outside the drag area so it always takes the click (also in the
right-click menu, for any resource): pick where the file is now, and the other streams of the same file follow.
Until it is found, the tracks that play it read as gaps and say so in their lanes. The panel checks about once a
second, so a file that comes back (a drive plugged in) is picked up by itself.

**Graphs** are resources too, on their own tab. A project can hold any number; **New graph** adds a
passthrough (the video wired to Video Output, the sound to Audio Output) and opens it. Double-click a graph (or
right-click → Open in editor) to open it in the node editor, which swaps it with the open one. The open graph is
shown in bold. While no graph layer holds an item, the open graph is rendered over the whole timeline; once one
does, only the layers are rendered (an interim rule the [roadmap](roadmap.md#timeline-resources-and-routing)
removes). Right-click also renames, duplicates or removes a graph (the open graph can't be removed). Drag a graph
card onto a [graph layer](#graph-layers) to place it on the timeline. **File → Import Graph…** and **Export
Graph…** read and write a graph as its own file.

## Timeline

A ruler, any number of **video tracks** and **audio tracks** (video above audio), with Reaper-style track headers on
the left. The ruler's corner has the **Time** / **Tempo** button (minutes and seconds, or bars and beats with the
tempo bar and its **Metronome** toggle) and the Snap button. Until a video is loaded, the timeline says it appears
once one is, with a button to add an audio track. Each track
is a lane of **items**, the stretches of its file placed on the timeline. The video header shows the track's name
(its file's name until renamed; the video input reads it by that name), size and frame rate; each audio header has
the track's name (which audio inputs select it by; renaming a track updates them), its volume in the track mix, and
its bus when the project has several. Every header has **mute** and **solo** (*S*: while any track of a kind is
soloed, only soloed tracks of that kind are in the [track mix](engine.md#track-mix-and-output-buses)); audio headers
also have a × that removes the track. Video tracks can't be removed from the timeline yet (a
[roadmap](roadmap.md#timeline-resources-and-routing) bug). Tracks left out of the mix are drawn dimmed. Each track's lane has its own outline (the selected track's is the accent color), so neighbouring tracks are easy to tell apart. The project lasts to the end of its last item and
runs at its [timebase](engine.md#timeline). **+ Track** adds an empty video or audio track to drag a resource
onto, or (Audio file…, also File → Add Audio Tracks…) several audio files at once, each named after its file.
Dropping a resource on a track puts it there from the drop point: an empty track takes it, a track of the same
resource gets another item, and any other track (a track holds one resource only) makes a new track instead.
Opening a video (File → Open Video…) replaces every video track and adds the
video's sound as an audio track too. Both go through [resources](#resources) for the file's best streams; the
Resources panel adds more video tracks, and any stream of a file. Every video track decodes its own thumbnails (two
tracks of one resource share a decoder), so any number of video tracks, including several of the same resource, show
their length and thumbnails. The unprocessed feed (a graph's source side by side with the result) still shows only the
first video track.

Every item has a **header bar** along its top with the track's name and a mute button (a muted item reads as a gap,
for graphs too). Drag the bar to move the item; the area below it shows the item's content, thumbnails of the
source video (decoded by a separate small decoder so they never slow rendering and survive graph edits) or the
audio's waveform, and behaves like empty lane space. Items can't start before the timeline does.

**Editing items:**

- Click an item's bar to select it, Ctrl+click to add it to the selection or take it out; clicking empty lane
  space selects none. **Drag over lane space** (empty, or an item's content) to box-select the items the box
  touches, on any tracks; Ctrl or Shift adds them to the selection. Selected items are outlined. Dragging a
  selected item moves every selected item, and every edit below applies to all of them.
- Drag an item's left or right edge (over its whole height) to **trim** it over its file; it stops at the file's
  ends. **Alt+drag** an edge to change the item's **rate** instead, keeping its in and out points: longer plays
  slower and lower, like tape (from 0.05× to 20×).
- **S** splits at the playhead: the selected items, or with none selected every item under it. Both halves stay
  selected.
- **Delete** (or Backspace) removes the selected items. **Ctrl+C**, **Ctrl+X** and **Ctrl+V** copy, cut and paste
  them; pasted items go back to the tracks they came from, the earliest at the playhead, and become the
  selection. These keys act on the timeline while the pointer is over it, and on the graph otherwise. Copying puts
  a marker on the system clipboard (the platform only sends Ctrl+V when it holds text); copying anything else
  since then means Ctrl+V over the timeline pastes nothing. The bar's right-click menu has the same commands.
- **Snapping** is on by default (the Snap button in the ruler's corner): moves and edges snap to the ruler's
  ticks (beats and bars in tempo mode), the edges of other items, the playhead and the timeline's start, within
  a few pixels. Hold **Shift** to drag freely.
- Every edit undoes as one step.

Right-click a header for its menu: **Link with ▸** lists the other tracks to link this one with, **Unlink** takes it
out of its link, and **Default height** resets its height. Moving, splitting, deleting or copying an item of a
linked track takes the items of the linked tracks that overlap it, and trimming takes the linked edges at the same
time, so a video and its sound stay together. Linked headers say so. (Track links are to be replaced by item
groups; see the [roadmap](roadmap.md#timeline-resources-and-routing).) Drag the bottom edge of a header to change
the track's **height**; the header's menu resets it. The edge's hint also promises a double-click reset, which
doesn't work yet (a roadmap bug). Drag an audio header's free space (not its name, volume or bus) to reorder the
audio tracks; video tracks can't be reordered yet. Frames rendered so far are marked in green along the bottom of the ruler.
Linking, heights, solo and mutes are saved with the project and undo like any edit.

- The scroll wheel zooms time around the pointer, from half the whole project down to a few frames (over the
  headers it scrolls the tracks); middle- or right-drag pans in both directions; F shows the whole project.
- Tick lines run behind the lanes, labelled on the ruler, down to single frames when zoomed in.
- Click or drag on the ruler to seek; lane space selects instead (see above).
- **Loop region** (as in Reaper): Ctrl+drag along the ruler to make one, snapped to whole frames (to the beat grid in
  Tempo mode); Ctrl+drag its edges to
  change it. R or the Loop button by the play button turns looping on and off; right-click the ruler to do the
  same or remove the region. Playing into the region repeats it; playing from after it plays on. While looping,
  rendering ahead wraps from the region's end to its start, so the loop plays without waiting. The region is saved
  with the project.

### Graph layers

Above the tracks, each **graph layer** is a lane of graph items, the top layer first (the project stores them
bottom first). **+ Layer** (beside **+ Track**) adds an empty layer on top. A layer's header has its name (edit it
like a track's), **mute** and **solo** (*S*: while any layer is soloed, only soloed layers render); right-click it
to delete the layer with its items. With no layers the timeline looks as before.

**Drag a graph card** from the Resources panel's Graphs tab onto a layer's lane to place it from the drop point
for the project's length (5 seconds in an empty project). Dropping anywhere else on the timeline (a track, or
empty space) makes a new layer on top first, and the lane under a dragged graph is outlined (or a hint says a layer
will be made). Whatever the new item lands on is trimmed, cut or removed, so items on a layer never overlap.

Graph items are drawn like track items, in their own color, with a **header bar** holding the graph's name and a mute
button (a muted item reads as a gap). They share the track items' header bar, edge handles, selection outline and
snapping (to ticks, item edges of tracks and layers, the playhead and the start; Shift drags freely):

- Click the bar to select the item (selecting a graph item deselects track items and the other way round).
  **Drag the bar** to move it: the item shows where it would land, and the move is made, as one undo step, when you
  let go, trimming what it lands on. It stays on its layer.
- Drag an edge to **trim** it. Graph items are never stretched: there is no rate, so Alt does nothing. Trimming the
  left edge keeps what follows where it is (the item's own start moves), within the neighbours and the graph's
  beginning.
- **S** splits the selected graph item at the playhead (with nothing selected, every track item and graph item under
  it); **Delete** removes it; the bar's right-click menu has Split and Delete. Copy, cut and paste don't cover graph
  items (graph layers are being replaced, see the [roadmap](roadmap.md#timeline-resources-and-routing)).
- **Double-click** the bar to open the item's graph in the node editor.

Selecting a graph item shows its **item inspector** in place of the node inspector (picking another node in the
graph brings the node inspector back). It names the graph and has the **Pre-roll** checkbox (warm the graph up as if
it had run before the item, so trimming the left edge changes nothing after it) and one row per Video Input or
Audio Input node of the graph with a combo box: **Layer below**, every track of the input's kind (video tracks for
video inputs, audio tracks for audio inputs), or **Nothing** (the input reads zeros). A new item binds every input to
*Layer below*. A row bound to a track that no longer exists says so in a note: that input reads nothing until
another is chosen. Bindings belong to the item, so one graph can sit twice reading different tracks. Layers and
items are saved with the project and undo like any edit; how they render is described in
[Graph layers](engine.md#graph-layers).

## Preview audio

Plays the master bus (the first output bus) and follows the playhead: its **track mix**, the audio tracks
routed to it at their volumes, leaving out muted ones (and unsoloed ones while any is soloed). When the rendered
graph has an **Audio Output** writing to the master bus, playback plays its rendered sound instead (track volume and
mute don't apply to it); a track wired straight into that Audio Output plays as it is. With graph layers, the sound
comes from the top layer's item that has such an Audio Output, and frames no item supplies sound for are silent
(see [Graph layers](engine.md#graph-layers)). A bus with more than two channels plays its first two. When playback slows because rendering can't
keep up, the audio is time-stretched (WSOLA: slowed without lowering the pitch) to stay with the picture, and fades
out when playback all but stops. Volume, mute and routing shape the track mix (in playback and the CLI's export),
never what graphs read.

With several buses, each audio track's header shows the bus it is routed to, with a menu to change it.

## Settings

**File → Settings…** (Ctrl+,) opens a window with two pages. **Application** is remembered on this computer:
**Appearance** (theme, wire style, language), the **graph editor** (node latency and warmup display, node
processing time, keeping input connections when duplicating and pasting), the view hovering an audio connection or
any other connection shows, and **Rendering** (the default preview resolution, the cache size in MiB and how many
seconds to render ahead, each with a reset button).
**Project** is saved in the project file: the **picture** (resolution and frame rate, from 23.976 to 60 fps with
NTSC rates as exact fractions; until one is set the fields show the first video's and say so, and the reset button
goes back to following it), tempo (bpm, beats per bar, first beat), *Max warmup frames* (how much
is pre-rendered after a jump in the timeline; the page warns when the graph needs more), the update rate for
inspecting connections, the Audio Output rate and the **output buses** (below). Each setting has a line of help.
The tempo bar in the timeline edits the same tempo fields. The preview resolution and volume are also on the
transport bar, where they are used while playing.

**Output buses** lists the project's buses, master first, each with a name and a channel count (mono, stereo, 3 to
8 channels; 5.1 is six). *+ Add bus* adds one; renaming a bus renames it on its tracks and Audio Outputs too. The
master can't be removed. Removing a bus that tracks are routed to, or that Audio Outputs write to, asks first and
lists them: the tracks move to the master, and the Audio Outputs are left writing to a bus that isn't there (shown
in red in the inspector) and aren't rendered until set to another.

## Keys

Space plays/pauses, ←/→ step one frame, Home jumps to the start, R turns looping on and off, F fits whichever panel
is under the pointer (preview, graph or timeline), Ctrl+S saves, Ctrl+Z undoes, Ctrl+Shift+Z (or Ctrl+Y) redoes. In
the graph, Ctrl+A selects all, Esc clears the selection, Delete removes it and Backspace removes it and reconnects
around it. In the graph, Ctrl+C, Ctrl+X and Ctrl+V copy, cut and paste nodes with
the connections between them; pasted nodes land at the pointer. The **Edit** menu has the same commands. Duplicate
and Paste also keep the node's input connections, unless **File → Settings → Keep input connections when duplicating and pasting** is off. Holding Shift
(Ctrl+Shift+D, Ctrl+Shift+V) does the opposite for one action. Connections to nodes that don't exist in the target
project are skipped.

## Undo

Covers every change to the project: graph edits, node moves, parameters and the timeline. A drag, a slider move or
a typed name is one step. Closing the window, opening a project or starting a new one asks to save unsaved changes
first.

## Projects and settings

Project files are described in [Graph files](graph-format.md#projects).

- **Theme:** File → Settings → Application → Theme picks Dark (the default), Light or Follow system. Every colour the
  app paints itself comes from `crates/rastersong-gui/src/theme.rs`. The theme, preview resolution and volume are
  remembered between sessions.
- **View** shows node latency and warmup, and node performance (processing time), under each node.
- **Help → About RasterSong** credits FFmpeg and its LGPL license and lists the loaded FFmpeg libraries and build
  configuration.
