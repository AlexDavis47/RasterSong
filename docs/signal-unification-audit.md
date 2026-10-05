# Signal unification audit (temporary working document)

Started 2026-10-04. Delete or fold into `readme.md` once the decisions are implemented.

## 1. Philosophy

RasterSong removes the wall between video and audio DSP. No node may require one domain.
Type information is **advisory**: it colours wires and produces warnings, but processing
always runs. The user is trusted to reinterpret signals on purpose (RGB into a stereo
splitter, stereo into a modulator as interlaced samples). Conversions are never implicit.

## 2. Current state (audited 2026-10-04)

| Area | State |
|---|---|
| Signal type | One `Signal` (`Vec<f32>` + `Layout{width,height,samples_per_pixel}`). Video is `rgb(w,h)`, audio is `Layout::audio(n)` = `(n,1,1)`. |
| Time base | One block = one video frame. Audio block = `round(sample_rate / fps)` samples (about 1602 at 48 kHz / 29.97 fps). |
| Rate matching | Main input (first) sets length and layout; other inputs resample to it (`graph.rs` `binding`). Mono over RGB uses `group = 3` so a pixel's R, G, B stay equal. RGB down to mono uses `group = 1` (interlaced as-is). |
| Units | `PrepareContext::sample_rate()` = `samples_per_frame * fps`, so rows, beats, Hz resolve per signal. |
| Conversion | Explicit `to_audio` / `to_video` (range), `split` / `combine` / `interleave` / `pack` (structure). Nothing clamps between nodes. |
| Type info | `PortHint` is a static label per output spec, editor-only (wire colour). Not part of the data. |
| Channels setting | `NodeDesc.channels`: `Together` runs the interleaved stream through one node; `Separate` runs one node instance per channel (deinterleave, process, reinterleave). Only for nodes flagged `.per_channel()`. Silently a no-op on mono main input. |
| Audio | Mono only for modulators (channels averaged on load). Graph output is video only. |
| Sink | Single `output_step`, single `latency_frames`. |

## 3. Decisions

1. **Type metadata lives in `Layout`** (resolved at compile time, propagates through `output_layouts`). It carries kind (audio / video / unknown), channel meaning (mono, stereo L/R, RGB), and a nominal range (0..1, -1..1, unknown).
2. **Hints never block processing.** Compile gains a non-fatal diagnostics list, for example "L/R Split expects stereo, got interlaced RGB" or "Gate expects -1..1, got 0..1".
3. **No implicit conversions.** Range conversion, summing, splitting are explicit nodes. Range tag defaults to *inherit*; only conversion and range-defining nodes (`to_audio`, `to_video`, `remap`, `offset`, `clamp`, generators) set it. Level-based nodes may declare an expected range, which only feeds warnings.
4. **Keep the mono-over-RGB grouping** as a node-wide option, and add an explicit "stretch to match" node, since it cannot be built from existing nodes.
5. **Audio output to the cache is planned.** Fixed-size blocks stay. Resample on cache write, with history carried across blocks (no edge clicks). If the audio source is wired straight to the sink, mux the original track untouched.
6. **Audio inputs are native interleaved.** Stereo arrives as L,R,L,R (`samples_per_pixel = 2`), tagged stereo. No downmix on load; the user adds a Sum or Split node. Old graphs are not migrated.
7. **Generic N-way Split / Combine Channels.** Port count follows `samples_per_pixel` (resolved at compile time); hints name the ports (L/R or R/G/B). Replaces the RGB-only `split` / `combine`.
8. **Audio sink sanitizing: hard clip to -1..1 and NaN/Inf to 0**, mirroring the video sink. Softening is the user's Limiter node.
9. **Separate Audio Output node.** Video Output is unchanged. Each sink has its own latency; the mux aligns them. A graph without an Audio Output exports the source audio untouched.
10. **Sink channel count:** 1 sample per pixel is mono, 2 is stereo. Anything else is written as interleaved samples to a stereo file, with a warning. Never an error.
11. **Sink sample rate:** a project-level output rate, 48 kHz by default. The write-side resampler converts from each block's effective rate (block length × fps).
12. **Relabel node:** changes only the tag (kind, channel meaning, range) for deliberate reinterpretation. Zero processing cost.
13. **Defaults set without asking** (change any of these if you disagree):
    - Mono-over-RGB grouping stays on by default, so current graphs render the same.
    - `Separate` copies stay identical, matching `Split → node ×3 → Combine` with the same params. Per-channel variation is the user's job.
    - NaN and Inf are scrubbed at sinks only, not between nodes.
    - Variable frame rate is deferred.
    - Diagnostics show as a badge on the node, with the message in its tooltip and in the inspector.

## 4. Pressure points

Impact: **High** gates other work; **Med** needs a decision before related work; **Low** track it.

| # | Point | Impact | Status |
|---|---|---|---|
| 1 | Fixed block vs exact sample counts (1601.6 samples per frame) | High | Decided: fixed blocks, resample on write, passthrough shortcut |
| 2 | Block-edge seams: `resample` clamps at block edges, so per-block resampling on write would click | High | Decided: Phase B step 2 |
| 3 | Single sink: one `output_step` and `latency_frames`; video and audio sinks need separate latency and mux offsets | High | Decided: separate Audio Output node, per-sink latency (Phase B) |
| 4 | Sink sanitizing policy (clamp or limit, NaN or Inf, DC) | Med | Decided: hard clip + NaN/Inf scrub at sinks |
| 5 | NaN poisoning: a NaN in a recursive node persists in state; warmup and seek re-poison it | Med | Deferred: scrub at sinks only |
| 6 | Tag transition rules after `to_audio`, `pack`, `interleave`, `resample`, merges of different kinds; user "relabel as" node | Med | Done (Phase A): tag rules + Relabel node |
| 7 | Range convention: nodes assume bipolar; video is 0..1 | Med | Done (Phase A): nominal tag, advisory warnings, explicit conversion |
| 8 | `Separate` vs `Together` unit parity: per-instance `sample_rate` is one third in `Separate`; cycles-per-sample params differ | Med | Done: rows/frames/Hz already agree per pixel; tested with a one-row delay |
| 9 | `Separate` copies start identical: seeded noise or LFO phase would match across channels | Low | Decided: copies stay identical |
| 10 | `per_channel` flag is hand-set on about 30 nodes; N-channel (stereo) should generalize it | Low | Done: `Separate` works for any channel count; the flag only offers the option, and a misfit falls back to `Together` with a warning |
| 11 | Variable frame rate: compiler assumes constant `frame_rate` | Low | Deferred |
| 12 | Multiple audio tracks at different sample rates | Low | Untested |
| 13 | Property tests use mono layouts only; no RGB, `Separate`, stereo, or cross-domain coverage | Med | Done: stereo and RGB block layouts in the property tests; graph tests for stereo split/combine, Separate on stereo, grouping |
| 14 | `Split` / `Combine` hard-require RGB (`expect_rgb`); should be one N-way channel split with a hint | Med | Done (Phase A): N-way Split / Combine Channels, `expect_rgb` removed |
| 15 | First input defines output length; easy to mis-order | Low | Editor: mark main input visually |

## 5. Implementation plan

### Phase A: tags and signal freedom (one commit) — implemented 2026-10-04, uncommitted
1. Kind, channel-meaning and nominal-range tag in `Layout`, propagated through `output_layouts`. Static `PortHint`s become tag-setting rules (decisions 1, 3).
2. Non-fatal compile diagnostics list. Editor shows a badge on the node, a tooltip and the message in the inspector; wire colour comes from the tag (decision 2).
3. Expected-range declarations on level-based nodes (gate, compressor, rectify, distortion, dB params).
4. Native interleaved audio input; remove the mono downmix in `Modulator` (decision 6).
5. N-way Split / Combine Channels replacing `split` / `combine`, with compile-time port counts (decision 7).
6. Stretch-to-match node, and a node-wide grouping option that is on by default (decision 4).
7. Relabel node (decision 12).
8. Generalize `Separate` to any `samples_per_pixel` (stereo included).
9. Tests: `Together` vs `Separate` parity, stereo and RGB layouts in property tests, cross-domain rate matching, diagnostics (items 8, 13).

### Phase B: audio output (separate commit)
1. Audio Output node; multi-sink compile with per-sink latency (decisions 9, 10).
2. Write-side resampler with cross-block history, to the project output rate (decisions 5, 11).
3. Sink sanitizing (decision 8) and the passthrough shortcut.
4. Audio cache and the export mux.

## 6. Phase A as built

Where the build differs from or adds to the plan above:

- **Tag** (`signal.rs`): `kind` (unknown/video/audio), `channels` (`ChannelMap`: unknown/mono/stereo/RGB/numbered),
  `part` (whole, R/G/B, L/R, numbered channel, low/mid/high band), `range` (unknown, `0..1`, `-1..1`). `part` was
  added so wire colours keep working from tags. A channel meaning that doesn't match `samples_per_pixel` is
  replaced by the usual one for the count and kind (`Tag::fit`); an untagged layout stays untagged. Layout
  equality includes the tag, so shape checks use `Layout::same_shape`, and shape-changing nodes use
  `Layout::reshaped` to keep the tag.
- **`PortHint` is gone.** Each `OutputSpec` has a `TagRule` (optional kind, part, range) that the compiler applies
  after `Node::output_layouts`. A rule that changes the kind re-derives a channel meaning that was only the
  default for the old kind, so three audio channels become RGB again after Audio to Video.
- **Range-setting nodes** write the range in `output_layouts`: generators (oscillator and noise from
  offset ± amplitude, beat `0..1`, constant from its value or the layout's usual range), Clamp, Remap, Offset.
  Rectify has a `center` parameter and works on either range, so it declares no expected range.
- **Expected ranges** (`NodeSpec::expects`): Gate, Compressor, Limiter and Distortion expect `-1..1`; Output
  expects `0..1`. No per-parameter dB expectations were added; the node-level one covers the warning case.
- **Diagnostics**: `Node::diagnostics(&LayoutContext)` holds node-specific warnings, and the compiler adds its own
  (expected range, Separate fallbacks). They are stored per node in `NodeStats { inputs, outputs, diagnostics }`,
  and `Graph::diagnostics()` flattens them. `LayoutContext` gained `connected`.
- **Separate** is never an error now. On a one-channel signal, on a node without `per_channel`, or on a node
  whose copies would change the shape, it runs Together and warns.
- **Split / Combine Channels**: the kinds stay `split` / `combine`; the ports are `c1`…`c8`
  (`MAX_CHANNELS = 8`). Split outputs past the input's channel count are silence. Combine's channel count is the
  number of the last connected input. Inputs of a different size are stretched to `c1` with a warning; this used
  to be an error. A port-rename migration maps old `r`/`g`/`b` connections, so old graphs and the goldens render
  the same. Interleave accepts any channel count, and Pack gained a `channels` parameter (default 3).
- **Stretch to Match** (`stretch`, inputs `like` and `in`) takes `like`'s shape and `in`'s values and tag. The
  node-wide option is `grouping: pixels | samples` on `NodeDesc`. It defaults to pixels, so old graphs render
  the same.
- **Relabel** (`relabel`, Conversion) sets `kind`, `channels` (keep/named/numbered), `range` and `part`
  (keep/whole). It copies its input (one memcpy), so it is cheap rather than free.
- **Audio input**: `Modulator` keeps the decoded channels interleaved. The renderer declares
  `Layout::audio_channels(block_len, channels)` and tags video as `Layout::video`.
- **Editor**: the app runs `Graph::inspect` on the edited graph whenever the graph or the engine's
  `compile_options()` change. It is a shape-only pass over every node, including ones that don't feed the output.
  The engine's latency and warmup are merged in. A Split therefore shows L and R as soon as stereo is plugged in.
  Wire colours come from these tags, with the static rules as a fallback for nodes that can't be inspected
  (before any video is loaded, or downstream of a node that rejects its input). Split shows one output per input
  channel; Combine shows its connected inputs plus one free one. Labels are R/G/B, L/R, or numbers. Warnings
  show as a badge with a tooltip. The inspector lists warnings and each output's layout and tag, and has the new
  Grouping setting.

## 7. Change log

- 2026-10-04: initial audit and decisions recorded. No code changed.
- 2026-10-04: open questions answered (decisions 6-13); plan split into Phase A and Phase B. Ready for implementation.
- 2026-10-04: Phase A implemented (see section 6). Workspace tests, clippy, fmt and the docs check pass. Not yet committed.
- 2026-10-04: Phase A implemented (see section 6). Not yet committed.
- 2026-10-04: the editor inspects every node (`Graph::inspect`), so Split names its outputs as soon as its input is connected.
