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

> **Note on sequential packing:**  
> Changing how carriers are structured will impact how they synchronize with the modulator. Special care and handling will be required to maintain proper sync between all streams.

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

## System Architecture

### Synchronization

In order to synchronize the carrier, modulators, effects, IO, and UI, we need a carefully designed architecture that handles the massive sample rate disparity between video and audio.

The solution is a **hybrid frame-and-sample approach**: treat the timeline as **frame-based** (because the preview is frame-based and scrubbing must be deterministic), but inside the effect graph work with **sample-stream blocks** where "sample" = a single color-channel value (i.e. carrier sample-rate = `fps × pixels_per_frame`).

**Key architectural decisions:**

- **Inputs are frame packages** at the top level
- A frame-package can be decoded into **channel streams** (per-channel sequences of `pixels_per_frame` samples) using a `FrameDecoder` node.
- **Nodes operate on blocks of samples** (per-channel) and declare `lookback`/`lookahead` (in samples) and `latency` (in samples)
- The graph is **pull-based** from the Output node: when output needs frame N, it pulls the necessary sample range from its inputs, which recursively pulls from upstream with the requested padding (lookback/ahead). This is opposed to push-based, where input would be sent into the graph first,
  and output would be retrieved from the graph after the input has been processed.
- For **real-time preview**: use a dynamic buffered playback system where playback speed is determined by the distance between the current playhead and the end of buffered frames.
  The playback rate is calculated as: `playback_speed = min(buffer_distance_in_seconds, 1.0)`.
  This means:

  - 1.0 second or more buffered = full speed playback
  - 0.5 seconds buffered = half speed playback
  - 0.25 seconds buffered = quarter speed playback
  - No buffered frames (just after seeking) = speed of 0, rising as frames render

  This approach is self-regulating and elegantly handles:

  - Initial buffering: playback starts at speed 0 and accelerates as frames are rendered
  - Seeking/scrubbing: speed drops to 0 and rebuilds naturally
  - Heavy effects: playback automatically slows to a sustainable rate matching render speed
  - Light effects: playback runs at full speed with healthy buffer

  The system will naturally find an equilibrium speed based on effect complexity and hardware capability.

- For **export/offline**: run the exact same graph at the highest possible processing rate (export functions nearly identically to preview due to the volatility of effects)

### Key Concepts & Definitions

**Frame:** The video frame the UI shows at time `t`.

**Pixel sample:** One color-channel value (e.g. R at pixel index i).

**Carrier sample rate (per channel):** `carrier_sr = fps × pixels_per_frame`

- Example: 30 fps × 240×180 = 1,296,000 samples/s per channel
- This is the **internal timeline unit** for the entire system

**Modulator sample rate:** Original file sample rate (e.g. 44100 Hz)

**Block:** A contiguous segment of samples that a node will process in one call. Block size is typically a whole-frame worth of samples (makes mapping to frame easy).

- Pixels/frame = `240×180 = 43,200` samples per channel per frame
- Per frame total carrier samples (3 channels) = 129,600
- Per second at 30fps = 3,888,000 samples/sec total (≈ 3.9M)

**Latency / lookahead / lookback:** All expressed in samples of the carrier sample rate.

- **lookahead** = how many _future_ samples the node needs to compute current outputs
- **lookback** = how many _past_ samples the node needs to compute current outputs
- **latency** = the delay (in samples) introduced by the node

To the user, most effects will be expressed in terms of frames or milliseconds, but these units will be converted internally for the system to work with. We can use the carrier metadata to dynamically convert between them.

### Why Frame-Top, Sample-Inside (Hybrid)

The UI and scrubbing are inherently **frame-based** — you need whole frames to display.

Many effects are naturally **sample-based** (e.g., delays, convolutions, per-pixel waves). Processing sample-by-sample gives maximum flexibility.

**Hybrid = easier UX** (timeline remains frame) + full expressiveness inside the graph. Also simpler latency mapping: everything is measured in carrier samples.

Most importantly, this approach allows a clear separation of concerns between the timeline and processor. The processor has no knowledge of a "frame" or "pixel". It simply processes samples. Likewise, the timeline has no knowledge of a "sample". It simply sends and receives frames to display.

### Multi-Rate Handling (Audio ↔ Carriers)

**Dynamic resampling approach:**

Due to the massive amount of samples required for a carrier signal, it is highly impractical to resample the modulator signals to the carrier sample rate. Instead, the modulator signal will be sent into the processor as is. This means that on input, the modulator signals will have massively lower numbers of samples than the carrier signals. We solve this in one of three ways:

- Since the number of samples in the modulator and carrier signals are both part of the chunk metadata, any processing node can simply do an interpolation (nearest neighbor, linear, etc) to the carrier sample rate internally.
- Alternatively, we add a resampling or interpolation node to the graph that will resample or interpolate signal A to signal B's sample rate.
- **!Currently Recommended Approach!**: Last, we just have a property on the modulator clips that says what kind of interpolation or resampling to use when sending chunks to the graph.
  Then the input processor of the graph would use the property to determine how to resample the modulator signal to the carrier.
  This is likely the most "intuitive" and sturdy approach, but would take a marginal amount of control away from the user to choose how each effect handles interpolation.

### Block Data Types & Formats

Nodes in the effect graph need to communicate sample data in various formats depending on their purpose. The system supports multiple block data types:

**Block format types:**

1. **Frame** (`Frame`)

   - Represents one frame of video
   - Highest level form of input and output. This is the form that the input and output nodes will receive and send.
   - Contains all three color channels, each as a `Vec<f32>` of the raw pixel data.
   - When sent into an effect, this should be interpreted as an object that needs all three color channels to be processed together.
   - For example, an AM node that receives a Frame would average the modulator signal across the entire frame, and then modulate all color channels by that average value. This results in the exact same effect all over the frame.

2. **Channel** (`Channel`)

   - Represents one color channel of a frame.
   - Single 'Vec<f32>' of `samples'
   - Best for: per-channel effects, channel-specific processing.
   - Simplest possible format for a node to receive and process.
   - Sending a Channel to an AM node would result in only that channel being modulated by the modulator signal.
   - Note this is also the format used for the modulator signals.

3. **Interleaved** (`Interleaved`)

   - Single `Vec<f32>` with R, G, B values interleaved: `[R0, G0, B0, R1, G1, B1, ...]`
   - Total length: `samples_per_channel × 3`
   - Arbitrary ordering possible (e.g., RGB, BGR, GBR)
   - Best for creating rainbow effects, or just sending one channel into an effect to process all three color channels at once.

4. **Sequential Packed** (`SequentialPacked`)

   - Similar to Interleaved, but color channels are simply packed next to each other in the vector.
   - Example: [R0, ...Rn, G0, ...Gn, B0, ...Bn] Where n is the number of samples in the color channel.

More may be added in the future, but these are the most fundamental ones.

**Format conversion**

A converter node will be included in the graph that will be able to convert between any two of the above formats.
To use the node, the user will configure the node with the input and output formats they want to convert between.
The node will setup it's input and outputs to match the configured format, and automatically convert input to the output format.

**Synchronization example (AM node with 3-channel carrier):**

When an AM node receives an `Interleaved` carrier (3× samples) and a `Channel` modulator (1× samples):

- Carrier: `[R0, G0, B0, R1, G1, B1, R2, G2, B2, ...]` (129,600 values for a 240×180 frame, 43,200 samples per channel x3)
- Modulator: `[M0, M1, M2, ...]` (43,200 values, 44.1kHz original interpolated at graph input processor to carrier channel sample rate)
- Application: `M0` modulates `R0, G0, B0`; `M1` modulates `R1, G1, B1`; etc.
- Each modulator sample affects one pixel across all channels, maintaining sync

**Block wrapper type:**

All block formats are wrapped in a unified `Block` enum that nodes receive:

```rust
enum Block {
    Frame { r: Vec<f32>, g: Vec<f32>, b: Vec<f32> },
    Channel(Vec<f32>),
    Interleaved(Vec<f32>),
    SequentialPacked { r: Vec<f32>, g: Vec<f32>, b: Vec<f32> },
}
```

This allows nodes to accept multiple formats and implement format-specific processing logic. The dispatch overhead is negligible since chunks are processed once per frame (not per sample).

**Node input/output type declarations:**

Each node declares which input formats it accepts and what format it outputs:

```rust
trait EffectGraphNode {
    // Returns the list of effect_processor formats this node can accept as input
    // Used by the graph engine to validate connections at connection time
    fn accepted_input_formats(&self) -> Vec<BlockFormat>;

    // Returns the format this node outputs
    fn output_format(&self) -> BlockFormat;
    // ... other methods
}
```

The graph engine validates connections by checking if the upstream node's output format is in the downstream node's `accepted_input_formats()`. If not, it either:

- Automatically inserts a conversion node when conversion is possible
- Errors if conversion would lose semantic meaning or is not supported

**Format-specific processing:**

Nodes that accept multiple formats implement different processing logic for each format. For example, an AM node might:

- **Frame**: Average the modulator signal across the entire frame, then modulate all color channels uniformly by that average value
- **Channel**: Apply per-sample modulation to the single channel independently
- **Interleaved**: Apply per-pixel modulation where each modulator sample affects the corresponding RGB triplet

### Node Contract (API)

Each node implements (at the minimum):

```rust
trait EffectGraphNode {
    // Declare which formats this node accepts (for connection validation)
    fn accepted_input_formats(&self) -> Vec<BlockFormat>;
    fn output_format(&self) -> BlockFormat;

    // Called to fill 'out' for sample indices [start, start+len)
    // sampleIndex is in carrier-sample units relative to timeline
    // Takes a Block enum and handles format dispatch internally
    fn process(&mut self, start: usize, len: usize, input: Block, out: &mut Block);

    // If node needs extra context, it returns:
    fn get_lookback(&self) -> usize;   // samples required before start
    fn get_lookahead(&self) -> usize;  // samples required after start+len-1
    fn get_latency(&self) -> usize;    // internal processing latency (samples)
    fn is_stateful(&self) -> bool;     // maintains internal state across calls (e.g. delays, reverb, feedback, etc.)
}
```

- `process` receives a `Block` enum and implements format-specific handling via internal dispatch (match/switch)
- `process` must be deterministic and pure given the same input ranges (except for nodes explicitly stateful like random/noise)
- The engine uses these declarations to calculate total graph latency and request appropriate sample ranges from upstream nodes
- Format dispatch overhead is negligible since it occurs once per frame (chunk boundary), not per sample

### Pull-Based Evaluation with Padding

When Output wants to render frame `F`:

1. Convert frame `F` → sample range `[S, S + samples_per_frame)` where `S = F × samples_per_frame`
2. Ask Output node to `process(S, samples_per_frame)`
3. Output node asks its input nodes for ranges expanded by their `lookback/lookahead`. Each upstream node returns data (recursing)
4. The engine **buffers** these intermediary ranges so if multiple nodes request overlapping ranges they share work
5. Each node internally offsets output by its `latency`. The engine sums latencies as part of mapping sample indices between nodes

**Important:** Any node that needs lookahead/lookback **inherently crosses frame boundaries**. If a node requires lookahead of 12 samples, it will always need the next 12 samples from the next frame.

This allows nodes that need future or past samples to request them. The engine fetches/decodes the required frames as needed.

Example inside a node:

```rust
fn process(&mut self, start: usize, len: usize, out: &mut SampleBuffer) {
    let lookback = self.get_lookback();
    let lookahead = self.get_lookahead();
    let upstream_start = start - lookback;
    let upstream_len = len + lookback + lookahead;
    let upstream_buf = self.upstream_node.get_samples(upstream_start, upstream_len);

    // compute using upstream_buf, fill out[0..len)
    // respect node latency: if node has latency L, write out[i] = computed[i - L]
}
```

### Dynamic Playback & Buffer Management

**Dynamic playback rate system:**

The preview system uses a dynamic playback rate that automatically adjusts based on available buffered frames:

```rust
playback_speed = min(buffer_distance_in_seconds, 1.0)
```

Where `buffer_distance_in_seconds` is the time between the current playhead position and the end of the pre-rendered frame cache.

**Behavior at different buffer levels:**

- **1.0+ seconds buffered**: Full speed playback (1.0x)
- **0.5 seconds buffered**: Half speed playback (0.5x)
- **0.25 seconds buffered**: Quarter speed playback (0.25x)
- **0 seconds buffered**: Paused (0.0x), waiting for frames to render

**Self-regulating advantages:**

- **No manual buffering needed**: When seeking, speed is 0 and naturally rises as frames are pre-rendered
- **Automatic performance adaptation**: Heavy effects naturally slow playback to a sustainable rate; light effects run at full speed
- **Smooth degradation**: Rather than stuttering or dropping frames, playback smoothly slows to match rendering capability
- **Natural equilibrium**: The system finds a stable playback speed based on effect complexity and hardware performance

**UI feedback:**

- Show current playback speed as a percentage (e.g., "Playing at 73%")
- Display buffer depth in seconds (e.g., "0.7s buffered")
- Visual indicator (e.g., progress bar) showing buffer health

**Node latency tracking:**

- Each node still reports `latency` (samples) for internal processing delay
- The engine computes `total_latency = sum(latency)` along the longest path for metadata/debugging purposes
- However, this latency is absorbed by the dynamic buffer system rather than requiring fixed compensation

### Stateful Effects & Real-Time State Management

**The core challenge:**

Stateful effects (feedback delays, reverbs, IIR filters, etc.) maintain internal buffers that depend on previously processed samples. In a random-access timeline where users can jump anywhere, we must carefully manage when state is valid and when it needs rebuilding.

**Example:** A feedback delay with 30 frames of tail time. To render frame 1000 accurately, we need the delay buffer to contain the results of processing frames 970-999. But if the user just jumped from frame 500, that state doesn't exist.

---

#### State Warmup on Seek

When the user seeks to an uncached position, stateful effects have no valid internal state. The solution:

**Warmup strategy:**

The processing graph does not handle state warmup. Instead, the timeline manages warmup by coordinating with the graph:

1. The graph reports its warmup requirement: `get_warmup_frames()` returns K, the maximum lookback required by any stateful node (in frames)
2. User seeks to frame N (no cache exists)
3. Timeline moves back K frames to frame N-K to account for warmup requirement
4. Timeline requests frames starting from N-K, simulating them from scratch
5. The graph processes frames N-K through N-1, updating internal state, but the timeline discards the output from these warmup frames
6. Frame N is then processed with properly warmed state and displayed
7. This handles any non-linearity in the timeline (seeking to N actually processes from N-K) transparently

**Important:**

- The warmup frames are processed but their output is discarded — the user never sees them
- Frame N will be accurate (as if processed from the start) because state was properly warmed
- This approach keeps the processing graph stateless regarding warmup — it just processes what it's asked for
- Export should process from the beginning or a valid state checkpoint for full accuracy

---

#### Proper Lookahead/Lookback Processing

Stateful effects must process ALL samples in the requested range, including lookahead/lookback padding, not just the output range:

**Incorrect approach:**

```rust
// BAD: Only process the requested output range
fn process(&mut self, start: usize, len: usize, out: &mut SampleBuffer) {
    let lookback = self.get_lookback();
    let upstream_data = self.upstream.get_samples(start - lookback, len + lookback);
    // Only processes samples [start, start+len), ignoring the lookback data
    self.apply_effect(&upstream_data[lookback..lookback+len], out);
}
```

**Correct approach:**

```rust
// GOOD: Process the entire range including padding
fn process(&mut self, start: usize, len: usize, out: &mut SampleBuffer) {
    let lookback = self.get_lookback();
    let lookahead = self.get_lookahead();
    let upstream_start = start - lookback;
    let upstream_len = len + lookback + lookahead;
    let upstream_data = self.upstream.get_samples(upstream_start, upstream_len);

    // Process ALL samples to update internal state correctly
    let mut full_output = vec![0.0; upstream_len];
    self.apply_effect(&upstream_data, &mut full_output);

    // Extract only the requested range for output
    out.copy_from_slice(&full_output[lookback..lookback+len]);
}
```

**Why this matters:** If a feedback node only processes the current frame and ignores the lookback data, its internal buffer won't be properly updated. The next frame request will then receive incorrect feedback.

---

#### Cache Validity

With the timeline handling state warmup, cache validity is greatly simplified:

**Cache validity rule:**

A cached frame is valid as long as the effect graph configuration hasn't changed. Since the timeline handles warmup by requesting frames from N-K and discarding their output, seeking does not invalidate the cache.

**Why seeking doesn't invalidate cache:**

- Cached frames store the deterministic output of processing with a specific graph configuration
- State is transient and rebuilt on-demand when frames are requested
- When seeking to frame N, the timeline requests frames N-K through N to warm up state, but this doesn't affect cached frames
- Cached frames remain valid regardless of seek history because they represent correct output for their graph configuration

**Invalid cache scenarios:**

1. **Effect graph changes**:
   - User tweaks a node's parameters
   - User adds/removes nodes
   - User changes node connections
   - All cached frames become invalid and must be recomputed

**Cache invalidation rules:**

```rust
struct CachedFrame {
    frame_index: usize,
    data: FrameData,
    effect_graph_hash: u64,  // Hash of the effect graph configuration
}

// Invalidate when:
// 1. Effect graph changes (effect_graph_hash changes)
// That's it! Seeking doesn't affect cache validity.
```

**Implementation strategy:**

- Each cached frame stores the effect graph hash it was computed with
- When the effect graph changes, invalidate all cached frames
- When seeking, check cache first — if frame exists and graph hash matches, use cached frame
- If cache miss, timeline handles warmup by requesting frames from N-K, then caches the result

#### State Snapshot System (Optional Optimization)

For better seeking performance with stateful effects:

**Snapshot strategy:**

```rust
struct StateSnapshot {
    frame_index: usize,
    effect_graph_hash: u64,
    node_states: HashMap<NodeId, Vec<u8>>,  // Serialized state per node
}

// Create snapshots:
// - Every N frames (e.g., every 150 frames = 5 seconds at 30fps)
// - Before major seeks (save current state before jumping)

// On seek to frame F:
// 1. Find nearest snapshot before F
// 2. Load snapshot state into all nodes
// 3. Process from snapshot frame to F
// 4. Much faster than processing from start or doing full warmup
```

**Trade-off:** Memory usage vs seek performance. For long videos with heavy stateful effects, snapshots dramatically improve UX.

---

#### Interactive Editing Workflow

**Critical UX consideration:** Users will often pause on a single frame and tweak effect parameters repeatedly. The system must handle this:

1. User pauses on frame 500
2. User adjusts feedback delay time
3. Effect graph changes → frame 500 cache invalidated
4. Timeline automatically:

   - Gets warmup requirement K from graph
   - Moves back to frame 500-K
   - Requests frames 500-K through 500, discarding output from 500-K through 499
   - Displays frame 500 with properly warmed state
   - Continues buffering forward

5. User tweaks again → repeat invalidation and warmup

**Note on frequent editing:**

- When user is rapidly tweaking parameters, frames may be invalidated faster than they're consumed during playback
- The dynamic playback system naturally handles this: playback slows as buffer shrinks
- The timeline handles the warmup non-linearity transparently — user always sees the frame they seeked to
- No special logic needed in the processing graph — it just processes what it's asked for

### Continuous Processing & Caching

**Always-running processing engine:**

- The processing engine is **always running** in the background at full speed, continuously pre-rendering frames ahead of the playhead
- Takes advantage of any available CPU/GPU time to build up the buffer, maximizing playback speed
- Works in tandem with the dynamic playback system:
  - When paused: continues rendering forward frames to build buffer
  - When playing: maintains balance between playback consumption and frame production
  - After seeking: timeline handles warmup by requesting frames starting from N-K, then the engine processes normally from the seek position
- **Cache invalidation:** whenever the node chain is modified, invalidate all cached frames (see Cache Validity section)
- **No warmup logic:** The processing engine does not handle state warmup — it simply processes frames as requested by the timeline

**Caching strategy with state awareness:**

- **Rendered frame cache**: Store processed frames with effect graph hash
  - Each frame tagged with: `frame_index`, `effect_graph_hash`
  - Cache remains valid across seeks — only graph changes invalidate cache
  - Target size: 1+ second of frames ahead of playhead
- **Source frame cache**: Keep decoded video frames in an LRU cache for quick access to raw carriers
  - Independent of effect graph state
  - Persists across effect graph changes
- **State snapshot cache** (optional): Periodic snapshots of all node states
  - Enables fast seeking without full recomputation
  - Example: snapshot every 5 seconds of video
- **Sample buffer pool**: Maintain reusable buffers to reduce allocations during processing
- **Hot cache strategy**:
  - Prioritize frames around playhead position
  - Satisfy node lookahead requirements
  - Note: Warmup frames are handled by the timeline, not cached (they're discarded after processing)

**Buffer management for dynamic playback:**

- Continuously monitor buffer depth (distance to furthest pre-rendered frame)
- Processing engine **always runs at full speed**, continuously building cache forward
- No priority adjustments or throttling — simplicity over micro-optimizations
- Buffer depth directly drives playback speed via the formula `min(buffer_distance_in_seconds, 1.0)`
- The "adaptive" nature is in the playback speed adjustment, not the rendering speed

**Cache coherency:**

Cache validity is simple — only check if the effect graph hash matches:

```rust
struct ProcessingEngine {
    frame_cache: HashMap<usize, CachedFrame>,
    current_effect_graph_hash: u64,
}

// On frame request:
// 1. Check if frame is cached AND effect_graph_hash matches current graph
// 2. If valid: return cached frame
// 3. If cache miss: timeline handles warmup by requesting frames starting from N-K
//    Processing engine just processes what it's asked for — no warmup logic here
// 4. Cache the result with current effect_graph_hash
```

### Export / Offline Rendering

Export uses the exact same graph and processing pipeline as preview, ensuring what you see is what you get:

- **Identical processing**: Same nodes, same algorithms, same sample-accurate results
- **No playback constraints**: Rendering runs at maximum speed without worrying about real-time playback
- **No dynamic speed adjustments**: Processes every frame sequentially at full quality
- **Buffer management**: Uses larger buffers optimized for throughput rather than latency
- **Progress indication**: Shows frame count and estimated time remaining instead of playback speed
- **Deterministic output**: Given the same input and effect graph, export produces bit-identical results every time

The key difference from preview is that export doesn't need to maintain the real-time buffer/playback balance—it simply processes frames as fast as possible and writes them to disk.

### Performance Optimization

Optimization strategies:

- Use **SIMD** on CPU for node operations across samples
- Offload heavy linear operations (convolutions, AM, bitcrush) to **GPU compute shaders** — carriers map naturally to large linear arrays that GPUs handle well
- Break up blocks into smaller chunks to allow higher concurrent node operations to maximize CPU parallelism
- Provide lower-resolution proxy pipeline: process at reduced resolution for live preview when needed

### Example Node Behaviors

**Delay node:**

- `lookback = N` samples, `latency = N` samples
- When asked to `process(S, L)`, fetches `[S-N, S+L)` from input
- Outputs the `[S, S+L)` portion, maintaining internal buffer for delay feedback

**Convolution (blur):**

- `lookback = kernel_radius`, `lookahead = kernel_radius`, `latency = kernel_radius`

**AM node (amplitude modulation):**

- If purely samplewise: `lookback = 0`, `lookahead = 0`, `latency = 0`
- If using smoothed envelopes: may require small lookback

### Spatial vs Temporal Dependencies

**Spatial effects (within-frame):**

- Effects that re-sample nearby pixels spatially (e.g., shifting pixels horizontally) have dependencies within the frame
- Can be handled purely in-frame since all pixels of the frame are available after decoding
- Only need multi-frame access when spatial shift reaches into next/previous frames (temporal shift)

### Practical Implementation Roadmap

1. **Basic frame pipeline**: Decode & display video frames with no graph effects

   - Video decoder integration
   - Frame display in UI

2. **Carrier extraction**: Implement node that converts decoded frame → channel sample buffers

   - Support multiple block formats (SeparateChannels, InterleavedRGB, etc.)
   - Frame to sample buffer conversion

3. **Up-front resampling**: Resample modulator audio to carrier rate at load time

   - High-quality resampling (polyphase/sinc)
   - Single-pass on load, cache result

4. **Block format system**:

   - Define BlockFormat enum (SeparateChannels, InterleavedRGB, SequentialPacked, SingleChannel)
   - Implement format conversion nodes
   - Add format validation to graph connections

5. **Simple pull-based engine**: Build basic graph evaluation with one stateless effect (AM)

   - Implement EffectGraphNode trait
   - Basic pull-based evaluation from output
   - Test with simple AM node (no state, no lookback)

6. **Lookback/lookahead support**:

   - Extend node API with get_lookback(), get_lookahead()
   - Implement sample range expansion in pull evaluation
   - Validate with a simple Delay node

7. **Node latency tracking**:

   - Add get_latency() to node API
   - Calculate total graph latency
   - Display in UI for debugging

8. **Basic frame caching**:

   - Implement simple frame cache (HashMap)
   - No state awareness yet—just LRU eviction

9. **Always-running processing engine**:

   - Background thread that continuously renders ahead of playhead
   - Thread-safe communication with main thread
   - Frame ready notifications

10. **Dynamic playback system**:

    - Implement buffer depth monitoring (time between playhead and furthest cached frame)
    - Add playback speed calculation: `min(buffer_distance_in_seconds, 1.0)`
    - Connect playback rate to buffer depth with smooth transitions
    - Add UI indicators for playback speed and buffer health

11. **Stateful effect support**:

    - Add is_stateful() to node API
    - Add get_warmup_frames() to graph API (returns max lookback in frames)
    - Timeline handles warmup: requests frames from N-K, discards warmup output
    - Build and test with feedback delay node

12. **Cache validity tracking**:

    - Add effect_graph_hash to CachedFrame
    - Implement cache invalidation on graph changes only
    - Cache remains valid across seeks (timeline handles warmup)

13. **State snapshot system** (optional optimization):

    - Implement node state serialization
    - Periodic snapshot creation (every N frames)
    - Snapshot-based seeking

14. **Performance optimization**:

    - SIMD for CPU-based effects
    - GPU compute shaders for heavy operations
    - Lazy float conversion for carriers
    - Memory pooling for sample buffers
