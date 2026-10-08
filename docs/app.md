# Desktop app

Two rows: the **timeline** along the bottom; above it three columns: the **preview** with its playback controls
underneath, the **node graph**, and the **inspector**. The app only holds UI state; everything is decoded,
rendered and cached by the [engine](engine.md) on its render thread.

This describes the app as it is. Changes planned from hands-on testing (Look/Listen tools, settings page, tooltips,
modulation controls and more) are in the [roadmap](roadmap.md).

## Preview

Play/pause, timecode and frame, how far ahead is rendered (and the playback speed when rendering can't keep up),
preview resolution (full, ½, ¼, ⅛, 1/16; ½ by default) and playback volume. With no video loaded, the preview
offers an Open Video button.

## Node graph

Our own editor, drawn on a pannable, zoomable canvas (see [Decisions](decisions.md#own-node-graph)).

- Scroll wheel zooms around the pointer; middle- or right-drag pans; F frames the whole graph.
- Left-drag on empty space box-selects (Shift adds). Click a node to select it and show it in the inspector;
  drag to move the selection.
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
  Enter (or click). Right-click a node to duplicate or delete it; Delete removes the selection, Ctrl+D
  duplicates it.
- **Inspecting a connection.** Resting the pointer on a wire or an output pin shows what it carries at the
  playhead: a meter and the mean, min, max and RMS, with a view above them and the list of views beside. The
  view is a **picture** (the signal stretched over the project's shape, the way Video Output does), a **scope**
  (the waveform), a **spectrum** (an FFT on a logarithmic frequency axis, smoothed between updates) or
  **readings only**, and any signal can be seen any way: audio as a picture, video as a spectrum. Hold **Alt**
  over a connection and scroll to go through the views, one click of the wheel to a step; the popup fades and
  resizes from one view to the next, and the wheel stops zooming meanwhile. The view is remembered separately for
  audio and for other signals (File → Settings). Hold **Shift** to hear the connection through the speakers,
  turning the playback down while the pointer stays on it; with playback stopped it plays on from the playhead
  without moving it. How often the view asks for a new frame while the playhead moves is the project's **update
  rate** (File → Settings → Inspecting connections). It all reads through the engine's
  [taps](engine.md#taps-and-listening), so it never changes the render. The picture, scope, spectrum and meter are
  shared widgets (`widgets/`) that take plain samples and pixels, so they can be used anywhere.
- Wire thickness follows the RMS level of the signal at the playhead, so modulation is visible: a kick drum
  through a band split shows as the bass wire pulsing.
- Wire colour shows what a wire carries, from the tag the last compile gave each output (before that, from
  the output's tag rule). A wire has a base colour for its kind (video neutral, audio teal) and, if it carries
  part of a signal, a second colour for that part: red, green or blue, or left or right, from Split Channels;
  low, mid or high from Three-Band Split. Effects keep the colours of their main input, so a delay on the red
  channel is still red video; Video to Audio turns it into red audio. View → Wires picks how the two show: solid
  (the part's colour, or the kind's), outlined (the kind's colour outlined in the part's) or gradient (the part's
  colour down the centre, fading to the kind's at the edges).
- Compile notes and warnings show as a badge on the node, with the message in its tooltip and in the inspector,
  which also lists what each output carries. Notes (a signal whose tag doesn't suit the node, say) get a quiet
  "i" badge; warnings (something lost or ignored) a yellow "!". Neither stops the render. With several inputs,
  the main input (which sets the output's length and layout) has a ring around its pin.
- When the graph can't render, a bar along the bottom of the graph says why and outlines the node at fault in
  red; clicking the bar shows the node.
- Moving or renaming nodes doesn't re-render; any other edit does.

## Inspector

The node's name (shown on the node instead of its type), its shared settings (Resampling, Channels; see
[Node behavior](node-behavior.md#shared-node-settings)) and its parameters, with units, sliders (always linear) reset-to-default buttons, and an `int` toggle on numbers that are not whole-only (which rounds the value and, when a signal modulates it, every sample). Parameters that only make sense whole (Beat division and steps, Pack channels, Chorus voices, Phaser stages, Noise seed, Sample & Hold levels, Resample width and height) are declared `.integer()` in their spec: the slider handle jumps between whole values, typed numbers round, and they have no toggle. Each slider covers the parameter's
usual range; typing (or dragging the value box) past it, up to the node's limits, widens the slider to match. Right-click a slider to set its range; the range is saved with the node in the graph (`"ranges"`), and it is also what a modulating signal is held to (unless **Allow past the slider's range** is ticked). Each
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

## Timeline

A ruler, the video track and any number of **audio tracks**, with Reaper-style track headers on the left. Each track
is a lane of **items**, the stretches of its file placed on the timeline. The video header shows the track's name
(its file's name until renamed; the video input reads it by that name), size and frame rate; each audio header has
the track's name (which audio inputs select it by; renaming a track updates them), its volume in the track mix, and
its bus when the project has several. Every header has **mute** and **solo** (*S*: while any track of a kind is
soloed, only soloed tracks of that kind are in the [track mix](engine.md#track-mix-and-output-buses)); audio headers
also remove the track. Tracks left out of the mix are drawn dimmed. The project lasts to the end of its last item and
runs at its [timebase](engine.md#timeline). **+ Audio track** (or File → Add Audio Tracks…) adds several files at
once, each named after its file. Opening a video that has sound adds that sound as an audio track too. For now the
app opens one video track; more arrive with the Resources panel on the
[roadmap](roadmap.md#timeline-resources-and-graph-layers).

Every item has a **header bar** along its top with the track's name and a mute button (a muted item reads as a gap,
for graphs too). Drag the bar to move the item; the area below it shows the item's content, thumbnails of the
source video (decoded by a separate small decoder so they never slow rendering and survive graph edits) or the
audio's waveform, and seeks like empty lane space. Items can't start before the timeline does.

Right-click a header to **link** its track with another (or unlink it): dragging an item then moves the items of
the linked tracks that overlap it, so a video and its sound stay together. Linked headers say so. Drag the bottom
edge of a header to change the track's **height** (double-click it, or the header's menu, for the default). Drag an
audio header to reorder the audio tracks. Frames rendered so far are marked in green along the bottom of the ruler.
Linking, heights, solo and mutes are saved with the project and undo like any edit.

- The scroll wheel zooms time around the pointer, from half the whole video down to a few frames (over the
  headers it scrolls the tracks); middle- or right-drag pans in both directions; F shows the whole video.
- Tick lines run behind the lanes, labelled on the ruler, down to single frames when zoomed in.
- Click or drag on the ruler, empty lane space or an item's content to seek.
- **Loop region** (as in Reaper): drag along the ruler to make one, snapped to whole frames; drag its edges to
  change it. R or the Loop button by the play button turns looping on and off; right-click the ruler to do the
  same or remove the region. Playing into the region repeats it; playing from after it plays on. While looping,
  rendering ahead wraps from the region's end to its start, so the loop plays without waiting. The region is saved
  with the project.

## Preview audio

Plays the master bus (the first output bus) and follows the playhead: its **track mix**, the audio tracks
routed to it at their volumes, leaving out muted ones (and unsoloed ones while any is soloed). When the graph has an **Audio Output** writing to the master bus, playback plays its
rendered sound instead (track volume and mute don't apply to it); a track wired straight into that Audio Output
plays as it is. A bus with more than two channels plays its first two. When playback slows because rendering can't
keep up, the audio is time-stretched (WSOLA: slowed without lowering the pitch) to stay with the picture, and fades
out when playback all but stops. Volume, mute and routing shape the track mix (in playback and the CLI's export),
never what graphs read.

With several buses, each audio track's header shows the bus it is routed to, with a menu to change it.

## Settings

**File → Settings…** (Ctrl+,) opens a window with two pages. **Application** is remembered on this computer: theme,
wire style, the node latency and warmup display, and keeping input connections when duplicating and pasting.
**Project** is saved in the project file: the **picture** (resolution and frame rate, from 23.976 to 60 fps with
NTSC rates as exact fractions; until one is set the fields show the first video's and say so, and the reset button
goes back to following it), tempo (bpm, beats per bar, first beat), *Max warmup frames* (how much
is pre-rendered after a jump in the timeline; the page warns when the graph needs more), the Audio Output rate and
the **output buses** (below). Each
setting has a line of help. The tempo bar in the timeline edits the same tempo fields. The preview resolution and
volume stay on the transport bar, where they are used while playing.

**Output buses** lists the project's buses, master first, each with a name and a channel count (mono, stereo, 3 to
8 channels; 5.1 is six). *+ Add bus* adds one; renaming a bus renames it on its tracks and Audio Outputs too. The
master can't be removed. Removing a bus that tracks are routed to, or that Audio Outputs write to, asks first and
lists them: the tracks move to the master, and the Audio Outputs are left writing to a bus that isn't there (shown
in red in the inspector) and aren't rendered until set to another.

## Keys

Space plays/pauses, ←/→ step one frame, Home jumps to the start, R turns looping on and off, Ctrl+S saves, Ctrl+Z
undoes, Ctrl+Shift+Z (or Ctrl+Y) redoes. In the graph, Ctrl+C, Ctrl+X and Ctrl+V copy, cut and paste nodes with
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

- **Theme:** View → Theme picks Dark (the default), Light or Follow system. Every colour the app paints itself
  comes from `rastersong-gui/src/theme.rs`. The theme, preview resolution and volume are remembered between
  sessions.
- **About** credits FFmpeg and its LGPL license and lists the loaded FFmpeg libraries and build configuration.
