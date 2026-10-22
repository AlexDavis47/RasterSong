# Benchmarks

Recorded with `cargo bench` (release build, single thread). Re-run after performance work and add a new
section rather than overwriting, so changes stay visible.

```sh
cargo bench -p rastersong-graph   # per-node throughput and whole example graphs at 1080p
cargo bench -p rastersong-media   # decode and audio load (needs `cargo xtask fixtures`)
```

## 2026-10-03: Phase 3 baseline

AMD Ryzen 9 3900X (12 cores, one used), 64 GB RAM, Windows 11. No SIMD work or parallelism yet.

These node rows come from the Phase 3 bench, which had modulated and feedback variants. Today's `cargo bench`
makes one entry per node kind from `NodeKind::BENCH`, and `lowpass` is now `filter` at 6 dB/oct.

### Nodes: one 1080p channel (2.07 M samples)

| Node | Time per frame | Throughput |
|---|---|---|
| `am` | 1.2 ms | 1.74 G samples/s |
| `lowpass` | 4.7 ms | 439 M samples/s |
| `three_band` | 6.7 ms | 311 M samples/s |
| `bitcrush` (modulated) | 11.3 ms | 183 M samples/s |
| `delay` | 11.4 ms | 182 M samples/s |
| `delay` (modulated) | 13.9 ms | 149 M samples/s |
| `delay` (feedback) | 14.2 ms | 146 M samples/s |
| `lowpass` (modulated) | 28.4 ms | 73 M samples/s |

### Example graphs: 1080p RGB frames

| Graph | Time per frame | Frames per second |
|---|---|---|
| `am_bands` | 32 ms | 31 |
| `packed_crush` | 102 ms | 9.8 |
| `bass_wave` | 169 ms | 5.9 |

### Decode

| Benchmark | Result |
|---|---|
| Open + sequential decode, `bframes.mp4` (320×240 MPEG-4) | 3,470 frames/s |
| Random access, same file (every request seeks) | 996 frames/s |
| Load audio, `music.wav` (2 s, 44.1 kHz) | 10 ms |

### Notes

- `am_bands` already renders 1080p in real time on one core. The heavier graphs are the target for Phase 5:
  the delay line does two modulo operations per sample, and modulated `lowpass` and `bitcrush` call `exp`/`exp2`
  per sample. Branch- and channel-level parallelism (three channels in `bass_wave`) is also unused.
- At ½ preview scale the graph cost drops about 4×, at ¼ about 16×.

## 2026-10-22: Export encoders

AMD Ryzen 9 3900X (24 threads), Windows 11, release build. 60 frames of 1080p30 with noise, video only, through
`MediaWriter` (frame conversion included). H.264 went through the GPU encoder.

| Format | One encoder thread (before) | Automatic threads (after) |
|---|---|---|
| H.264 (GPU) | 195 fps | 208 fps |
| ProRes HQ (`prores_ks`) | 1.5 fps | 16 fps |
| FFV1 | 8.7 fps | 50 fps |

### Notes

- Converting RGB frames to the encoder's pixel format costs about 5 ms per frame; the encoders were the cost.
- `prores_ks` searches for a quantizer per slice and stays slow on detailed frames even threaded. FFmpeg's
  `prores` encoder wrote the same frames at about 180 fps (measured with the `ffmpeg` program).

## 2026-10-22: Where an export spends its time

Same machine. 300 frames of 1080p30 H.264 (noisy test pattern) and a 10 s song through the example graphs, written
with `rastersong-cli render ... --trace` (FFV1) and profiled with samply (120 frames of `bass_wave`). See
[Profiling](development.md#profiling).

| Graph | MP4 (GPU H.264) | MKV (FFV1) |
|---|---|---|
| `am_bands` | 6.6 fps | 7.8 fps |
| `bass_wave` | 4.1 fps | 3.9 fps |

### Share of the export, from the trace

| Stage | `am_bands` | `bass_wave` |
|---|---|---|
| Graph nodes (`graph`) | 50% | 82% |
| … inside nodes' `process` | 28% | 47% |
| … resampling inputs and modulated parameters | 21% | 35% |
| Composition copies (`compose` self) | 9% | 4% |
| `to_rgb8` | 20% | 5% |
| Decode, convert and `to_signal` | 6% | 3% |
| Encode (`write`, FFV1) | 15% | 5% |

Heaviest nodes in `bass_wave`, per frame: `filter` 85 ms (half of it computing its modulated cutoff), each
`delay` about 30 ms, `combine` 17 ms, `split` 10 ms.

### From samply (`bass_wave`)

94% of samples are on the main thread. Its top functions: `dsp::resample` 14%, `memmove` 11%,
`DelayLine::read` 8.5%, `exp` (modulated `filter`) 7.5%, `ParamBinding::update` 5%, `dsp::mix` 5%, `round`
and `roundf` 5%, FFmpeg (decode, scale, FFV1) about 5%.

### Notes

- With encoders threaded, the format no longer decides the speed; the graph does, on one core.
- The roadmap's [Performance](roadmap.md#performance) section lists the follow-ups.

## 2026-10-22: How far more cores can go

Same machine (2 × 32 GB DDR4-3200, dual channel). Before any threading work, two measurements of the ceiling:
copies of `rastersong-cli render` run at once (each one thread of graph work, the same 1080p30 noisy H.264
input and a 10 s song, GPU H.264 out), and a probe that streams 1080p-RGB-sized `f32` signals the way a cheap
node pass does (`out = a * k + b`: two reads, one write per sample) on 1 to 24 threads.

### Copies of the export at once

| Copies | `bass_wave` per copy | `bass_wave` total | `am_bands` per copy | `am_bands` total |
|---|---|---|---|---|
| 1 | 3.7 fps | 3.7 fps | 8.7 fps | 8.6 fps |
| 2 | 3.4 fps | 6.7 fps | 7.5 fps | 14.9 fps |
| 4 | 3.1 fps | 12.0 fps | 5.7 fps | 22.5 fps |
| 8 | 2.3 fps | 18.4 fps | 3.2 fps | 23.7 fps |
| 12 | 1.7 fps | 19.8 fps | 2.1 fps | 24.4 fps |

### Streaming probe

| Threads | 1 | 2 | 4 | 8 | 12 | 24 |
|---|---|---|---|---|---|---|
| GB/s moved | 18.9 | 23.2 | 21.3 | 20.2 | 21.1 | 23.4 |

### Notes

- **Memory bandwidth is the ceiling, and it is low.** One thread streaming whole frames already uses about 80%
  of what the machine can move; more threads add almost nothing. Each 1080p signal is 24 MB, larger than a
  core complex's 16 MB of L3, so every node pass goes to RAM.
- Independent renders level off at about 20 fps (`bass_wave`, 5.4×) and 24 fps (`am_bands`, 2.8×). At ~23 GB/s
  that is roughly 1 GB of memory traffic per frame for either graph. `bass_wave` scales further only because more
  of its time is arithmetic (`exp`, the delay's modulo) rather than streaming.
- So splitting frames into chunks across renderers would not beat threading within a frame here: both hit the
  same wall, and only the within-frame kind stays bit-exact. What raises the ceiling is moving fewer bytes per
  frame: modulated parameters at the modulator's resolution, fewer whole-frame copies, and later working on bands
  of rows that stay in cache across several nodes.
- Each copy also decodes and encodes, so the totals slightly understate what the graphs alone could do.
