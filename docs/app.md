# Desktop app

Two rows: along the bottom, the **Resources** panel and the **timeline** beside it; above them three columns: the
**preview** with its playback controls underneath, the **node graph**, and the **inspector**. The app only holds UI
state; everything is decoded, rendered and cached by the [engine](engine.md) on its render thread.

This describes the app as it is. Changes planned from hands-on testing are in the
[roadmap](roadmap.md#timeline-resources-and-routing).

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
  input through. **Bypass graphs**, on the canvas, skips every graph track and shows and plays the mix, to compare
  with the original.
- Drag from a pin to connect. An input takes one connection; a new one replaces the old. Dragging a connected
  input picks its wire up to move it. Dropping a wire on empty space opens the node search, connected.
- Exposed parameters show as diamond pins under a node's inputs; a wire into one modulates that parameter.
  Each node type exposes its main parameters by default (e.g. Delay's time and feedback); the inspector's
  diamond toggles show or hide the others. Hiding a connected parameter disconnects it.
- A graph's inputs are **ports**: **Video In** and **Audio In** nodes, each reading the port its *Port* setting
  names (titled `▣ In: Video`, `♪ In: Audio`, … unless renamed). `Video` and `Audio` are the main ports; any other
  name is a port of its own, and several nodes can read one port. Whoever uses the graph fills the ports: each
  graph item on the timeline picks a source for every port ([Graph tracks](#graph-tracks)), so tracks add no nodes
  and renaming a track leaves the graph alone. Ports are added and deleted like any node. Every graph has one
  **Output**, which can't be deleted or copied; a graph missing one gets one when opened.
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
  red; clicking the bar shows the node. A placed graph that isn't the open one is named by its id in the
  message, and the bar has no node to show.
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

The **Resources** panel, left of the timeline, holds everything the project uses, in two tabs: **Media** and **Graphs**. Both show their resources as a grid of cards that wraps to the panel's width (one row at a time, as many cards as fit), drawn by one shared component (`resource_card`) so media and graphs look, drag and open menus the same way. The Media tab lists the media the project uses. Each resource is one stream of a
linked file (embedding comes later on the [roadmap](roadmap.md#timeline-resources-and-routing)): a video stream
or an audio stream with any number of channels. **Import…** (or File → Import Media…, or dropping files onto the
window) adds files. A file with one video or audio stream becomes a resource at once; a file with more, such as a
video with its sound, opens **Found multiple tracks in this media** with a checkbox per stream (kind, title,
language, codec and shape, all ticked), and each chosen stream becomes its own resource. Cover pictures, subtitles
and data streams aren't listed.

Resources are named after their file (video keeps the extension, audio drops it, and a stream's title is added),
made unique with `_2`, `_3`, …. Nothing reaches the timeline until it is placed: **drag a resource onto the
timeline** to add a track playing it from where it is dropped, or double-click it (or right-click → Add to
Timeline) to add one at the playhead. The track is named after the resource. Right-click a card renames a resource (its tracks keep their names, since
graph sources read tracks by name) or removes it; removing a resource that tracks play asks first and removes those tracks with it. A resource
whose file is missing is drawn in red, with its path in the tooltip and a **Relocate…** button under the card, outside the drag area so it always takes the click (also in the
right-click menu, for any resource): pick where the file is now, and the other streams of the same file follow.
Until it is found, the tracks that play it read as gaps and say so in their lanes. The panel checks about once a
second, so a file that comes back (a drive plugged in) is picked up by itself.

**Graphs** are resources too, on their own tab. A project can hold any number; **New graph** adds the template (the
video wired to Video Output, the sound to Audio Output) and opens it. Double-click a graph (or right-click → Open in
editor) to open it in the node editor, which swaps it with the open one. The open graph is shown in bold. A graph
applies only where it is placed as a **graph item** on a [graph track](#graph-tracks): with none placed, the preview
and the CLI play the plain mixes, and the open graph is only a description to edit. Right-click also renames,
duplicates or removes a graph (the open graph can't be removed; removing one removes its graph items). Drag a graph
card into the Graphs section of the timeline to place it. **File → Import Graph…** and **Export Graph…** read and
write a graph as its own file.

## Timeline

The timeline has three **sections** under a ruler, divided by a thick line: **Graphs** on top, then **Video**, then
**Audio**. Tracks stay in their section and reorder within it. The ruler's corner has the **Time** / **Tempo**
button (minutes and seconds, or bars and beats with the tempo bar and its **Metronome** toggle) and the Snap
button. The timeline shows from launch, over a default ten seconds, with each empty section naming what to drop
in it. Each track is a lane of **items**, the stretches of its file (or, in the Graphs section, graphs) placed on
the timeline.

- **Video tracks layer.** The top video track with an item at a frame wins, as in Premiere or Resolve; the
  **video mix** is that picture.
- **Audio tracks are summed** into their bus at their volumes: the **audio mix**
  ([track mix](engine.md#track-mix-and-output-buses)). While any track of a kind is **soloed** (*S*), only soloed
  tracks of that kind are in the mix. Tracks left out of the mix are drawn dimmed.
- **Every track is also a stem.** A graph can read any track on its own, before its volume, mute and solo.
- **Graph tracks** hold graph items; see [Graph tracks](#graph-tracks).

The video header shows the track's name (its file's name until renamed), size and frame rate; each audio header has
the track's name (renaming a track updates the sources that read it), its volume and, when the project has several
buses, its bus. Every header has **mute** and, for video and audio, **solo**, and a × that removes the track. Each
track's lane has its own outline (the selected track's is the accent color), so neighbouring tracks are easy to
tell apart. The project lasts to the end of its last item and runs at its [timebase](engine.md#timeline). There is
no way to make an empty track: a track appears when a resource is placed (File → Add Audio Tracks… places several
audio files at once, each named after its file).
Dropping a resource on a track puts it there from the drop point: a track of the same resource gets another item,
and any other track (a track holds one resource only) makes a new track instead. Dropped **on the line between two
tracks**, or above the first or below the last track of a section, the resource gets a new track there, whatever
it is dropped on: a thick **ghost line** shows the place. A track's right-click menu has **New track above** and
**New track below**, which list the resources of its kind and put the one chosen on a new track there, at the
playhead.
Click a media card to select it, Ctrl+click (or Shift+click) to add cards to the selection or take them out: dragging any selected card takes them all (stems, say), each onto a track of its own in the panel's order, and the bubble reads "name and N more". A card outside the selection drags alone. Dragging a graph onto the Graphs section works the same as media: dropped on a graph track it goes on that track, on the line between two graph tracks (or below the last one) it gets a new graph track there, anywhere else a new track on top.
**Linked drops.** A resource places **every resource of its file** at the same time, each on its own track of its
kind (the video on a video track, each audio stream on an audio track), grouped so they move together. Hold **Alt**
to place only the one dragged. A video with sound dropped on the timeline therefore arrives with its sound.
While a card is dragged a **drag bubble** (a pill with the card's icon and name) follows the pointer; it pops in,
blooms where it is dropped, and shrinks back to the card when nothing takes it. Over the lanes a **ghost** shows
where the drop will land: an outlined item on the track that takes it, or a ghost row where the new track will
go, at the snapped time (snapping like an item drag, Shift for none). The drop uses that time. A resource no
track plays yet has no known length, so its ghost marks only where it starts. Dragged track headers show the
bubble too.
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
- **S** splits at the playhead: the selected items, or with none selected every item under it. The first half of
  each split item stays selected, so the second half can be dragged on its own.
- **Ctrl+A** selects every item in every section, graph items too, while the pointer is over the timeline.
  Clicking in the graph editor deselects the timeline's items, so the inspector shows the clicked node again.
- **Delete** (or Backspace) removes the selected items. **Ctrl+C**, **Ctrl+X** and **Ctrl+V** copy, cut and paste
  them; pasted items (graph items too) go back to the tracks they came from, the earliest at the playhead, and
  become the selection. These keys act on the timeline while the pointer is over it, and on the graph otherwise. Copying puts
  a marker on the system clipboard (the platform only sends Ctrl+V when it holds text); copying anything else
  since then means Ctrl+V over the timeline pastes nothing. The bar's right-click menu has the same commands.
- **Ctrl+G** (or **Group** in the bar's menu) groups the selected items, and **Ctrl+Shift+G** (**Ungroup**) ungroups
  the groups they are in. Moving, splitting, deleting or copying a grouped item takes the rest of its group, and
  trimming takes the group's edges at the same time, so a video and its sound stay together. A grouped item's bar
  shows 🔗. Splitting a group leaves a group on each side; pasted copies make a new group. Opening a video with
  sound groups the picture's item with its sound's.
- **Snapping** is on by default (the Snap button in the ruler's corner): moves and edges snap to the ruler's
  ticks (beats and bars in tempo mode), the edges of other items, the playhead and the timeline's start, within
  a few pixels. Hold **Shift** to drag freely.
- Every edit undoes as one step.

Right-click a header for its menu: **Default height** resets its height, **New track above** / **New track below**
(see above) add a track and **Remove track** removes it. A video or audio header also shows the track's name, size
or volume, and **mute** and **solo**; a graph track's header has its name and mute. Drag the bottom edge of a
header to change the track's **height**; double-clicking the edge, or the header's menu, resets it. Drag a header
by its free space or its labels (not its name field, buttons, volume or bus) to **reorder** the track **within its
section**; a marker line shows where it goes. Frames rendered so far are marked in green along the bottom of the
ruler. Item groups, heights, solo and mutes are saved with the project and undo like any edit.

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

### Graph tracks

The **Graphs** section holds **graph tracks**, and each holds **graph items**: a graph placed on the timeline,
like an adjustment layer in a video editor. How they render is described in
[Composition](engine.md#composition).

- **Drag a graph card** from the Resources panel's Graphs tab into the Graphs section: onto a graph track it goes
  on that track from the drop point, on the line between two graph tracks (or below the last) it gets a new graph
  track there, anywhere else a new track on top. An empty section says *Drag a graph here to make a graph track*.
- **The topmost graph item playing is the master.** At each frame the picture and sound you see and hear are the
  Video Output and Audio Output of the item on the highest graph track that has one there, or the plain mixes where
  none does. Items on one track never overlap: placing or moving one trims what it lands on. A muted graph track
  or item is skipped, so what is below it shows through. **Bypass graphs** (on the canvas) skips every graph track
  to compare with the original.
- **Items** have the same bar, trim, split, mute, copy, paste and group commands as other items, and **pre-roll**
  (on by default): the graph warms up as if it had run before the item, so trimming the left edge changes nothing
  after it. Dragging a graph item moves it within its own track, applied when you let go; it doesn't move between
  tracks yet. Double-click an item to open its graph in the editor.
- **The graph item inspector.** Selecting a graph item shows one row per input port of its graph, each with a
  source picker: **Below** (the graph playing under this one, or the mix with none; the default for `Video` and
  `Audio`), **Video mix** or **Audio mix**, **Track: name** (a track's stem, read before its volume and mute, so a
  muted kick can still drive an effect), **Graph track: name** (an output of a graph track below this one), or
  **Nothing** (zeros). Other ports start on the track of the same name, if one exists. A source that has gone
  (a removed track, a graph track no longer below) says so and reads nothing until another is picked. Renaming a
  track updates the sources that read it, and renaming a port (an input node's *Port*) keeps the sources its items
  picked for it. Sources only point down, so graphs can't loop.
- A new graph starts as a visible template (`In: Video` into Video Output, `In: Audio` into Audio Output), so a
  fresh graph item changes nothing. The starter graph of a new project (`am_bands`) also wires `In: Audio` to an
  Audio Output, so its sound plays. A graph placed over graphs below it chains them unless a source is changed.

## Preview audio

Plays the master bus (the first output bus) and follows the playhead: its **audio mix**, the audio tracks routed to
it at their volumes, leaving out muted ones (and unsoloed ones while any is soloed). When the topmost graph item
playing has an **Audio Output** writing to the master bus that doesn't just pass the mix through, playback plays
the rendered sound instead, through the same sink the export uses (see [Composition](engine.md#composition)); a
graph with no Audio Output is silent where it plays. A bus with more than two channels plays its first two. When
playback slows because rendering can't keep up, the audio is time-stretched (WSOLA: slowed without lowering the
pitch) to stay with the picture, and fades out when playback all but stops. Volume, mute and solo shape the mix,
never what a graph reads from a track's stem.

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

## Export

**File → Export…** opens the Export window. It renders the project at full resolution on its own thread, while
the app stays usable.

- **Format**: *H.264 (MP4)* with AAC sound, *ProRes (MOV)* with 24-bit PCM, or *FFV1 lossless (MKV)* with 16-bit
  PCM. Only formats whose encoder works on this machine are listed; H.264 needs a GPU or OS encoder (NVENC, AMF,
  VideoToolbox or Media Foundation), and the line under the settings names the one in use.
- **Quality** (H.264: low, medium or high) or **Profile** (ProRes: Proxy, LT, Standard or HQ).
- **Sound**: the bus written, the master by default. Its sound is the graph's Audio Output for it, or else the mix
  of the tracks routed to it.
- **Range**: the whole project, or only the [loop region](#timeline) when there is one (Ctrl+drag along the ruler
  to make it). The region is rendered exactly: the frames before it are rendered first, without being written, as
  far as the graph needs history, or from the start when it has feedback.
- **Size**: the project's, or *Custom*. H.264 and ProRes can only encode even widths and heights; for an odd size
  the window says which side is odd, offers a button for the nearest even size (one pixel more), and won't start
  until the size is even.

*Export…* asks where to save, next to the project and named after it. While it runs the window shows the progress,
the frame and the time left, and *Cancel*, which stops it and removes the unfinished file. Closing the window
doesn't stop an export; opening it again shows the progress. When it ends the window says where the file went, or
why it failed. Tracks whose files are missing are left out, as in the preview.

## Keys

Space plays/pauses, ←/→ step one frame, Home jumps to the start, R turns looping on and off, F fits whichever panel
is under the pointer (preview, graph or timeline), Ctrl+S saves, Ctrl+Z undoes, Ctrl+Shift+Z (or Ctrl+Y) redoes. In
the graph, Ctrl+A selects all, Esc clears the selection, Delete removes it and Backspace removes it and reconnects
around it. In the graph, Ctrl+C, Ctrl+X and Ctrl+V copy, cut and paste nodes with
the connections between them; pasted nodes land at the pointer. The **Edit** menu has the same commands. Duplicate
and Paste also keep the node's input connections, unless **File → Settings → Keep input connections when duplicating and pasting** is off. Holding Shift
(Ctrl+Shift+D, Ctrl+Shift+V) does the opposite for one action. Connections to nodes that don't exist in the target
project are skipped. Over the timeline, S splits, Delete removes the selected items, Ctrl+C, Ctrl+X and Ctrl+V
copy, cut and paste them, and Ctrl+G and Ctrl+Shift+G group and ungroup them (see [Timeline](#timeline)).

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
- **Help → About RasterSong** credits FFmpeg and links the text of the LGPL version the loaded build is under
  (2.1 or later for the release build, 3 for the development build) and lists the loaded FFmpeg libraries and build
  configuration.
