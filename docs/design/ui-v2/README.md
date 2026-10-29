# UI V2 design boards

The source of the UI V2 mockups described in [UI V2](../../ui-v2.md). Each `.dc.html` file is one artboard
from the design canvas; `canvas.json` lays them out and holds the sticky notes.

The live canvas, with notes and the interactive wire-probe drum, is at
<https://claude.ai/artifact/5jCJr4VbuUeR1xNKGPtgCD>. These files need the canvas runtime to render, so open
the link to look at them; read the files here for exact sizes, colours and wording.

| Board | Shows |
|---|---|
| [Main.dc.html](Main.dc.html) | The edit workspace with a node selected: top bar, preview with the transport under it, node graph, inspector, Resources, timeline in Time mode, status bar |
| [GraphDetail.dc.html](GraphDetail.dc.html) | The graph editor: bypassed and failing nodes, the error bar, the wire legend, node search, and the wire probe with its view drum |
| [Export.dc.html](Export.dc.html) | The Export window: settings on the left, the latest rendered frame, summary and progress on the right |
| [Tempo.dc.html](Tempo.dc.html) | The timeline in Tempo mode: the tempo group, the two-tier ruler, the beat grid, and the transport reading bars and beats |

When the design changes, update the canvas and copy the changed files here in the same commit as the matching
change to [UI V2](../../ui-v2.md).
