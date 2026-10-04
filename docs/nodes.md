# Node reference

Generated from the node definitions by `cargo xtask docs`; edit the node's source file, not this page.

## Inputs

| Node | What it does |
|---|---|
| [Audio](#audio_input) | The audio track, one frame's worth per block, -1 to 1 |
| [Video](#video_input) | The video as RGB, 0 to 1 |

### `audio_input`

**Audio**: The audio track, one frame's worth per block, -1 to 1

**Outputs**

- `out` (audio): The audio, one frame's worth per block, from -1 to 1

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `source` (Source) | `audio` | text | no | Name of the host-supplied audio signal |

### `video_input`

**Video**: The video as RGB, 0 to 1

**Outputs**

- `out` (RGB video): The video, as RGB from 0 to 1

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `source` (Source) | `video` | text | no | Name of the host-supplied video signal |

## Channels

| Node | What it does |
|---|---|
| [Combine](#combine) | Separate R, G and B signals into RGB |
| [Interleave](#interleave) | RGB as one mono carrier, three times as wide (R, G, B, R, G, B, …) |
| [Pack](#pack) | A packed mono carrier back into RGB |
| [Split](#split) | RGB into separate R, G and B signals |

### `combine`

**Combine**: Separate R, G and B signals into RGB

**Inputs**

- `r` (main, required): The red channel
- `g` (required): The green channel
- `b` (required): The blue channel

**Outputs**

- `out` (RGB video): RGB video

### `interleave`

**Interleave**: RGB as one mono carrier, three times as wide (R, G, B, R, G, B, …)

**Inputs**

- `in` (main, required): RGB video to pack

**Outputs**

- `out` (RGB video): The channels in sequence, as one mono signal three times as wide

### `pack`

**Pack**: A packed mono carrier back into RGB

**Inputs**

- `in` (main, required): A mono signal three times as wide as the picture, as Interleave makes

**Outputs**

- `out` (RGB video): RGB video

### `split`

**Split**: RGB into separate R, G and B signals

**Inputs**

- `in` (main, required): RGB video to take apart

**Outputs**

- `r` (red channel): The red channel
- `g` (green channel): The green channel
- `b` (blue channel): The blue channel

## Conversion

| Node | What it does |
|---|---|
| [Audio to Video](#to_video) | Audio's -1 to 1 back to video's 0 to 1, as read from an 8-bit file |
| [Video to Audio](#to_audio) | Video's 0 to 1 as audio's -1 to 1, as written to an 8-bit file |

### `to_video`

**Audio to Video**: Audio's -1 to 1 back to video's 0 to 1, as read from an 8-bit file

**Inputs**

- `in` (main, required): Audio, with values from -1 to 1

**Outputs**

- `out` (as video): The same samples as video, from 0 to 1

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `mapping` (Mapping) | `accurate` | `accurate`, `bugged` | no | accurate maps black to -1 and white to 1; bugged reproduces the signed/unsigned misread, wrapping at mid-gray |

### `to_audio`

**Video to Audio**: Video's 0 to 1 as audio's -1 to 1, as written to an 8-bit file

**Inputs**

- `in` (main, required): Video, with values from 0 to 1

**Outputs**

- `out` (as audio): The same samples as audio, from -1 to 1

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `mapping` (Mapping) | `accurate` | `accurate`, `bugged` | no | accurate maps black to -1 and white to 1; bugged reproduces the signed/unsigned misread, wrapping at mid-gray |

## Effects

| Node | What it does |
|---|---|
| [Amplitude Modulation](#am) | Scales the carrier by the modulator |
| [Bit Crush](#bitcrush) | Reduces bit depth, posterizing the image |
| [Compressor](#compressor) | Turns loud parts down, following the input or a sidechain |
| [Delay](#delay) | Delays the signal by rows or frames; modulating the time bends rows into waves |
| [Distortion](#distortion) | Drives the signal into a waveshaper: soft, hard, folding or wrapping |
| [Gate](#gate) | Silences the signal while it, or a sidechain, is quiet |
| [Low Pass](#lowpass) | Smooths the signal along rows, a horizontal blur |
| [Three-Band Split](#three_band) | Low, mid and high frequency bands that add back up to the input |

### `am`

**Amplitude Modulation**: Scales the carrier by the modulator

Can process R, G and B separately.

**Inputs**

- `carrier` (main, required): The signal that gets scaled
- `modulator` (required): The signal that scales the carrier

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `depth` (Depth) | 1 | -10 to 10 (up to -∞ to ∞) | yes | How strongly the modulator scales the carrier: carrier × (1 + depth × modulator) |

### `bitcrush`

**Bit Crush**: Reduces bit depth, posterizing the image

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `bits` (Bits) | 4 bits | 1 to 24 | yes | Bit depth; fewer bits means fewer levels |

### `compressor`

**Compressor**: Turns loud parts down, following the input or a sidechain

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to compress
- `sidechain` (optional): A signal whose level drives the compression instead of the input's own

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `threshold` (Threshold) | -18 dB | -60 to 0 (up to -200 to 60) | yes | Level above which the signal is turned down |
| `ratio` (Ratio) | 4 | 1 to 20 (up to 1 to 1000) | yes | How much is taken off above the threshold: 4 lets 1 dB through for every 4 dB over |
| `attack` (Attack) | 10 ms | 0.01 to 1000 (up to 0 to 1000000) | yes | How quickly the compressor turns the signal down once it goes over |
| `release` (Release) | 100 ms | 0.1 to 5000 (up to 0 to 1000000) | yes | How quickly it lets go once the signal falls back |
| `knee` (Knee) | 6 dB | 0 to 24 (up to 0 to 100) | yes | Width of the soft transition around the threshold; 0 is a hard knee |
| `makeup` (Makeup) | 0 dB | -24 to 24 (up to -96 to 96) | yes | Gain applied after compression |

### `delay`

**Delay**: Delays the signal by rows or frames; modulating the time bends rows into waves

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `time` (Time) | 0.05 | 0.001 to 100 (up to 0 to 1000) | yes | Delay length, in rows or frames. Small fractions of a row give the finest waves |
| `unit` (Unit) | `rows` | `rows`, `frames` | no | Unit for the time |
| `feedback` (Feedback) | 0 | 0 to 0.99 | yes | How much of the delayed signal is fed back in |
| `mix` (Mix) | 1 | 0 to 1 | yes | 0 is the dry input, 1 is only the delayed signal |

### `distortion`

**Distortion**: Drives the signal into a waveshaper: soft, hard, folding or wrapping

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `shape` (Shape) | `soft` | `soft`, `hard`, `fold`, `wrap` | no | soft rounds off, hard clips flat, fold reflects loud parts back, wrap jumps from top to bottom |
| `drive` (Drive) | 12 dB | 0 to 48 (up to -96 to 96) | yes | Gain before shaping; more drive, more distortion |
| `bias` (Bias) | 0 | -1 to 1 (up to -100 to 100) | yes | Offset added before shaping, for uneven distortion |
| `mix` (Mix) | 1 | 0 to 1 | yes | 0 is the dry input, 1 is only the distorted signal |

### `gate`

**Gate**: Silences the signal while it, or a sidechain, is quiet

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to gate
- `sidechain` (optional): A signal whose level opens the gate instead of the input's own

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `threshold` (Threshold) | -40 dB | -80 to 0 (up to -200 to 60) | yes | Level the signal must reach to open the gate |
| `attack` (Attack) | 1 ms | 0.01 to 1000 (up to 0 to 1000000) | yes | How quickly the gate opens |
| `hold` (Hold) | 50 ms | 0 to 5000 (up to 0 to 1000000) | yes | How long the gate stays open after the signal drops below the threshold |
| `release` (Release) | 100 ms | 0.1 to 5000 (up to 0 to 1000000) | yes | How quickly the gate closes |
| `range` (Range) | -80 dB | -80 to 0 | yes | How far a closed gate turns the signal down; -80 dB is silence |

### `lowpass`

**Low Pass**: Smooths the signal along rows, a horizontal blur

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `cutoff` (Cutoff) | 40 cycles/row | 0.01 to 100000 (up to 0.000001 to 1000000000) | yes, in octaves | Cutoff in cycles per row; lower is smoother |

### `three_band`

**Three-Band Split**: Low, mid and high frequency bands that add back up to the input

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `low` (low band): Everything below the low crossover
- `mid` (mid band): What is left between the crossovers
- `high` (high band): Everything above the high crossover

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `low_hz` (Low / mid) | 250 Hz | 1 to 100000 (up to 0.001 to 1000000000) | no | Crossover between the low and mid bands, in Hz |
| `high_hz` (Mid / high) | 4000 Hz | 1 to 100000 (up to 0.001 to 1000000000) | no | Crossover between the mid and high bands, in Hz |

## Output

| Node | What it does |
|---|---|
| [Output](#output) | The rendered result: RGB, or mono shown as grayscale |

### `output`

**Output**: The rendered result: RGB, or mono shown as grayscale

**Inputs**

- `in` (main, required): The picture to render: RGB, or mono for grayscale

**Outputs**

- `out`: The rendered picture

