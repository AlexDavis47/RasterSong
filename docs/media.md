# Media layer

FFmpeg is used for decoding and encoding, and it lives entirely inside `rastersong-media` behind a small trait.
No FFmpeg type appears in any other crate. Tests for the engine and graph use a fake backend that produces
synthetic frames. (Licensing of FFmpeg itself is in [Licensing](licensing.md#ffmpeg).)

- **Open/probe:** builds a frame index from **packets only** (no full decode), sorted by presentation time.
  Frames that can't be decoded from a clean start are left out of the index: packets before the first keyframe,
  the leading frames of an open GOP at the start of a cut stream, and packets the demuxer flags as corrupt
  (e.g. the cut-off end of a truncated file).
- **API by frame index:** `frame(i) -> Arc<VideoFrame>` (packed 8-bit RGB). Callers never see GOPs, keyframes
  or timestamps. Requesting the same frame twice is free. `frame_time(i)` gives each frame's presentation time,
  which is how variable-frame-rate sources are placed on the timeline.
- **Sequential fast path:** the decoder keeps going forward when it is already at or before the keyframe a seek
  would land on. It only seeks on a jump.
- **Seeking:** each frame records which keyframe decoding must start from. For the leading B-frames of an open GOP
  this is the *previous* keyframe, since their references are in the previous GOP. Demuxers seek imprecisely, so
  every seek is verified: if decoding would start after the needed keyframe, it retries by decode timestamp
  (MPEG-TS seeks by DTS), then at earlier keyframes, and finally reopens the file. The source remembers which
  kind of seek works. The decoder is drained at end of stream so trailing frames are never lost.
- **Conversion:** one reused scaler per source using FFmpeg's `sws_scale_frame`, which picks the color matrix
  and range from each frame. Output is at the requested size (project/preview resolution). Display-matrix
  rotation (phone video) is applied so frames come out upright.
- **Audio:** decoded in full to interleaved `f32` via swresample, optionally resampled and remixed. Encoder
  priming samples are trimmed using the container's edit list. Decoding hands the samples on in pieces
  (`MediaBackend::decode_audio`), so they can go straight to a file.
- **Audio cache:** `AudioCache` decodes each audio stream once into an uncompressed `f32` file and reads it back
  through a memory map (`Samples`), so a track costs address space rather than memory, every reader shares one
  copy, and reopening a project decodes nothing. Files are named by the SHA-256 of the source's bytes (remembered
  against its path, size and modified time, so a long video isn't hashed on every open) and the decoding options,
  and are written under a temporary name and renamed into place. The cache lives in `%LOCALAPPDATA%\RasterSong\cache`
  on Windows, `~/Library/Caches/RasterSong` on macOS and `$XDG_CACHE_HOME/rastersong` (or `~/.cache/rastersong`)
  elsewhere; `RASTERSONG_CACHE_DIR` overrides it. A missing file is decoded again, a damaged one rebuilt, and a
  cache that can't be written falls back to decoding into memory. Uncompressed WAVs are decoded to the cache like
  any other file for now rather than mapped directly.
- **Latest-wins requests:** a video source is a synchronous object. Stale work is dropped by the engine's render
  thread (see [Cancellation](engine.md#always-rendering-ahead)).
