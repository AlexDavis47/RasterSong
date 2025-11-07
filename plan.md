# RasterSong Plan

RasterSong is a unique video editing tool that allows video and audio to merge,
creating interesting or glitchy video effects that are directly tied to audio.

## Core Concept

Video files are typically made up of frames, which are made up of pixels.
Each pixel is typically three or four color channels, with 8 bits per channel allowing 256 values for each channel.

Similarly, audio files are made up of samples, values that represent the amplitude of the audio at a given time.
Typically these files are sampled at 44.1kHz (44100 samples per second). And 16 bits per sample allowing 65535 amplitude values.

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
Keep each color channel as a separate carrier signal. This gives us three carriers and one modulator (the original audio).
Each approach produces vastly different visual effects. Since RasterSong is graph-based, we'll use this approach with separate carriers, and include a graph node that can convert them to sequential format if desired, even allowing the user to choose what ordering they want.

- Red carrier (Each pixel's red channel value)
- Blue carrier (Each pixel's blue channel value)
- Green carrier (Each pixel's green channel value)
- Modulator (The original audio from the video file)

We can include a graph node that converts multiple carriers to sequential format if desired.

### Synchronization Challenge

The modulator was recorded at 44.1kHz, meaning for every real time second there are **44,100** values.

But our video was made up of frames, let's say 30fps, 240×180 pixels, 8-bit color depth, and no alpha channel:

- 30 Frames per second
- 240 pixels wide
- 180 pixels high
- 3 colors per pixel
- 8 bits per color

**Approach 1: Sequential Packing**
For every real time second: 30 × (240 × 180) × 3 = **3,888,000** 8-bit values.

- Ratio: 3,888,000 ÷ 44,100 ≈ **88x** more samples per second
- Carrier needs to be **sped up by 88x**

**Approach 2: Separate Carriers**
For every real time second, each carrier has: 30 × (240 × 180) = **1,296,000** 8-bit values.

- Ratio: 1,296,000 ÷ 44,100 ≈ **29.4x** more samples per second
- Each carrier needs to be **sped up by ~29x**

**Performance Challenge:** With three separate carriers, we need to process approximately 1.3 million values per second per carrier (about 3.9 million total across all carriers) to preview the video in real time. This requires significant optimizations.

## How It Works

Now that we've established the core concept, let's get into what RasterSong actually does.

Imagine you're making a music video. You've got your video, and you've got your audio. Wouldn't it be cool if the song _itself_ could interact with your visuals?

### Basic Workflow

1. **Import your video file** - RasterSong converts it into carrier signals (red, green, blue)
2. **Import your music file** - This becomes your modulator signal
3. **Open the effects graph** - You'll see four input nodes:
   - Three carrier inputs (red, green, blue)
   - One modulator input
4. **Split the modulator** - Drag it into a three-band splitter, which outputs:
   - Bass track
   - Mids track
   - Treble track
5. **Apply amplitude modulation** - Create an AM node and connect:
   - Red carrier → carrier input
   - Bass track → modulator input
   - Repeat for blue (mids) and green (treble)

### What Happens

When amplitude modulation is applied, the amplitude of each carrier is modulated by its corresponding frequency band:

- **Positive waveform** → Carrier amplitude increases
- **Negative waveform** → Carrier amplitude decreases

The brightness of each color channel now follows its frequency band. When a kick drum hits, the red channel will begin waving around!

### Advanced Effects

Using utility and effect nodes, you can create a variety of effects:

- **Sample delay node** - Offsets each line of video slightly from the next, creating a wave effect that follows your bass notes. A rising bass note causes the waving to morph and change rates.
- **Bit crush node** - Reduces the bit depth of the carrier, creating a sort of posterization effect that can be tied to a modulator.
- **Low pass filter node** - Smooths the carrier signal, creating a sort of blur effect that can be tied to a modulator.

### What Makes It Unique

This isn't the same as having a video processing effect automated by amplitude. The coolest part is that you're seeing the **literal audio interacting with every single pixel** in the video at the most base level.

While there are many programs that can make interesting visuals reacting to audio, RasterSong has a sort of sentimental value to it, and as far as I know, the visual effects produced are unique to itself.
