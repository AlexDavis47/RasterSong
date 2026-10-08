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
