# Node reference

Generated from the node definitions by `cargo xtask docs`; edit the node's source file, not this page.

## Inputs

| Node | What it does |
|---|---|
| [Audio In](#audio_input) | The graph's main sound: what is below its item on the timeline, one frame's worth per block, -1 to 1, channels interleaved |
| [Audio Source](#audio_source) | Sound from elsewhere in the project: the audio mix, one audio track on its own, or a graph track's output |
| [Video In](#video_input) | The graph's main picture: what is below its item on the timeline, as RGB from 0 to 1 |
| [Video Source](#video_source) | A picture from elsewhere in the project: the video mix, one video track on its own, or a graph track's output |

### `audio_input`

**Audio In**: The graph's main sound: what is below its item on the timeline, one frame's worth per block, -1 to 1, channels interleaved

Every graph has one, which can't be removed. Below the lowest graph track it is the audio mix; above another graph track it is that track's sound where one of its items plays. Use an Audio Source to read anything else.

**Outputs**

- `out` (audio, -1 to 1): The sound, one frame's worth per block, from -1 to 1; stereo comes as L, R, L, R, …

### `audio_source`

**Audio Source**: Sound from elsewhere in the project: the audio mix, one audio track on its own, or a graph track's output

A track is read before its volume and mute, so a muted kick can still drive an effect. A graph track must be below the one this graph plays on, or it reads silence; where none of its items plays it reads what is below that track. Nothing picked reads silence.

**Outputs**

- `out` (audio, -1 to 1): The sound, one frame's worth per block, from -1 to 1; stereo comes as L, R, L, R, …

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `source` (Source) | `` | text | no | What this node reads: the audio mix, a track's stem, or a graph track below this one |
| `output` (Output) | `` | text | no | Which output of the graph track to read: its main Audio, or one of its graphs' Named Outputs |

### `video_input`

**Video In**: The graph's main picture: what is below its item on the timeline, as RGB from 0 to 1

Every graph has one, which can't be removed. Below the lowest graph track it is the video mix; above another graph track it is that track's picture where one of its items plays. Use a Video Source to read anything else.

**Outputs**

- `out` (video, 0 to 1): The picture, as RGB from 0 to 1

### `video_source`

**Video Source**: A picture from elsewhere in the project: the video mix, one video track on its own, or a graph track's output

A track is read before its mute. A graph track must be below the one this graph plays on, or it reads black; where none of its items plays it reads what is below that track. Nothing picked reads black.

**Outputs**

- `out` (video, 0 to 1): The picture, as RGB from 0 to 1

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `source` (Source) | `` | text | no | What this node reads: the video mix, a track's stem, or a graph track below this one |
| `output` (Output) | `` | text | no | Which output of the graph track to read: its main Video, or one of its graphs' Named Outputs |

## Generators

| Node | What it does |
|---|---|
| [Beat](#beat) | A 0 to 1 signal locked to the project's beats, restarting every note length: phase, decay, pulse or steps |
| [Constant](#constant) | The same value in every sample: a flat colour, or silence |
| [Graph Progress](#graph_progress) | How far through its graph item the render is: 0 at the item's start, rising evenly to 1 at its end |
| [Noise](#noise) | Random values in a chosen colour: grain in video, hiss in audio |
| [Oscillator](#oscillator) | A sine, triangle, square or saw wave: stripes in video, a tone in audio |

### `beat`

**Beat**: A 0 to 1 signal locked to the project's beats, restarting every note length: phase, decay, pulse or steps

**Outputs**

- `out`: The beat-locked signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `numerator` (Numerator) | 1 | 1 to 32 (up to 1 to 1024) | yes | How many notes of the denominator's length each cycle lasts, as in a time signature: 1/4 is a beat, 4/4 four beats, 16/4 sixteen |
| `denominator` (Denominator) | 4 | 1 to 32 (up to 1 to 1024) | yes | The note the numerator counts, as a fraction of a whole note: 4 is a quarter note (a beat), 8 an eighth, 16 a sixteenth |
| `shape` (Shape) | `decay` | `phase`, `decay`, `pulse`, `step` | no | phase rises 0 to 1, decay falls 1 to 0, pulse is on for the width, step climbs in stairs |
| `width` (Width) | 0.25 | 0 to 1 | yes | For the pulse shape, the fraction of each cycle it stays on. Used when `shape` is `pulse`. |
| `steps` (Steps) | 4 | 1 to 32 (up to 1 to 1024) | yes | For the step shape, how many stairs each cycle climbs. Used when `shape` is `step`. |

### `constant`

**Constant**: The same value in every sample: a flat colour, or silence

**Outputs**

- `out`: The constant signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `value` (Value) | 0 | -1 to 1 (up to -10 to 10) | yes | The value of every sample |

### `graph_progress`

**Graph Progress**: How far through its graph item the render is: 0 at the item's start, rising evenly to 1 at its end

Stretching the item makes it rise more slowly. It is 0 during pre-roll. A graph rendered over the whole project goes from 0 to 1 over the project.

**Outputs**

- `out`: The item's progress, 0 to 1

### `noise`

**Noise**: Random values in a chosen colour: grain in video, hiss in audio

**Outputs**

- `out`: The noise

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `color` (Color) | `white` | `white`, `pink`, `brown`, `blue`, `violet` | no | How the noise is spread over frequencies: white is sharp grain, brown is slow drift, violet is the finest grain |
| `seed` (Seed) | 0 | 0 to 999 (up to 0 to 4000000000) | yes | Picks which noise; the same seed always gives the same noise |
| `amplitude` (Amplitude) | 0.5 | 0 to 1 (up to -10 to 10) | yes | Scales the noise, which spans -1 to 1 before the offset is added |
| `offset` (Offset) | 0.5 | -1 to 1 (up to -10 to 10) | yes | Added to the noise: 0.5 with amplitude 0.5 fills the video range 0 to 1, 0 suits audio |

### `oscillator`

**Oscillator**: A sine, triangle, square or saw wave: stripes in video, a tone in audio

**Outputs**

- `out`: The wave

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `wave` (Wave) | `sine` | `sine`, `triangle`, `square`, `saw` | no | The shape of one cycle |
| `freq` (Frequency) | 8 `row` | 0.01 to 100 (up to 0 to 1000000) | yes | Cycles per unit of time or space: how many stripes fit in a row, or how high the tone is. Cycles per unit in `freq.unit`, or one cycle every so many units in `freq.every`. |
| `phase` (Phase) | 0 | 0 to 1 (up to -1000 to 1000) | yes | Where in the cycle the wave starts, as a fraction of a cycle |
| `amplitude` (Amplitude) | 0.5 | 0 to 1 (up to -10 to 10) | yes | Half the peak-to-peak height; with the offset it places the wave in the signal's range |
| `offset` (Offset) | 0.5 | -1 to 1 (up to -10 to 10) | yes | Added to the wave: 0.5 with amplitude 0.5 fills the video range 0 to 1, 0 suits audio |
| `pulse_width` (Pulse width) | 0.5 | 0 to 1 | yes | For the square wave, the fraction of the cycle it stays high. Used when `wave` is `square`. |

## Channels

| Node | What it does |
|---|---|
| [Combine Channels](#combine) | Separate signals into one interleaved signal: R, G, B into RGB, or L, R into stereo |
| [Flip](#flip) | Mirrors or turns over the picture |
| [Interleave](#interleave) | Channels as one mono carrier, as many times as wide (R, G, B, R, G, B, …) |
| [Pack](#pack) | A packed mono carrier back into channels: RGB, stereo, … |
| [Resample](#resample) | Resizes the picture; effects after it work at the new resolution |
| [Split Channels](#split) | Each channel of an interleaved signal on its own: R, G, B of video or L, R of stereo |
| [Stretch to Match](#stretch) | A signal stretched to the length and layout of another |
| [Transpose](#transpose) | Swaps rows and columns: the picture's width and height trade places |

### `combine`

**Combine Channels**: Separate signals into one interleaved signal: R, G, B into RGB, or L, R into stereo

Each connected input becomes one channel of the output, in order. The first input sets the size; the others are stretched to it. Three channels of video make RGB; two of audio make stereo.

**Inputs**

- `c1` (main, required): Channel 1: red, or left. Sets the size
- `c2` (optional): Channel 2: green, or right
- `c3` (optional): Channel 3: blue
- `c4` (optional): Channel 4
- `c5` (optional): Channel 5
- `c6` (optional): Channel 6
- `c7` (optional): Channel 7
- `c8` (optional): Channel 8

**Outputs**

- `out` (): The channels interleaved, pixel by pixel

### `flip`

**Flip**: Mirrors or turns over the picture

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `mode` (Mode) | `horizontal` | `horizontal`, `vertical`, `reverse` | no | horizontal mirrors each row, vertical turns the rows upside down, reverse does both, |

### `interleave`

**Interleave**: Channels as one mono carrier, as many times as wide (R, G, B, R, G, B, …)

**Inputs**

- `in` (main, required): An interleaved signal, such as RGB video, to flatten

**Outputs**

- `out`: The channels in sequence, as one mono signal as many times as wide as there are channels

### `pack`

**Pack**: A packed mono carrier back into channels: RGB, stereo, …

**Inputs**

- `in` (main, required): A mono signal as many times as wide as the picture as there are channels, as Interleave makes

**Outputs**

- `out`: The interleaved signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `channels` (Channels) | 3 | 1 to 8 (up to 1 to 64) | no: The number of channels changes the signal's layout, which the graph is compiled for and can't change per sample | How many channels each pixel gets: 3 for RGB, 2 for stereo |

### `resample`

**Resample**: Resizes the picture; effects after it work at the new resolution

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `width` (Width) | 0 | 0 to 4096 (up to 0 to 16384) | no: The size changes the signal's layout, which the graph is compiled for and can't change per sample | New width in pixels; 0 keeps the input's width |
| `height` (Height) | 0 | 0 to 4096 (up to 0 to 16384) | no: The size changes the signal's layout, which the graph is compiled for and can't change per sample | New height in pixels; 0 keeps the input's height |
| `method` (Method) | `linear` | `nearest`, `linear` | no | nearest picks the closest pixel, linear blends the four closest |

### `split`

**Split Channels**: Each channel of an interleaved signal on its own: R, G, B of video or L, R of stereo

One output per channel of the input: three for RGB video, two for stereo audio. Outputs past the input's channel count carry silence.

**Inputs**

- `in` (main, required): The interleaved signal to take apart

**Outputs**

- `c1`: Channel 1: red, or left
- `c2`: Channel 2: green, or right
- `c3`: Channel 3: blue
- `c4`: Channel 4
- `c5`: Channel 5
- `c6`: Channel 6
- `c7`: Channel 7
- `c8`: Channel 8

### `stretch`

**Stretch to Match**: A signal stretched to the length and layout of another

The output has `like`'s size and layout and `in`'s values: audio stretched over a picture, or a picture squeezed into an audio block. With pixel grouping (the default), a mono signal stretched over RGB moves each pixel's channels together; per sample, it is spread over every channel value.

**Inputs**

- `like` (main, required): The signal whose size and layout the result takes
- `in` (required): The signal to stretch

**Outputs**

- `out`: `in` at the size of `like`

### `transpose`

**Transpose**: Swaps rows and columns: the picture's width and height trade places

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

## Conversion

| Node | What it does |
|---|---|
| [Audio to Video](#to_video) | Audio's -1 to 1 back to video's 0 to 1, as read from an 8-bit file |
| [Relabel](#relabel) | Changes what a signal is said to be (kind, channels, range), not its samples |
| [Video to Audio](#to_audio) | Video's 0 to 1 as audio's -1 to 1, as written to an 8-bit file |

### `to_video`

**Audio to Video**: Audio's -1 to 1 back to video's 0 to 1, as read from an 8-bit file

**Inputs**

- `in` (main, required): Audio, with values from -1 to 1

**Outputs**

- `out` (video, 0 to 1): The same samples as video, from 0 to 1

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `mapping` (Mapping) | `accurate` | `accurate`, `bugged` | no | accurate maps black to -1 and white to 1; bugged reproduces the signed/unsigned misread, wrapping at mid-gray |

### `relabel`

**Relabel**: Changes what a signal is said to be (kind, channels, range), not its samples

Signals carry a tag (video or audio, which channels, the range of values) that colours wires and drives warnings. Nothing converts a signal because of its tag; this node only rewrites it, for when a signal is reused on purpose as something else.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The same samples, relabelled

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `kind` (Kind) | `keep` | `keep`, `video`, `audio`, `unknown` | no | What the signal is meant to be |
| `channels` (Channels) | `keep` | `keep`, `named`, `numbered` | no | named gives the channels their usual names (RGB, stereo), numbered just counts them |
| `range` (Range) | `keep` | `keep`, `0 to 1`, `-1 to 1`, `unknown` | no | The range the values are meant to span |
| `part` (Part) | `keep` | `keep`, `whole` | no | whole stops treating the signal as one channel or band of another |

### `to_audio`

**Video to Audio**: Video's 0 to 1 as audio's -1 to 1, as written to an 8-bit file

**Inputs**

- `in` (main, required): Video, with values from 0 to 1

**Outputs**

- `out` (audio, -1 to 1): The same samples as audio, from -1 to 1

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `mapping` (Mapping) | `accurate` | `accurate`, `bugged` | no | accurate maps black to -1 and white to 1; bugged reproduces the signed/unsigned misread, wrapping at mid-gray |

## Effects

| Node | What it does |
|---|---|
| [Amplitude Modulation](#am) | Scales the carrier by the modulator |
| [Bit Crush](#bitcrush) | Reduces bit depth, posterizing the image |
| [Blend](#blend) | Combines two signals: add, multiply, screen, difference and more |
| [Chorus](#chorus) | Thickens the signal with delayed copies; modulate the time to make them drift |
| [Clamp](#clamp) | Limits every sample to a range |
| [Compressor](#compressor) | Turns loud parts down, following the input or a sidechain |
| [Crossfade](#crossfade) | Fades or cuts between two signals |
| [DC Filter](#dc_filter) | Removes the constant offset of a signal with a very slow high pass |
| [Delay](#delay) | Delays the signal by rows or frames; modulating the time bends rows into waves |
| [Distortion](#distortion) | Drives the signal into a waveshaper: soft, hard, folding or wrapping |
| [Dynamic EQ Band](#dynamic_eq) | An equalizer band whose gain follows the level of the signal: tame a range only when it gets loud |
| [EQ Band](#equalizer) | One equalizer band: a peak, shelf, cut, notch or band pass. Chain several for a full equalizer |
| [Envelope](#envelope) | Follows how strong the signal is, as a smooth curve from 0 up (Slew limits the signal itself instead) |
| [FM](#fm) | Bends the carrier by reading it through a delay the modulator controls |
| [Filter](#filter) | A resonant low, high, band or all pass, tilt or comb filter |
| [Flanger](#flanger) | A short delay with feedback that combs the signal; modulate the time to sweep it |
| [Frequency Shifter](#frequency_shifter) | Moves every frequency up or down by a fixed amount, giving inharmonic tones |
| [Gain](#gain) | Makes the signal louder or quieter, in decibels |
| [Gate](#gate) | Silences the signal while it, or a sidechain, is quiet |
| [Invert](#invert) | Flips the signal: negative for video, upside down for audio |
| [Limiter](#limiter) | Stops the signal from passing a ceiling by pulling the gain down |
| [Offset](#offset) | Adds a constant to every sample |
| [Phaser](#phaser) | Sweeps notches through the signal with allpass filters; modulate the frequency |
| [Quantize](#quantize) | Snaps every sample to a grid of evenly spaced levels |
| [Rectify](#rectify) | Folds or drops one half of the wave around a centre value |
| [Remap](#remap) | Stretches one range of values onto another: contrast, inversion, levels |
| [Reverb](#reverb) | A dense decaying wash of echoes |
| [Ring Modulation](#ring_mod) | Multiplies the carrier by the modulator, leaving sum and difference tones |
| [Sample & Hold](#sample_hold) | Holds each sampled value for a while and optionally rounds it to a few levels |
| [Slew](#slew) | Limits how fast the signal itself can rise and fall, turning jumps into ramps (Envelope follows its strength instead) |
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

### `blend`

**Blend**: Combines two signals: add, multiply, screen, difference and more

Can process R, G and B separately.

**Inputs**

- `a` (main, required): The base signal
- `b` (required): The signal blended onto the base

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `mode` (Mode) | `add` | `add`, `subtract`, `multiply`, `screen`, `difference`, `min`, `max`, `average`, `overlay` | no | How `a` and `b` are combined |
| `amount` (Amount) | 1 | 0 to 1 | yes | 0 passes `a` through, 1 is the full blend |

### `chorus`

**Chorus**: Thickens the signal with delayed copies; modulate the time to make them drift

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `time` (Time) | 20 `ms` | 0 to 50 (up to 0 to 1000) | yes | Delay of the copies; wire an oscillator in here to make them drift. Unit in `time.unit`. |
| `voices` (Voices) | 2 | 1 to 4 | yes | How many delayed copies are mixed in |
| `spread` (Spread) | 0.3 | 0 to 0.6 | yes | How far apart the copies' delays are, as a fraction of the time |
| `mix` (Mix) | 1 | 0 to 1 | yes | 0 is the dry input, 1 is only the processed signal; in between crossfades the two |

### `clamp`

**Clamp**: Limits every sample to a range

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `min` (Min) | 0 | -1 to 1 (up to -100 to 100) | yes | Samples below this are raised to it |
| `max` (Max) | 1 | -1 to 1 (up to -100 to 100) | yes | Samples above this are lowered to it |

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
| `attack` (Attack) | 10 `ms` | 0.01 to 1000 (up to 0 to 1000000) | yes | How quickly the compressor turns the signal down once it goes over. Unit in `attack.unit`. |
| `release` (Release) | 100 `ms` | 0.1 to 5000 (up to 0 to 1000000) | yes | How quickly it lets go once the signal falls back. Unit in `release.unit`. |
| `knee` (Knee) | 6 dB | 0 to 24 (up to 0 to 100) | yes | Width of the soft transition around the threshold; 0 is a hard knee |
| `makeup` (Makeup) | 0 dB | -24 to 24 (up to -96 to 96) | yes | Gain applied after compression |

### `crossfade`

**Crossfade**: Fades or cuts between two signals

Can process R, G and B separately.

**Inputs**

- `a` (main, required): The signal heard at position 0
- `b` (required): The signal heard at position 1

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `curve` (Curve) | `fade` | `fade`, `switch` | no | fade blends smoothly, switch cuts from `a` to `b` at the middle |
| `position` (Position) | 0.5 | 0 to 1 | yes | 0 is only `a`, 1 is only `b` |

### `dc_filter`

**DC Filter**: Removes the constant offset of a signal with a very slow high pass

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The signal with its offset removed

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `cutoff` (Cutoff) | 10 `second` | 0.1 to 100 (up to 0.000001 to 1000000000) | yes | Below this frequency the signal is removed. Keep it far below anything you want to keep; the default suits audio. Cycles per unit in `cutoff.unit`, or one cycle every so many units in `cutoff.every`. |

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
| `time` (Time) | 0.05 `row` | 0 to 100 (up to 0 to 1000) | yes | Delay length, in rows or frames. Small fractions of a row give the finest waves. Unit in `time.unit`. |
| `feedback` (Feedback) | 0 | 0 to 0.99 | yes | How much of the delayed signal is fed back in |
| `mix` (Mix) | 1 | 0 to 1 | yes | 0 is the dry input, 1 is only the processed signal; in between crossfades the two |

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
| `mix` (Mix) | 1 | 0 to 1 | yes | 0 is the dry input, 1 is only the processed signal; in between crossfades the two |

### `dynamic_eq`

**Dynamic EQ Band**: An equalizer band whose gain follows the level of the signal: tame a range only when it gets loud

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process
- `sidechain` (optional): Optional: a signal whose level drives the band instead of the main one

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `band` (Type) | `peak` | `peak`, `low_shelf`, `high_shelf` | no | peak, low_shelf or high_shelf |
| `frequency` (Frequency) | 1000 `second` | 20 to 20000 (up to 0.000001 to 1000000000) | yes | Centre of a peak, or the corner of a shelf. Cycles per unit in `frequency.unit`, or one cycle every so many units in `frequency.every`. |
| `q` (Q) | 1 | 0.1 to 20 (up to 0.05 to 100) | yes | Width of the band: higher is narrower |
| `gain` (Gain) | -9 dB | -24 to 24 (up to -48 to 48) | yes | The most the band moves, in decibels. Negative turns the band down when the signal is loud; positive turns it up |
| `threshold` (Threshold) | -18 dB | -60 to 0 (up to -200 to 60) | yes | Level above which the band starts to move |
| `ratio` (Ratio) | 4 | 1 to 20 (up to 1 to 1000) | yes | How quickly the band moves toward its gain as the level rises past the threshold |
| `attack` (Attack) | 0.01 `second` | 0.0001 to 1 (up to 0 to 1000000) | yes | How fast the band moves away from flat. Unit in `attack.unit`. |
| `release` (Release) | 0.1 `second` | 0.001 to 5 (up to 0 to 1000000) | yes | How fast the band returns to flat. Unit in `release.unit`. |

### `equalizer`

**EQ Band**: One equalizer band: a peak, shelf, cut, notch or band pass. Chain several for a full equalizer

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `band` (Type) | `peak` | `peak`, `low_shelf`, `high_shelf`, `low_cut`, `high_cut`, `notch`, `band_pass` | no | What the band does: peak, low_shelf and high_shelf boost or cut by the gain; low_cut and high_cut remove one side; notch removes a narrow range; band_pass keeps only a range |
| `frequency` (Frequency) | 30 `row` | 0.01 to 500 (up to 0.000001 to 1000000000) | yes | Centre of a peak, notch or band pass, or the corner of a shelf or cut. Cycles per unit in `frequency.unit`, or one cycle every so many units in `frequency.every`. |
| `q` (Q) | 0.707 | 0.1 to 20 (up to 0.05 to 100) | yes | Width of the band: higher is narrower. On a cut, higher adds a resonant peak at the corner; 0.707 is flat |
| `gain` (Gain) | 0 dB | -24 to 24 (up to -48 to 48) | yes | Boost or cut in decibels. Used when `band` is `peak` or `low_shelf` or `high_shelf`. |
| `slope` (Slope (dB/oct)) | `12` | `12`, `24`, `48` | no | How sharply a cut falls off past its corner. Used when `band` is `low_cut` or `high_cut`. |

### `envelope`

**Envelope**: Follows how strong the signal is, as a smooth curve from 0 up (Slew limits the signal itself instead)

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `detector` (Detector) | `peak` | `peak`, `rms` | no | peak follows each sample's magnitude, rms follows average power and is smoother |
| `attack` (Attack) | 5 `ms` | 0 to 1000 (up to 0 to 1000000) | yes | How quickly the output rises when the input gets stronger. Unit in `attack.unit`. |
| `release` (Release) | 50 `ms` | 0 to 5000 (up to 0 to 1000000) | yes | How quickly the output falls when the input gets weaker. Unit in `release.unit`. |

### `fm`

**FM**: Bends the carrier by reading it through a delay the modulator controls

Can process R, G and B separately.

**Inputs**

- `carrier` (main, required): The signal that gets bent
- `modulator` (required): The signal that sets how far back the carrier is read

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `depth` (Depth) | 0.5 `row` | 0 to 10 (up to 0 to 1000) | yes | How far the modulator pushes the carrier, in rows or frames: the modulator at +1 reads twice this far back, at -1 not at all. Unit in `depth.unit`. |
| `mix` (Mix) | 1 | 0 to 1 | yes | 0 is the dry input, 1 is only the processed signal; in between crossfades the two |

### `filter`

**Filter**: A resonant low, high, band or all pass, tilt or comb filter

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `response` (Type) | `lowpass` | `lowpass`, `highpass`, `bandpass`, `allpass`, `tilt`, `comb` | no | lowpass, highpass, bandpass, allpass, tilt (gain dB of low-versus-high balance) or comb (echo every cutoff cycle) |
| `slope` (Slope (dB/oct)) | `12` | `6`, `12`, `24`, `48` | no | How sharply the cut falls off past the cutoff: 6 is a gentle one-pole roll-off, 48 a brick wall. Used when `response` is `lowpass` or `highpass`. |
| `cutoff` (Cutoff) | 40 `row` | 0.01 to 200 (up to 0.000001 to 1000000000) | yes | Frequency of the filter's corner or centre. Cycles per unit in `cutoff.unit`, or one cycle every so many units in `cutoff.every`. |
| `q` (Resonance) | 0.707 | 0.1 to 20 (up to 0.05 to 100) | yes | Sharpness: 0.707 is flat (no resonance), higher rings or narrows. For a comb, higher repeats more. The 6 dB slope has none |
| `gain` (Gain) | 0 dB | -24 to 24 (up to -48 to 48) | yes | For tilt: dB boost of lows and cut of highs (negative reverses) |

### `flanger`

**Flanger**: A short delay with feedback that combs the signal; modulate the time to sweep it

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `time` (Time) | 2 `ms` | 0 to 10 (up to 0 to 1000) | yes | Delay length; wire an oscillator in here to sweep the comb. Unit in `time.unit`. |
| `feedback` (Feedback) | 0.5 | -0.95 to 0.95 | yes | How much of the delayed signal is fed back in; negative flips its sign |
| `mix` (Mix) | 1 | 0 to 1 | yes | 0 is the dry input, 1 is only the processed signal; in between crossfades the two |

### `frequency_shifter`

**Frequency Shifter**: Moves every frequency up or down by a fixed amount, giving inharmonic tones

With Mix between 0 and 1 the shifted signal beats against the original.

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `shift` (Shift) | 100 `second` | -1000 to 1000 (up to -1000000000 to 1000000000) | yes | How far every frequency moves: positive shifts up, negative down. Cycles per unit in `shift.unit`, or one cycle every so many units in `shift.every`. |
| `mix` (Mix) | 1 | 0 to 1 | yes | 0 is the dry input, 1 is only the processed signal; in between crossfades the two |

### `gain`

**Gain**: Makes the signal louder or quieter, in decibels

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `gain` (Gain) | 0 dB | -48 to 24 (up to -120 to 120) | yes | How much louder (or brighter) the signal gets; negative is quieter, 0 changes nothing |

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
| `attack` (Attack) | 1 `ms` | 0.01 to 1000 (up to 0 to 1000000) | yes | How quickly the gate opens. Unit in `attack.unit`. |
| `hold` (Hold) | 50 `ms` | 0 to 5000 (up to 0 to 1000000) | yes | How long the gate stays open after the signal drops below the threshold. Unit in `hold.unit`. |
| `release` (Release) | 100 `ms` | 0.1 to 5000 (up to 0 to 1000000) | yes | How quickly the gate closes. Unit in `release.unit`. |
| `range` (Range) | -80 dB | -80 to 0 | yes | How far a closed gate turns the signal down; -80 dB is silence |

### `invert`

**Invert**: Flips the signal: negative for video, upside down for audio

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `mode` (Mode) | `video` | `video`, `audio` | no | video flips around the middle of 0 to 1 (1 - x), audio flips the sign (-x) |
| `amount` (Amount) | 1 | 0 to 1 | yes | 0 leaves the signal alone, 1 is fully inverted, in between fades toward it |

### `limiter`

**Limiter**: Stops the signal from passing a ceiling by pulling the gain down

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `ceiling` (Ceiling) | -6 dB | -48 to 0 (up to -120 to 24) | yes | The loudest any sample may get: 0 dB is full scale, 1.0 |
| `release` (Release) | 50 `ms` | 0 to 1000 (up to 0 to 1000000) | yes | How slowly the gain recovers after a peak; longer is smoother. Unit in `release.unit`. |

### `offset`

**Offset**: Adds a constant to every sample

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `amount` (Amount) | 0 | -1 to 1 (up to -100 to 100) | yes | Added to every sample: brightens video, shifts audio up |

### `phaser`

**Phaser**: Sweeps notches through the signal with allpass filters; modulate the frequency

Mix is fully wet by default; around 0.5 the notches are deepest, because the filtered signal then cancels the dry one.

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `stages` (Stages) | 4 | 1 to 12 | yes | How many allpass filters are chained; every two add a notch |
| `freq` (Frequency) | 1000 `second` | 20 to 5000 (up to 0.000001 to 1000000000) | yes | Where the notches sit; wire an oscillator in here to sweep them. Cycles per unit in `freq.unit`, or one cycle every so many units in `freq.every`. |
| `feedback` (Feedback) | 0.3 | -0.95 to 0.95 | yes | How much of the chain's output is fed back in, which sharpens the notches |
| `mix` (Mix) | 1 | 0 to 1 | yes | 0 is the dry input, 1 is only the processed signal; in between crossfades the two |

### `quantize`

**Quantize**: Snaps every sample to a grid of evenly spaced levels

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `step` (Step) | 0.125 | 0.001 to 1 (up to 0.000001 to 1000000) | yes | Size of one step: 0.25 gives the levels 0, 0.25, 0.5, 0.75, 1 |
| `offset` (Offset) | 0 | -1 to 1 (up to -1000000 to 1000000) | yes | Where the steps start: steps sit at offset + n × step |
| `rounding` (Rounding) | `nearest` | `nearest`, `floor`, `ceil` | no | nearest picks the closest step, floor the one below, ceil the one above |
| `mix` (Mix) | 1 | 0 to 1 | yes | 0 is the dry input, 1 is only the processed signal; in between crossfades the two |

### `rectify`

**Rectify**: Folds or drops one half of the wave around a centre value

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `kind` (Kind) | `full` | `full`, `half`, `negative` | no | full flips the negative half up, half drops it, negative keeps only the negative half |
| `center` (Center) | 0 | -1 to 1 (up to -100 to 100) | yes | The value the wave is rectified around: 0 for audio, 0.5 to fold video around mid-gray |

### `remap`

**Remap**: Stretches one range of values onto another: contrast, inversion, levels

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `in_low` (In low) | 0 | -1 to 1 (up to -100 to 100) | yes | The input value that becomes `out_low` |
| `in_high` (In high) | 1 | -1 to 1 (up to -100 to 100) | yes | The input value that becomes `out_high` |
| `out_low` (Out low) | 0 | -1 to 1 (up to -100 to 100) | yes | What `in_low` becomes |
| `out_high` (Out high) | 1 | -1 to 1 (up to -100 to 100) | yes | What `in_high` becomes |
| `outside` (Outside) | `clamp` | `clamp`, `extend` | no | clamp stops at the output range, extend continues the line beyond it |

### `reverb`

**Reverb**: A dense decaying wash of echoes

Uses 12 internal delay lines whose lengths follow the signal's sample rate. On video the tail is long in samples, so the node uses a lot of memory and asks the host to render up to 120 frames of warmup before a seek.

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `size` (Size) | 0.5 | 0 to 1 | yes | How long the tail rings: higher is longer |
| `damping` (Damping) | 0.5 | 0 to 1 | yes | How quickly the tail loses its fast detail: higher is duller |
| `predelay` (Pre-delay) | 0 `ms` | 0 to 100 (up to 0 to 10000) | yes | Gap before the reverb starts. Unit in `predelay.unit`. |
| `mix` (Mix) | 1 | 0 to 1 | yes | 0 is the dry input, 1 is only the processed signal; in between crossfades the two |

### `ring_mod`

**Ring Modulation**: Multiplies the carrier by the modulator, leaving sum and difference tones

Can process R, G and B separately.

**Inputs**

- `carrier` (main, required): The signal that gets multiplied
- `modulator` (required): The signal it is multiplied by

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `mix` (Mix) | 1 | 0 to 1 | yes | 0 is the dry input, 1 is only the processed signal; in between crossfades the two |

### `sample_hold`

**Sample & Hold**: Holds each sampled value for a while and optionally rounds it to a few levels

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `period` (Period) | 0.25 `row` | 0 to 4 (up to 0 to 1000000) | yes | How long each sampled value is held; 0 samples every sample (no hold). Unit in `period.unit`. |
| `levels` (Levels) | 0 | 0 to 32 (up to 0 to 65536) | yes | Rounds each value to this many evenly spaced levels between 0 and 1; 0 or 1 leaves values alone |

### `slew`

**Slew**: Limits how fast the signal itself can rise and fall, turning jumps into ramps (Envelope follows its strength instead)

Can process R, G and B separately.

**Inputs**

- `in` (main, required): The signal to process

**Outputs**

- `out`: The processed signal

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `rise` (Rise) | 0.25 `row` | 0 to 4 (up to 0 to 1000000) | yes | Time to climb a full 0 to 1 when the input jumps up; 0 is instant. Unit in `rise.unit`. |
| `fall` (Fall) | 0.25 `row` | 0 to 4 (up to 0 to 1000000) | yes | Time to fall a full 1 to 0 when the input jumps down; 0 is instant. Unit in `fall.unit`. |

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
| `low_hz` (Low / mid) | 250 `second` | 1 to 100000 (up to 0.001 to 1000000000) | yes | Crossover between the low and mid bands. Cycles per unit in `low_hz.unit`, or one cycle every so many units in `low_hz.every`. |
| `high_hz` (Mid / high) | 4000 `second` | 1 to 100000 (up to 0.001 to 1000000000) | yes | Crossover between the mid and high bands. Cycles per unit in `high_hz.unit`, or one cycle every so many units in `high_hz.every`. |
| `slope` (Slope (dB/oct)) | `12` | `12`, `24`, `48` | no | How sharply each crossover separates the bands. The mid band is what remains, so the three bands always add back up to the input at any slope |

## Output

| Node | What it does |
|---|---|
| [Audio Output](#audio_output) | The rendered sound of one output bus: replaces that bus's track mix in the preview and the export |
| [Named Output](#named_output) | An output other graphs read by its name, without adding to this graph's picture or sound |
| [Output](#output) | The rendered result: RGB, or mono shown as grayscale |

### `audio_output`

**Audio Output**: The rendered sound of one output bus: replaces that bus's track mix in the preview and the export

Every graph has one on the master bus, which can't be removed; add one for each other bus. With nothing connected, the bus plays its track mix: the audio tracks routed to it, at their volumes; a track wired straight in is also used as it is. A mono signal is written to every channel of the bus; any other signal is written as interleaved samples across the bus's channels (a stereo signal to a stereo bus as it is). Each frame's block is resampled from its own rate to the project's audio rate, and clipped to -1 to 1.

**Inputs**

- `in` (main, required): The sound to output, from -1 to 1: mono, stereo, or any signal read as samples

**Outputs**

- `out` (audio, -1 to 1): The sound as written

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `volume` (Volume) | 0 dB | -48 to 12 (up to -120 to 60) | yes | Louder or quieter, in decibels, before the sound is clipped to -1 to 1. 0 changes nothing; the meter shows the peak after the volume, so a peak above 0 dB will clip. |
| `bus` (Bus) | `Main` | text | no | The output bus this sound goes to, from the buses in Project settings. The project's first bus (Main unless renamed) is the one the preview plays and the export writes. An output naming a bus the project doesn't have is not rendered. |

### `named_output`

**Named Output**: An output other graphs read by its name, without adding to this graph's picture or sound

A Video Source or Audio Source in a graph on a higher graph track reads it by picking this graph's track and the name. Any signal goes in and comes out as it is. Names are unique within a graph.

**Inputs**

- `in` (main, required): The signal to offer to other graphs

**Outputs**

- `out`: The signal as offered

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `name` (Name) | `Out` | text | no | What other graphs pick it by |

### `output`

**Output**: The rendered result: RGB, or mono shown as grayscale

**Inputs**

- `in` (main, required): The picture to render: RGB, or mono for grayscale

**Outputs**

- `out` (video, 0 to 1): The rendered picture

**Parameters**

| Name | Default | Range | Modulation | What it does |
|---|---|---|---|---|
| `stretch` (Stretch to fit) | `on` | `on`, `off` | no | on stretches a signal of any size or kind to the project size, as if it were wired to the video input, so the output always shows something; off requires an RGB or mono signal of exactly the project size |

