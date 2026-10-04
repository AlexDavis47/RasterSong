mod output;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rastersong_engine::{AudioOptions, FfmpegBackend, GraphDesc, MediaBackend, RenderSettings};
use tracing_subscriber::EnvFilter;

/// Headless RasterSong renderer.
#[derive(Debug, Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print version and media backend information.
    Info,
    /// Render a video through a graph, modulated by an audio file.
    Render(RenderArgs),
}

#[derive(Debug, clap::Args)]
struct RenderArgs {
    /// Source video.
    video: PathBuf,
    /// Modulator audio: any file with an audio stream.
    audio: PathBuf,
    /// Graph file (JSON).
    graph: PathBuf,
    /// Output: a `.mkv` file (lossless FFV1 video with the modulator as its audio track), or a
    /// directory to fill with a PNG sequence.
    out: PathBuf,
    /// Process and output at this size instead of the video's, e.g. `320x180`.
    #[arg(long, value_parser = parse_size)]
    size: Option<(u32, u32)>,
    /// Render only the first N frames.
    #[arg(long)]
    frames: Option<usize>,
    /// Seconds the audio starts after the video (negative to start it earlier).
    #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
    audio_offset: f64,
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .ok_or("expected WIDTHxHEIGHT, e.g. 320x180")?;
    let parse = |v: &str| {
        v.trim()
            .parse::<u32>()
            .ok()
            .filter(|&n| n > 0)
            .ok_or(format!("`{v}` is not a positive whole number"))
    };
    Ok((parse(w)?, parse(h)?))
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    match Cli::parse().command {
        Command::Info => info(),
        Command::Render(args) => render(args),
    }
}

fn info() -> Result<()> {
    let backend = rastersong_engine::init()?;
    println!("RasterSong {}", env!("CARGO_PKG_VERSION"));
    println!("FFmpeg ({})", backend.license());
    for lib in &backend.libraries {
        println!("  {:<11} {}", lib.name, lib.version);
    }
    println!("  configuration: {}", backend.configuration);
    Ok(())
}

fn render(args: RenderArgs) -> Result<()> {
    let graph_json = std::fs::read_to_string(&args.graph)
        .with_context(|| format!("reading {}", args.graph.display()))?;
    let graph = GraphDesc::from_json(&graph_json)
        .with_context(|| format!("loading {}", args.graph.display()))?;

    let backend = FfmpegBackend::new()?;
    let audio = backend
        .load_audio(&args.audio, AudioOptions::default())
        .with_context(|| format!("loading audio from {}", args.audio.display()))?;

    let mut sink = output::Output::new(&args.out, &audio);
    let settings = RenderSettings {
        size: args.size,
        frames: args.frames,
        audio_offset: args.audio_offset,
        tempo: Default::default(),
    };
    let started = std::time::Instant::now();
    let info =
        rastersong_engine::render(&backend, &args.video, &audio, &graph, &settings, &mut sink)?;
    sink.finish()?;

    let seconds = started.elapsed().as_secs_f64();
    eprintln!(
        "Rendered {} frames at {}x{} in {seconds:.1} s ({:.1} fps) to {}",
        info.frames,
        info.width,
        info.height,
        info.frames as f64 / seconds.max(1e-9),
        args.out.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_size;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("320x180"), Ok((320, 180)));
        assert_eq!(parse_size("64X48"), Ok((64, 48)));
        assert!(parse_size("320").is_err());
        assert!(parse_size("0x10").is_err());
        assert!(parse_size("ax10").is_err());
    }
}
