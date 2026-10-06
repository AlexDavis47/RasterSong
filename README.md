# RasterSong

RasterSong is a video/audio editing tool with a focus on complex datamoshing.

Pull a kick drum out of your track, use it to amplitude-modulate the red channel, and the red channel waves
around with every hit. You build effects as a node graph (split, combine, amplitude modulation, delay, bit crush,
filters, compressor, gate and more), preview them live, and render the result to a video file.

> RasterSong is being rebuilt from the ground up and is in early testing. The 2025 prototype is preserved under the
> `prototype-2025` tag.

## Download

Test builds for Windows, macOS and Linux are on the [releases page](https://github.com/AlexDavis47/RasterSong/releases).

## Build from source

You need the Rust toolchain (pinned by `rust-toolchain.toml`) and libclang (on Windows: `winget install LLVM.LLVM`).

```sh
cargo xtask fetch-ffmpeg   # once: downloads the pinned LGPL FFmpeg build
cargo run --release -p rastersong-gui
```

Render without the app:

```sh
cargo run --release -p rastersong-cli -- render video.mp4 song.wav examples/graphs/am_bands.json out.mkv
```

More setup, testing and packaging details are in the [design document](docs/design.md).

## Documentation

- [Design document](docs/design.md): original concepts, architecture, node contract, testing, licensing and roadmap
- [Node reference](docs/nodes.md): every node's ports and parameters
- [Benchmarks](docs/benchmarks.md)
- [Example graphs](examples/graphs/)

## License

RasterSong is source-available, not open source. You can read, study and build it for personal use;
redistribution and commercial use need permission. See [LICENSE](LICENSE). RasterSong uses FFmpeg under the LGPL;
see the About screen in the app for details.
