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
- The graph is **pull-based** from the Output node: when output needs frame N, it pulls the necessary sample range from its inputs, which recursively pulls from upstream with the requested padding (lookback/ahead)
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

### Why Frame-Top, Sample-Inside (Hybrid)

The UI and scrubbing are inherently **frame-based** — you need whole frames to display.

Many effects are naturally **sample-based** (e.g., delays, convolutions, per-pixel waves). Processing sample-by-sample gives maximum flexibility.

**Hybrid = easier UX** (timeline remains frame) + full expressiveness inside the graph. Also simpler latency mapping: everything is measured in carrier samples.

### Multi-Rate Handling (Audio ↔ Carriers)

**Up-front resampling approach:**

- Modulator signals are **fully resampled up-front** to carrier sample rate using highest-quality resampling
- This completely offloads real-time processing requirements
- A full resample is quick and only happens once when the audio is loaded
- After resampling, there is **no multi-rate issue** — carrier rate is the sole internal timeline unit
- All subsequent processing operates at carrier sample rate

This approach simplifies the architecture significantly and eliminates the need for real-time resampling nodes.

### Block Data Types & Formats

Nodes in the effect graph need to communicate sample data in various formats depending on their purpose. The system supports multiple block data types:

**Block format types:**

1. **Separate Channels** (`SeparateChannels`)

   - Three separate buffers: `Vec<f32>` for R, G, B
   - Each buffer contains `samples_per_frame` values
   - Best for: per-channel effects, channel-specific processing
   - Example: separate AM modulation on each color channel

2. **Interleaved RGB** (`InterleavedRGB`)

   - Single buffer with R, G, B values interleaved: `[R0, G0, B0, R1, G1, B1, ...]`
   - Total length: `samples_per_frame × 3`
   - Best for: effects that treat RGB as a unit, spatial effects
   - Example: pixel shifting, rotation

3. **Sequential Packed** (`SequentialPacked`)

   - All R values, then all G values, then all B values: `[R0...Rn, G0...Gn, B0...Bn]`
   - Total length: `samples_per_frame × 3`
   - Custom ordering possible (e.g., BGR, GBR)
   - Best for: treating video as continuous audio stream
   - Example: user wants to hear/modulate the video as pure audio

4. **Single Channel** (`SingleChannel`)
   - One buffer: `Vec<f32>`
   - Used for modulators, mono audio, or single-channel extractions
   - Length: `samples_per_frame` (at carrier rate after resampling)

**Format conversion nodes:**

The graph includes utility nodes for format conversion:

- `ChannelSplitter`: SeparateChannels → 3× SingleChannel
- `ChannelMerger`: 3× SingleChannel → SeparateChannels
- `ToInterleaved`: SeparateChannels → InterleavedRGB
- `ToSeparate`: InterleavedRGB → SeparateChannels
- `ToSequential`: SeparateChannels → SequentialPacked (with ordering options)
- `FromSequential`: SequentialPacked → SeparateChannels

**Synchronization example (AM node with 3-channel carrier):**

When an AM node receives an `InterleavedRGB` carrier (3× samples) and a `SingleChannel` modulator (1× samples):

- Carrier: `[R0, G0, B0, R1, G1, B1, R2, G2, B2, ...]` (129,600 values for 240×180)
- Modulator: `[M0, M1, M2, ...]` (43,200 values, resampled to carrier rate)
- Application: `M0` modulates `R0, G0, B0`; `M1` modulates `R1, G1, B1`; etc.
- Each modulator sample affects one pixel across all channels, maintaining sync

**Node input/output type declarations:**

```rust
trait EffectGraphNode {
    fn input_format(&self) -> BlockFormat;
    fn output_format(&self) -> BlockFormat;
    // ... other methods
}
```

The graph engine automatically inserts conversion nodes when connecting incompatible formats, or errors if conversion would lose semantic meaning.

### Node Contract (API)

Each node implements (at the minimum):

```rust
trait EffectGraphNode {
    // Called to fill 'out' for sample indices [start, start+len)
    // sampleIndex is in carrier-sample units relative to timeline
    fn process(&mut self, start: usize, len: usize, out: &mut SampleBuffer);

    // If node needs extra context, it returns:
    fn get_lookback(&self) -> usize;   // samples required before start
    fn get_lookahead(&self) -> usize;  // samples required after start+len-1
    fn get_latency(&self) -> usize;    // internal processing latency (samples)
    fn is_stateful(&self) -> bool;     // maintains internal state across calls (e.g. delays, reverb, feedback, etc.)
}
```

- `process` must be deterministic and pure given the same input ranges (except for nodes explicitly stateful like random/noise)
- The engine uses these declarations to calculate total graph latency and request appropriate sample ranges from upstream nodes

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

1. User seeks to frame N (no cache exists)
2. Playback speed drops to 0 (per dynamic buffer system)
3. Processing engine begins rendering from frame N-K, where K = maximum lookback required by any stateful node
4. First K frames are marked as "warming up" — state is being built but is not historically accurate
5. As frames render, buffer fills, and playback speed rises naturally
6. After K frames, state is "warm" and rendering continues normally

**Important:** The warmup frames are what the user will see during this period. They won't be perfectly accurate (as if we processed from the start of the video), but this is acceptable for interactive editing. Export should process from the beginning or a valid state checkpoint.

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

#### Cache Validity & State Continuity

A cached frame is only valid if it was computed with the correct preceding state. This creates complex invalidation rules:

**Valid cache scenarios:**

1. **Continuous forward processing**: Frames rendered sequentially from N to N+100 are all valid as long as the effect graph hasn't changed
2. **State checkpoint exists**: If we saved a state snapshot at frame N, frames N+1 onwards computed from that state are valid

**Invalid cache scenarios:**

1. **Seek breaks continuity**:

   - Cache contains frames 100-200 (rendered with continuous state)
   - User seeks to frame 500 (state resets)
   - User seeks back to frame 200
   - Cached frame 200 is NO LONGER VALID — it was computed with state from frames 100-199, but current state is empty

2. **Effect graph changes**:
   - User tweaks a feedback node's parameters at frame 150
   - All cached frames 150+ are now invalid and must be recomputed

**Cache invalidation rules:**

```rust
struct CachedFrame {
    frame_index: usize,
    data: FrameData,
    state_hash: u64,  // Hash of the effect graph + state continuity
    computed_with_state_from: Option<usize>,  // Previous frame that provided state
}

// Invalidate when:
// 1. Effect graph changes (state_hash changes)
// 2. State continuity breaks (computed_with_state_from doesn't match)
```

**Implementation strategy:**

- Each cached frame stores metadata about what state it was computed with
- When seeking back to a cached region after state reset, one of two approaches:

  **Option A: Recompute from warmup point**

  - Discard invalid cache entries
  - Start warmup from frame N-K
  - Recompute forward, overwriting old cache

  **Option B: State snapshots**

  - Store full node state snapshots at regular intervals (e.g., every 5 seconds)
  - When seeking back, load nearest prior snapshot
  - Recompute from snapshot to target frame
  - More memory, but faster seeks

---

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
4. System automatically:

   - Starts warmup from frame 500-K
   - Processes to frame 500
   - Shows result (playback speed still 0, since we're paused)
   - Continues buffering forward

5. User tweaks again → repeat invalidation and warmup

**Optimization for paused editing:**

- When paused, prioritize rendering the current frame + small buffer ahead
- Don't waste resources rendering hundreds of frames forward if user is tweaking
- Once tweaking stops (e.g., 2 seconds of no changes), begin aggressive forward caching

### Continuous Processing & Caching

**Always-running processing engine:**

- The processing engine is **always running** in the background, continuously pre-rendering frames ahead of the playhead
- Takes advantage of any available CPU/GPU time to build up the buffer, maximizing playback speed
- Works in tandem with the dynamic playback system:
  - When paused: aggressively renders forward frames to build buffer (with awareness of editing mode—see state management)
  - When playing: maintains balance between playback consumption and frame production
  - After seeking: immediately starts warmup + buffering from the new position
- **Cache invalidation:** whenever the node chain is modified, invalidate affected regions based on state continuity rules (see Cache Validity section)

**Caching strategy with state awareness:**

- **Rendered frame cache**: Store processed frames with metadata about state continuity
  - Each frame tagged with: `frame_index`, `effect_graph_hash`, `computed_with_state_from`
  - Allows intelligent invalidation when seeks break state continuity
  - Target size: 1+ second of frames ahead of playhead, plus warmup frames behind
- **Source frame cache**: Keep decoded video frames in an LRU cache for quick access to raw carriers
  - Independent of effect graph state
  - Persists across effect graph changes
- **State snapshot cache** (optional): Periodic snapshots of all node states
  - Enables fast seeking without full recomputation
  - Example: snapshot every 5 seconds of video
- **Sample buffer pool**: Maintain reusable buffers to reduce allocations during processing
- **Hot cache strategy**:
  - Prioritize frames around playhead position
  - Maintain warmup frames (K frames behind) for stateful effects
  - Satisfy node lookahead requirements

**Buffer management for dynamic playback:**

- Continuously monitor buffer depth (distance to furthest pre-rendered frame)
- Processing thread priority adjusts based on buffer health AND editing mode:
  - **Active editing** (frequent parameter changes):
    - Focus on current frame + small lookahead (2-5 frames)
    - Avoid wasting work on frames that will be invalidated
  - **Playback mode** (no recent changes):
    - Low buffer (< 0.5s): high priority rendering
    - Healthy buffer (> 1.0s): lower priority, allow CPU for other tasks
  - **Post-seek**:
    - High priority for warmup frames (K frames back)
    - Then forward buffering
- Buffer depth directly drives playback speed via the formula `min(buffer_distance_in_seconds, 1.0)`

**Cache coherency with stateful effects:**

The processing engine must track state continuity to determine cache validity:

```rust
struct ProcessingEngine {
    frame_cache: HashMap<usize, CachedFrame>,
    current_state_lineage: StateLineage,  // Tracks continuous processing ranges
    last_processed_frame: Option<usize>,
}

// On frame request:
// 1. Check if frame is cached AND state lineage matches
// 2. If valid: return cached frame
// 3. If invalid: determine warmup point, reprocess from there
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
- Offload heavy operations (convolutions, AM, bitcrush) to **GPU compute shaders** — carriers map naturally to large linear arrays that GPUs handle well
- Use **uint8** or packed bytes for carriers until effects need float — convert to float lazily
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
    - Implement state warmup on seek (process from frame N-K)
    - Mark warmup frames in UI
    - Build and test with feedback delay node

12. **Cache validity tracking**:

    - Add state_hash and computed_with_state_from to CachedFrame
    - Implement state continuity checking
    - Intelligent cache invalidation on seeks and graph changes

13. **State snapshot system** (optional optimization):

    - Implement node state serialization
    - Periodic snapshot creation (every N frames)
    - Snapshot-based seeking

14. **Interactive editing optimizations**:

    - Detect active editing mode (frequent parameter changes)
    - Adaptive caching strategy (small lookahead during edits)
    - Aggressive forward caching after editing stops

15. **Performance optimization**:
    - SIMD for CPU-based effects
    - GPU compute shaders for heavy operations
    - Lazy float conversion for carriers
    - Memory pooling for sample buffers
