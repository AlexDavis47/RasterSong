mod output;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use rastersong_engine::export::{ExportJob, ExportSettings, export};
use rastersong_engine::{
    AudioCache, AudioOptions, FfmpegBackend, FormatKind, GraphDesc, MediaBackend, Project,
};
use rastersong_lang::tr_args;
use tracing_subscriber::filter::{EnvFilter, LevelFilter, Targets};
use tracing_subscriber::prelude::*;

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
    /// Render a project, or a video through a graph modulated by an audio file.
    Render(RenderArgs),
}

#[derive(Debug, clap::Args)]
struct RenderArgs {
    /// Either `<project> <out>`, or `<video> <audio> <graph> <out>`: a project of a video on the
    /// track `Video` and any file with an audio stream on the track `Audio`, under a graph file
    /// (JSON) for as long as either plays.
    ///
    /// The output is a video file with the bus's sound (what the graphs' Audio Outputs for it
    /// render, silence where a graph playing has none, or else the mix of the audio tracks
    /// routed to it where no graph plays), its format picked by the extension: `.mp4`
    /// (H.264 and AAC, through an OS or hardware encoder), `.mov` (ProRes HQ and 24-bit PCM)
    /// or `.mkv` (lossless FFV1 and 16-bit PCM). Any other path is a directory to fill with a
    /// PNG sequence.
    #[arg(num_args = 2..=4, required = true, value_names = ["INPUTS", "OUT"])]
    paths: Vec<PathBuf>,
    /// Process and output at this size instead of the project's, e.g. `320x180`.
    #[arg(long, value_parser = parse_size)]
    size: Option<(u32, u32)>,
    /// Render only the first N frames.
    #[arg(long)]
    frames: Option<usize>,
    /// Seconds the audio starts after the video (negative to start it partway into the audio).
    /// Only with a video, an audio file and a graph.
    #[arg(long, allow_negative_numbers = true)]
    audio_offset: Option<f64>,
    /// Sample rate of the sound (Hz). Defaults to the project's, or 48000.
    #[arg(long)]
    audio_rate: Option<u32>,
    /// The output bus whose sound is written: its graphs' Audio Outputs, or its track mix where no
    /// graph plays. Defaults to the project's first bus, the master.
    #[arg(long)]
    bus: Option<String>,
    /// Write a timeline of where the render spends its time to this file: each frame's decode,
    /// graph nodes, conversion and encoding, in Chrome's trace format. Open it in
    /// https://ui.perfetto.dev. Tracing slows the render a little.
    #[arg(long, value_name = "FILE")]
    trace: Option<PathBuf>,
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
    let cli = Cli::parse();
    let trace = match &cli.command {
        Command::Render(args) => args.trace.as_deref(),
        Command::Info => None,
    };
    // Flushes the trace file when dropped, at the end of `main`.
    let _trace = init_logging(trace);

    match cli.command {
        Command::Info => info(),
        Command::Render(args) => render(args),
    }
}

/// Logs warnings (or `RUST_LOG`) to stderr and, with `trace`, writes every RasterSong span to
/// that file as a Chrome trace. The returned guard finishes the file when dropped.
fn init_logging(trace: Option<&Path>) -> Option<tracing_chrome::FlushGuard> {
    let log = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")));
    let (chrome, guard) = match trace {
        Some(path) => {
            let (layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
                .file(path)
                .include_args(true)
                .build();
            let ours = Targets::new()
                .with_default(LevelFilter::OFF)
                .with_target("rastersong_engine", LevelFilter::TRACE)
                .with_target("rastersong_graph", LevelFilter::TRACE)
                .with_target("rastersong_media", LevelFilter::TRACE);
            (Some(layer.with_filter(ours)), Some(guard))
        }
        None => (None, None),
    };
    tracing_subscriber::registry().with(log).with(chrome).init();
    guard
}

fn info() -> Result<()> {
    let backend = rastersong_engine::init()?;
    println!(
        "{}",
        tr_args(
            "cli.info.version",
            &[("version", env!("CARGO_PKG_VERSION"))]
        )
    );
    println!(
        "{}",
        tr_args("cli.info.ffmpeg", &[("license", backend.license())])
    );
    for lib in &backend.libraries {
        println!("  {:<11} {}", lib.name, lib.version);
    }
    println!(
        "{}",
        tr_args(
            "cli.info.configuration",
            &[("value", backend.configuration)]
        )
    );
    Ok(())
}

/// A project on its own, from `rastersong render <project> <out>`.
fn project_job(path: &Path, args: &RenderArgs) -> Result<ExportJob> {
    if args.audio_offset.is_some() {
        bail!(
            "--audio-offset only applies to a video, an audio file and a graph; move the audio track's item in the project instead"
        );
    }
    let project = Project::load(path).map_err(anyhow::Error::msg)?;
    let mut job = ExportJob::from_project(&project);
    if let Some(rate) = args.audio_rate {
        job.audio_rate = rate;
    }
    Ok(job)
}

/// A video, an audio file and a graph, from `rastersong render <video> <audio> <graph> <out>`: a
/// project of the two files with the graph over all of them (see [`Project::from_files`]).
fn files_job(
    backend: &dyn MediaBackend,
    cache: Option<&AudioCache>,
    [video, audio, graph]: [&Path; 3],
    args: &RenderArgs,
) -> Result<ExportJob> {
    let json =
        std::fs::read_to_string(graph).with_context(|| format!("reading {}", graph.display()))?;
    let graph =
        GraphDesc::from_json(&json).with_context(|| format!("loading {}", graph.display()))?;
    let offset = args.audio_offset.unwrap_or(0.0);
    // The graph plays for as long as either file does: the video from the start, the audio from
    // its offset (starting partway into it when the offset is negative).
    let info = backend
        .open_video_stream(video, None)
        .with_context(|| format!("opening {}", video.display()))?
        .info()
        .clone();
    let video_length = info.frame_count as f64 / info.frame_rate.as_f64();
    let clip = match cache {
        Some(cache) => cache.load(backend, audio, AudioOptions::default()),
        None => backend.load_audio(audio, AudioOptions::default()),
    }
    .with_context(|| format!("decoding {}", audio.display()))?;
    let audio_length = offset.max(0.0) + (clip.duration_secs() - (-offset).max(0.0)).max(0.0);
    let length = video_length.max(audio_length);
    let project = Project::from_files(video, audio, graph, offset, length);
    let mut job = ExportJob::from_project(&project);
    if let Some(rate) = args.audio_rate {
        job.audio_rate = rate;
    }
    Ok(job)
}

fn render(args: RenderArgs) -> Result<()> {
    let backend = FfmpegBackend::new()?;
    let audio_cache = AudioCache::in_user_dir();
    let (job, out) = match args.paths.as_slice() {
        [project, out] => (project_job(project, &args)?, out),
        [video, audio, graph, out] => (
            files_job(&backend, audio_cache.as_ref(), [video, audio, graph], &args)?,
            out,
        ),
        _ => bail!("expected `<project> <out>` or `<video> <audio> <graph> <out>`"),
    };

    let bus = match &args.bus {
        None => job.composition.master(),
        Some(name) => job
            .bus(name)
            .with_context(|| format!("the project has no bus named `{name}`"))?,
    };

    let started = std::time::Instant::now();
    let format = out
        .extension()
        .and_then(|e| e.to_str())
        .and_then(FormatKind::from_extension);
    let info = match format {
        Some(kind) => export(
            &backend,
            audio_cache.as_ref(),
            &job,
            &ExportSettings {
                format: kind.default_format(),
                size: args.size,
                range: args.frames.map(|n| 0..n),
                bus,
            },
            out,
            None,
            &mut |_, _| true,
        )?,
        None => {
            let (audio, _) = job.load(&backend, audio_cache.as_ref())?;
            let settings = job.render_settings(bus, args.size, args.frames.map(|n| 0..n));
            let mut sink = output::PngSequence::new(out);
            rastersong_engine::render(
                &backend,
                &job.composition,
                &audio,
                &settings,
                &mut sink,
                &mut |_, _| true,
            )?
        }
    };

    let seconds = started.elapsed().as_secs_f64();
    eprintln!(
        "{}",
        tr_args(
            "cli.render.done",
            &[
                ("frames", &info.frames.to_string()),
                ("width", &info.width.to_string()),
                ("height", &info.height.to_string()),
                ("seconds", &format!("{seconds:.1}")),
                (
                    "fps",
                    &format!("{:.1}", info.frames as f64 / seconds.max(1e-9))
                ),
                ("out", &out.display().to_string()),
            ],
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use rastersong_engine::{AudioClip, FakeBackend, FakeVideo, Rational, TrackKind};

    use super::*;

    #[test]
    fn the_cli_files_render_goes_through_a_project() {
        let dir = std::env::temp_dir().join(format!("rastersong-cli-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let graph = dir.join("graph.json");
        std::fs::write(&graph, r#"{ "version": 0, "nodes": [] }"#).unwrap();
        // Two seconds of video and three of audio, starting one second in.
        let backend = FakeBackend::new()
            .with_video(
                "clip",
                FakeVideo {
                    width: 4,
                    height: 2,
                    frame_count: 20,
                    frame_rate: Rational::new(10, 1),
                },
            )
            .with_audio(
                "song",
                AudioClip {
                    sample_rate: 100,
                    channels: 1,
                    samples: vec![0.0; 300].into(),
                },
            );
        let Command::Render(args) =
            Cli::parse_from(["rastersong", "render", "--audio-offset", "1", "a", "b"]).command
        else {
            unreachable!()
        };
        let job = files_job(
            &backend,
            None,
            [Path::new("clip"), Path::new("song"), &graph],
            &args,
        )
        .unwrap();
        let tracks: Vec<_> = job
            .composition
            .tracks
            .iter()
            .map(|t| (t.name.as_str(), t.kind, t.items[0].position))
            .collect();
        assert_eq!(
            tracks,
            [
                ("Graph", TrackKind::Graph, 0.0),
                (rastersong_engine::VIDEO_SOURCE, TrackKind::Video, 0.0),
                (
                    rastersong_engine::DEFAULT_AUDIO_TRACK,
                    TrackKind::Audio,
                    1.0
                ),
            ]
        );
        // The graph covers both files: to the end of the audio, at 4 s.
        assert_eq!(job.composition.tracks[0].items[0].end, Some(4.0));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("320x180"), Ok((320, 180)));
        assert_eq!(parse_size("64X48"), Ok((64, 48)));
        assert!(parse_size("320").is_err());
        assert!(parse_size("0x10").is_err());
        assert!(parse_size("ax10").is_err());
    }
}
