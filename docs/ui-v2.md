# UI V2

The plan for the second version of the desktop app's interface. It keeps every feature in [Desktop app](app.md)
and changes where things live and how they read. This is a **target**, not a description of the code:
[Desktop app](app.md) stays the source of truth for how the app works today, and each part below moves into it
as it ships. The work is tracked in the [roadmap](roadmap.md#ui-v2); the reasons are in
[Decisions](decisions.md#ui-v2-october-2026).

The mockups are in [`design/ui-v2/`](design/ui-v2/README.md), with a live canvas linked from there. Where this
page and a mockup disagree, this page wins; fix the mockup.

## Priorities

1. **Controls sit next to the work they act on.** The transport goes under the preview, the tempo fields next to
   the Time/Tempo switch, the bypass toggle in the inspector next to the node it bypasses.
2. **The graph reads at a glance.** Node text at 12 to 13 px, a category colour strip along the top of each node,
   a legend for wire colours.
3. **The inspector follows the selection**: a node, a graph item or a track, each with the same layout.
4. **Nothing hides behind a tab that doesn't need to.** (Media and Graphs shared the Resources panel at first;
   since October 2026 they are two tabs, so a panel shows more cards of one kind without scrolling.)
5. **Destructive actions are one step removed.** Removing a track or a resource moves into menus, out of the
   headers and cards where a stray click hits it.
6. **The hidden gestures are taught where they apply.** The status bar names the modifier keys for what is under
   the pointer (Alt to change the probe view, Shift to listen, Ctrl+drag for a loop region).
7. **Numbers line up.** Every timecode, size, rate and value is set in a monospaced face so it doesn't jitter as
   it changes.

## Design tokens

The dark theme first. One accent, used for the playhead, the selection, focus and the primary button; colour
otherwise means a signal or a state, never decoration. All values are in `theme.rs` (the one place colours live):
the surfaces in `Theme::DARK.surfaces`, which `apply_style` gives to egui's panels and widgets, the rest in the
other `Theme` fields. **These shipped in stage 1** for the dark theme; the light theme has the same tokens (stage
10, [below](#light-theme)).

| Token | Value | In `theme.rs` | Used for |
|---|---|---|---|
| Ground | `#0E0F11` | `surfaces.ground`, `canvas_bg` | Wells, text fields, the graph canvas |
| Panel | `#16171A` | `surfaces.panel` | Panels, windows, menus |
| Raised | `#1C1D21`, hover `#24262B`, pressed `#2A2C31` | `surfaces.raised*`, `lane_bg` | Buttons, dropdowns, checkboxes, timeline lanes |
| Border | `#2A2C31`, hover `#3A3C42` | `surfaces.border*` | Panel and control outlines |
| Divider | `#202227` | `surfaces.divider` | Separators and lines inside a panel |
| Inner card | `#1A1B1F` | `surfaces.inner_card` | A card inside a panel: the modulated parameter |
| Text | `#ECEDEF` | `surfaces.text` | Primary text |
| Text muted | `#A1A4AC` | `text_dim` | Secondary text, units |
| Text soft | `#C3C6CD` | `surfaces.text_soft` | Setting names, ghost buttons |
| Text faint | `#868A94` | `surfaces.text_faint`, `tick_label` | Labels, hints, disabled |
| Node | body `#1E1F23`, outline `#33353B`, grid `#1E2024` | `node_body`, `node_outline`, `grid_minor` | The graph canvas |
| Accent | `#8FA2FF` | `accent`, `playhead` | Playhead, selection, focus, links, primary button |
| Video | `#C9CCD3` | `ports.video` | Video wires |
| Audio | `#3FC7B4` | `ports.audio` | Audio wires |
| Red / green / blue | `#FF5D5D` / `#4ED37A` / `#5B8CFF` | `ports.red` … | Channel parts |
| Low / mid / high | `#FF9A3C` / `#F2D04B` / `#B48CFF` | `ports.low` … | Band parts |
| Graph | `#9C86FF` | `graph_block` | Graph items |
| Warning | `#F2B544` | `warning` | Warnings |
| Solo | `#F2D04B` | `solo` | A soloed track's S button |
| Beat | `#F2B544` | `beat` | The first-beat marker, the metronome and its beat lights |
| Warning ground | `#26231B`, border `#6E5420`, text `#F4E3BD` | `warning_note`, `warning_note_border`, `warning_note_text` | A node's warning note, the odd-size bar in Export |
| Graph text | `#B9A9FF` | `graph_text` | The GRAPHS section label |
| Graph item | body `#2D2650`, bar `#3F3570`, text `#E4DDFF`, outline `#9C86FF` | `graph_item`, `graph_block` | Graph items on the timeline |
| Video item | body `#1E2433`, bar `#2A3247`, text `#D6DBE8` | `video_item` | Video items |
| Audio item | body `#10302C`, bar `#17423D`, text `#C9F1EA`, waveform `#3FC7B4` | `audio_item`, `ports.audio` | Audio items |

Category colours for nodes keep the six in `Theme::categories`; the strip shows them at full strength. A test holds
every text colour, in both themes, to WCAG contrast on every surface it can sit on: 4.5 for text and muted text, 3
for faint labels and the accent, and 4.5 for an item's name on its bar.

### Light theme

**Shipped in stage 10.** The boards are dark only, so the light theme is the same tokens turned over rather than a
board of its own: panels are white (`#FFFFFF`) on a cool grey ground (`#EEEFF2`), which wells and text fields sink
into; raised controls step down `#F4F5F7` / `#ECEDF1` / `#E2E4E9`, outlined `#D5D8DE` (hover `#B9BDC6`); text is
`#17181B`, soft `#3D4048`, muted `#555A64`, faint `#6B6F7A`. The accent darkens to `#4A5FD9` to read on white and
is the playhead and the drag bubble too. Items are pale tints of their kind with dark names (video `#E3E8F2`, audio
`#DCF2EE`, graphs `#ECE8FC`); the preview stays dark. All values are in `Theme::LIGHT`.

**Type.** Inter 4.1 for text, JetBrains Mono 2.304 (the NL build, without ligatures) for numbers, bundled in
`crates/rastersong-gui/assets/fonts/` and put ahead of egui's own faces, which still cover the symbols and emoji
they lack (see [Licensing](licensing.md#fonts)). Inter comes in three weights, as on the boards: regular for text,
medium for buttons, menus and tabs (egui's button style, 12.5 px), and semibold for titles (panel titles, node
titles, the project and graph names; `theme::semibold`). The faces are drawn unhinted, as browsers draw them on
the boards: hinting rounds the lowercase height up a whole pixel at these sizes, and the text looks tall and
narrow. Sizes: body
and buttons 13 px, monospace 12.5 px, small 10.5 px, headings 16 px (these are egui's text styles, set in
`apply_style`); node titles 12.5 px, port labels 11 px, section labels 10.5 px in capitals with 0.08 em tracking,
and the large timecode 15 px come with the stages that draw them.

**Shape.** Primary controls are 32 px high (`CONTROL_HEIGHT`), every other control 26 px (`DENSE_HEIGHT`,
egui's interact height). Corners are 6 px on controls, 8 px on nodes, cards and menus, 12 px on windows (the
`*_RADIUS` constants). Spacing steps are 4, 6, 8, 10, 12, 16 and 20 px.

## Layout

The window keeps the two rows from [Desktop app](app.md): preview, graph and inspector on top; Resources and the
timeline below. Board: [Main](design/ui-v2/Main.dc.html). **The top bar, status bar, panel cards, preview header
and transport shipped in stage 2**; what Tempo mode adds to the transport shipped in stage 8.

- **Panels are cards** (`widgets::card`): rounded 8 px, outlined, on the window's ground, 6 px apart and 6 px from
  the window's edge. A card's title row (`card_header`) is 40 px with a divider under it. The preview and the graph
  have theirs; the inspector, Resources and the timeline get theirs with their own stages (5, 6 and 7).
- The gap between two cards is the resize handle (`card_panel`): each card keeps half the gap, so the panel's edge
  runs down the middle, and no line is drawn there, even while hovered; the cursor shows it.
- Controls are outlined at rest, as the boards' buttons are.

- **Top bar**: the menus, the project name with an "edited" mark, a Settings button, and an **Export…** button on
  the right. No transport here.
- **Status bar** along the bottom: the compile state on the left ("Graph compiled", or the error), the hint for
  what is under the pointer in the middle, the cache use and the project's picture size and rate on the right.

## Preview and transport

The preview is up to 460 px wide. Its header holds **Result / Original / Split** (Original replaces the
*Unprocessed* toggle) and a **Fit** button.

The **transport sits under the picture**, in two rows that never wrap:

1. Go to start, **play** (the accent button), loop, and the timecode box: position large, length small, the frame
   number at the right.
2. **Ahead** (how far is rendered, as a bar with seconds; it names the playback speed when rendering can't keep
   up), the preview resolution, and the volume.

In Tempo mode the timecode box reads **bar.beat.tick** large and seconds small, with a beat light of a segment per
beat ([Tempo mode](#timeline-in-tempo-mode)). Unlike the board, the light sits in the box between the reading and
the seconds, and the row under it keeps Ahead, resolution and volume without the board's "128 bpm · 4/4" (the
tempo group shows it), so the narrow preview card doesn't crowd.

## Node graph

Board: [GraphDetail](design/ui-v2/GraphDetail.dc.html). **Shipped in stage 3**, the wire probe in stage 4. Where it
differs from the board: the legend is a small icon in the bottom-right corner (above the error bar)
that shows the key while hovered, so it never covers a node; the key also names stereo's L R. Fitting the graph
stops at 85% so its text stays readable; a graph too wide for that starts from its inputs. The error bar is as before: a full-width strip that shows the node when clicked, without the
board's *Show node* button or its line about the last good frame.

- Each node has a **5 px colour strip** along its top edge in its category colour; the title sits below it, so
  colour and text don't compete. Nodes are wider (116 px at 100%) so titles fit.
- A **bypassed** node is drawn faded on a hatched fill, with BYPASSED in its title row. A node with a warning
  has an amber outline and its message on the node; a failing node a red outline. The canvas header counts them
  ("1 error · 1 warning"). The error bar along the bottom of the canvas stays: it says why the graph can't render,
  that the preview shows the last good frame, and has *Show node*.
- A **legend** in the corner names the wire colours (video, audio, R G B, low mid high) and that thickness is RMS.
- The canvas header names the open graph and where it is placed ("on Graph track 1 · 0:00–0:03"), with *Add node*
  (Tab).
- **Node search** opens as a palette over the canvas: a search field, what it connects to when opened from a
  dropped wire, results grouped by category with a one-line description each, and the keys (↑↓, Enter, Esc) at the
  bottom.

### The wire probe

Hovering a wire opens the probe, as now: the wire's ends ("Three-Band Split.low → Delay.in"), what it carries,
a view, a meter and the readings. The list of views becomes a **drum**:
a vertical wheel of view icons (Picture, Scope, Spectrum, Readings) beside the view, with the active one in the
middle, highlighted, and its neighbours tilting away above and below. It **wraps around**: past Readings comes
Picture again.

- Alt+scroll on the wire turns the drum one view per wheel click, as Alt+scroll changes the view today.
- Clicking an icon turns the drum to it. With the probe focused, ↑/↓ turn it.
- The drum settles with a short ease; the view cross-fades as it does now.
- Shift to listen is unchanged; the probe's footer names both gestures.

Each of these is a kittest ([CONTRIBUTING](../CONTRIBUTING.md) rule 14).

Where the shipped probe differs from the board: it stays open while the pointer is on it, or on its way to it from
where it opened, so the drum can be clicked; the plain wheel over the drum turns it too (Alt+wheel stays the
wire's). The scope and spectrum are drawn in the wire's colour. The meter is still the shared one, a bar with its
value in a box, until the inspector's stage restyles it everywhere. The line under the ends is the tooltip's
layout and tag, sample count included. *Readings* shows no box, since the numbers are right under it.

## Inspector

Board: [Main](design/ui-v2/Main.dc.html). **Shipped in stage 5**; the layout is `widgets/inspector.rs`, shared by
the node, graph item and track inspectors. Where it differs from the board: a graph item's name is its graph's,
shown but not edited there (graphs are renamed in the Resources panel); graph tracks aren't selectable, so only video
and audio tracks have a track inspector; the meters under the outputs are still the shared ones. The knob's
right-click menu is gone, since everything it held is now in the card.

Follows the selection, and every kind of selection uses one layout: a header with the kind ("Node", "Graph item",
"Track"), the name (editable through `name_edit`) and the actions that apply to it (Bypass with its Alt+click hint
for a node), then sections with capital labels.

For a node: the name, the type with a one-line description, what each output carries, the node settings, then the
parameters. A **modulated parameter** gets a card: the slider with the modulated range outlined and the ghost
handle at the live value, the amount knob, the source ("from Three-Band Split · low"), one way / both ways, and
"Moves by" in the parameter's unit. The behaviour is the same as in [Inspector](app.md#inspector).

## Resources

Board: [Main](design/ui-v2/Main.dc.html). **Shipped in stage 6.** Where it differs from the board: a missing file
keeps the error red rather than the board's dashed amber, and says *missing* (the board's *file missing* wraps
beside Relocate…); the video's picture is an icon on the video colour, not its first frame; the hint sits under the
graphs rather than pinned to the panel's bottom; and the groups stack when the panel is narrowed below 360 px.

Media and Graphs first showed **together**, as two labelled groups of rows. Since October 2026 they are two tabs
with counts ("Media · 4", "Graphs · 2") over a wrapping grid of 4:3 cards, which fits more resources in the panel;
see [Resources](app.md#resources) for how it works now. A filter field at the top narrows both. Cards keep
`resource_card`; a missing file keeps its red card and Relocate… button. A hint line under the cards names the
drag and double-click gestures.

## Timeline

Board: [Main](design/ui-v2/Main.dc.html). **Shipped in stage 7**; the section labels, track heights and item colours
followed with stage 10. Where it differs from the board: the tempo fields and Metronome stay in the title row (the
Tempo stage's group), so the loop region and Snap sit at its right end, the region before Snap; the volume reads in
percent, not dB; lanes keep their outlined grounds, so an empty stretch of a track still reads as the track; a
track's name is still edited in place, outlined on hover; and a waveform is drawn as the audio's shape, not the
board's bars.

The three sections, items, trimming and graph items work as in [Timeline](app.md#timeline). The changes:

- Each section is named in a capital label above its tracks (GRAPHS in the graph colour, VIDEO, AUDIO in the audio
  colour), the thick line between two sections along the label's top. Tracks are 34 px (graphs), 52 px (video) and
  44 px (audio) until resized.
- Items are a dark body of their kind's colour with a bar along the top in a step lighter, their name in semibold;
  graph items are outlined in the graph colour.
- Track headers are calmer: a bar of the kind's colour, the name with a faint detail line under it (size and
  frame rate, or a slim volume bar), and small M and S toggles (S yellow while on); **removing a track moves into
  its right-click menu**.
- The toolbar holds the **Time / Tempo** switch, **Snap**, and the **Loop** button with the region it covers; with
  no region it says "Ctrl+drag the ruler for a loop region".

### Timeline in Tempo mode

Board: [Tempo](design/ui-v2/Tempo.dc.html). **Shipped in stage 8.** Where it differs from the board: the bpm
field types on a click like every value box, not a double-click; there is one beat light per beat of the bar, up to
eight; the transport's beat light and tempo are placed as described [above](#preview-and-transport); beat-grid
snapping and loop edges already followed the grid before this stage, and Shift already dragged freely.

- **The tempo fields live in one group attached to the Time/Tempo switch**: bpm (drag to scrub, click to
  type), beats per bar, first beat, and the **Metronome** toggle with a beat light per beat showing the current one.
  Unlike the board, the group stays in Time mode (the Metronome greyed out), so switching modes moves nothing
  on the toolbar. File → Settings → Project still edits the same fields.
- **The ruler has two tiers**: bar numbers bold on top, beats small below (2.2, 2.3, …).
- **Every other bar is shaded** down through the lanes, and beat lines are fainter than bar lines.
- An **amber marker** on the ruler shows the first beat.
- **Snap reads "Snap · beats"**, and the loop region is given in bars ("2.1 – 4.1"); both follow the beat grid.
  Shift drags freely.
- The transport under the preview reads bar.beat ([above](#preview-and-transport)).

## Export

Board: [Export](design/ui-v2/Export.dc.html). The window is two columns. **Shipped in stage 9.** Where it differs
from the board: the window is 720 px tall, not 760; the encoder is named as FFmpeg calls it (`h264_nvenc`); the
fix button reads *Use 1282×720*, both sides; the card at the bottom right is titled *Progress* and says "Not
started yet." before the first export, then where the file went; while an export runs the settings are locked; and
an odd side is outlined in the warning colour only while the size is custom.

- **Left, the settings**: format as three cards (name, what it is for, the sound it carries, the encoder in use),
  quality or profile as a segmented control, sound (bus) and range side by side, size with Project / Custom. An
  odd size shows a warning bar that names the odd side, with a *Use 1282* button.
- **Right, the result**: the **latest rendered frame** as a 16:9 preview with a "Rendering" tag and its size,
  labelled with its frame and time; the summary (file, picture, rate, length, warm-up and why); the progress with
  the time left and Cancel; and the Export… button, disabled with its reason while the size is odd.

Everything else is as in [Export](app.md#export).

## Not covered yet

- **The Settings window** keeps its current layout until it gets a board of its own.
