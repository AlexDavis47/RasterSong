# Concepts

RasterSong is a unique video editing tool that allows video and audio to merge, creating interesting or glitchy
video effects that are directly tied to audio.

## Core Concept

Video files are typically made up of frames, which are made up of pixels.
Each pixel is typically three or four color channels, with 8 bits per channel allowing 256 values for each channel.

Similarly, audio files are made up of samples, values that represent the amplitude of the audio at a given time.
Typically these files are sampled at 44.1kHz (44,100 samples per second), and 16 bits per sample allowing 65,536 amplitude values.

RasterSong works by taking a video file and unraveling its frames pixel by pixel into sequences of values.
Each channel is just a sequence of values, and we interpret them as audio samples.

To keep things simple, we'll call the converted video signal the "carrier" and any additional audio signals the "modulators".
At this point, all of our data is being treated as audio.

We want to lay the carrier and the modulators on top of each other.
There are two approaches to handling color channels:

**Approach 1: Sequential Packing**
Concatenate all color channels into a single carrier stream (e.g., R, G, B, R, G, B, ...).
This results in one carrier signal with all channel data interleaved.

**Approach 2: Separate Carriers**
Keep each color channel as a separate carrier signal. This gives us three carriers and one modulator (the original audio):

- Red carrier (each pixel's red channel value)
- Green carrier (each pixel's green channel value)
- Blue carrier (each pixel's blue channel value)
- Modulator (the audio track)

Each approach produces vastly different visual effects. RasterSong is graph-based, so both are available:
the video input provides an RGB signal, and explicit **Split** / **Combine** / **Interleave** nodes let the user
choose how channels are separated, packed, and in which order. See [Node behavior](node-behavior.md#channels-and-interleaving).

### Synchronization Challenge

The modulator was recorded at 44.1kHz, meaning for every real time second there are **44,100** values.

But our video is made up of frames. Say 30fps, 240×180 pixels, 8-bit color depth, and no alpha channel:

**Approach 1: Sequential Packing**
For every real time second: 30 × (240 × 180) × 3 = **3,888,000** values.

- Ratio: 3,888,000 ÷ 44,100 ≈ **88×** more samples per second than the modulator

**Approach 2: Separate Carriers**
For every real time second, each carrier has: 30 × (240 × 180) = **1,296,000** values.

- Ratio: 1,296,000 ÷ 44,100 ≈ **29.4×** more samples per second per carrier

**Performance challenge:** a preview needs to process millions of values per second, and the count scales with
resolution (1080p30 RGB is ~187M values per second). RasterSong does not need to be strictly real time; it renders
ahead into a cache, and reduced-resolution previews cut the cost dramatically (see
[Preview resolution](engine.md#preview-resolution)). How the engine keeps signals of different lengths in step is in
[Signals & units](signals-and-units.md#units--timing) and [Rate matching](signals-and-units.md#rate-matching).

## How It Works

Imagine you're making a music video. You've got your video, and you've got your audio. Wouldn't it be cool if the song _itself_ could interact with your visuals?

### Basic Workflow

1. **Import your video file** - It becomes a video track, which reaches the graph through its Video In port as an RGB signal
2. **Import your music file** - It becomes an audio track, which reaches the graph through an Audio In port as your modulator
3. **Split the carrier** - A Split node turns the RGB signal into red, green, and blue signals
4. **Split the modulator** - A three-band splitter outputs:
   - Bass track
   - Mids track
   - Treble track
5. **Apply amplitude modulation** - Create an AM node and connect:
   - Red carrier → carrier input
   - Bass track → modulator input
   - Repeat for green (mids) and blue (treble)
6. **Combine** - A Combine node rebuilds the RGB signal and feeds the Output node

### What Happens

When amplitude modulation is applied, the amplitude of each carrier is modulated by its corresponding frequency band:

- **Positive waveform** → Carrier amplitude increases
- **Negative waveform** → Carrier amplitude decreases

The brightness of each color channel now follows its frequency band. When a kick drum hits, the red channel will begin waving around!

### Advanced Effects

Using utility and effect nodes, you can create a variety of effects:

- **Delay node** - Offsets each line of video slightly from the next, creating a wave effect that follows your bass notes. A rising bass note causes the waving to morph and change rates.
- **Bit crush node** - Reduces the bit depth of the carrier, creating a sort of posterization effect that can be tied to a modulator.
- **Filter node (low pass)** - Set to a low pass, it smooths the carrier signal, creating a sort of blur effect that can be tied to a modulator. A 6 dB/oct slope is the gentlest blur; steeper slopes cut harder.

### What Makes It Unique

This isn't the same as having a video processing effect automated by amplitude. The coolest part is that you're seeing the **literal audio interacting with every single pixel** in the video at the most base level.

While there are many programs that can make interesting visuals reacting to audio, RasterSong has a sort of sentimental value to it, and as far as I know, the visual effects produced are unique to itself.

## Design Principles

1. **Correctness is testable.** Every layer can be tested without the layers above it, and most of the system can be tested without FFmpeg at all.
2. **The graph never sees media formats, and the media layer never sees the graph.** Frames go in, frames come out.
3. **Simple over clever.** One signal type, one processing order (sequential), one cache rule.
4. **CPU only.** No GPU compute. Real time is a goal for preview, not a requirement.
5. **Nothing for the user to install.** All media dependencies ship with the app.
6. **One implementation per idea.** A behavior that more than one node, panel or tool needs (a unit list, a meter, a
   tooltip, a filter design) lives in one shared place. See [Shared building blocks](node-authoring.md#shared-building-blocks)
   and the [DRY workstream](roadmap.md#code-health-and-dry).
