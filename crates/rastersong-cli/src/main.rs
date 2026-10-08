mod output;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use rastersong_engine::playback::MixTrack;
use rastersong_engine::sources::Modulator;
use rastersong_engine::{
    Bus, DEFAULT_AUDIO_TRACK, FfmpegBackend, GraphDesc, LayerSet, MediaBackend, Project,
    RenderSettings, RenderTrack, Timeline, TrackKind, TrackSpec, VIDEO_SOURCE,
};
use rastersong_lang::tr_args;

use crate::output::SourceTrack;
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
    /// Render a project, or a video through a graph modulated by an audio file.
    Render(RenderArgs),
}

#[derive(Debug, clap::Args)]
struct RenderArgs {
    /// Either `<project> <out>`, or `<video> <audio> <graph> <out>`: a video as the track
    /// `video` and any file with an audio stream as the track `audio`, through a graph file
    /// (JSON).
    ///
    /// The output is a `.mkv` file (lossless FFV1 video with the master bus's sound: the graph's
    /// Audio Output for it, or else the mix of the audio tracks routed to it), or a directory to
    /// fill with a PNG sequence.
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
    /// The output bus whose sound is written: its Audio Output, or its track mix when the graph
    /// has none. Defaults to the project's first bus, the master.
    #[arg(long)]
    bus: Option<String>,
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

/// What to render: the project, as the render needs it.
struct Job {
    timeline: Timeline,
    /// The project's open graph: rendered over the whole timeline, or, with `layers`, the
    /// description of the open graph among the graph items.
    graph: GraphDesc,
    /// The project's graph items; `None` when it has none.
    layers: Option<LayerSet>,
    tempo: rastersong_engine::Tempo,
    audio_rate: u32,
}

/// A project on its own, from `rastersong render <project> <out>`.
fn project_job(path: &Path, args: &RenderArgs) -> Result<Job> {
    if args.audio_offset.is_some() {
        bail!(
            "--audio-offset only applies to a video, an audio file and a graph; move the audio track's item in the project instead"
        );
    }
    let project = Project::load(path).map_err(anyhow::Error::msg)?;
    Ok(Job {
        timeline: project.timeline(),
        audio_rate: args.audio_rate.unwrap_or(project.audio_rate),
        tempo: project.tempo,
        layers: project.layer_set(),
        graph: project.graph,
    })
}

/// A video, an audio file and a graph, from `rastersong render <video> <audio> <graph> <out>`.
fn files_job(video: &Path, audio: &Path, graph: &Path, args: &RenderArgs) -> Result<Job> {
    let json =
        std::fs::read_to_string(graph).with_context(|| format!("reading {}", graph.display()))?;
    let graph =
        GraphDesc::from_json(&json).with_context(|| format!("loading {}", graph.display()))?;
    let offset = args.audio_offset.unwrap_or(0.0);
    let mut song = TrackSpec::new(DEFAULT_AUDIO_TRACK, TrackKind::Audio, audio);
    // Starting earlier than the video starts the item partway into the audio.
    song.items[0].position = offset.max(0.0);
    song.items[0].start = (-offset).max(0.0);
    let video = TrackSpec::new(VIDEO_SOURCE, TrackKind::Video, video);
    Ok(Job {
        timeline: Timeline {
            tracks: vec![video, song],
            ..Timeline::default()
        },
        graph,
        layers: None,
        tempo: Default::default(),
        audio_rate: args
            .audio_rate
            .unwrap_or(rastersong_engine::DEFAULT_AUDIO_RATE),
    })
}

fn render(args: RenderArgs) -> Result<()> {
    let (job, out) = match args.paths.as_slice() {
        [project, out] => (project_job(project, &args)?, out),
        [video, audio, graph, out] => (files_job(video, audio, graph, &args)?, out),
        _ => bail!("expected `<project> <out>` or `<video> <audio> <graph> <out>`"),
    };

    let bus = match &args.bus {
        None => job.timeline.master(),
        Some(name) => job
            .timeline
            .buses
            .iter()
            .chain(job.timeline.buses.is_empty().then(Bus::main).as_ref())
            .find(|b| b.name == *name)
            .cloned()
            .with_context(|| format!("the project has no bus named `{name}`"))?,
    };

    let backend = FfmpegBackend::new()?;
    let audio_cache = rastersong_engine::AudioCache::in_user_dir();
    let mut tracks = Vec::new();
    let mut sources = Vec::new();
    for spec in &job.timeline.tracks {
        let media = match spec.kind {
            TrackKind::Video => rastersong_engine::TrackMedia::Video {
                path: spec.path.clone(),
                stream: spec.stream,
            },
            TrackKind::Audio => {
                let clip = match &audio_cache {
                    Some(cache) => cache.load(&backend, &spec.path, spec.audio_options()),
                    None => backend.load_audio(&spec.path, spec.audio_options()),
                }
                .with_context(|| format!("loading audio from {}", spec.path.display()))?;
                let modulator = Arc::new(Modulator::new(&clip));
                sources.push(SourceTrack {
                    name: spec.name.clone(),
                    bus: spec.bus.clone(),
                    track: MixTrack {
                        clip: Arc::new(clip),
                        items: spec.items.clone(),
                        gain: spec.gain,
                    },
                });
                rastersong_engine::TrackMedia::Audio(modulator)
            }
        };
        tracks.push(RenderTrack {
            name: spec.name.clone(),
            media,
            items: spec.items.clone(),
            in_mix: spec.gain > 0.0,
        });
    }

    let mut sink = output::Output::new(out, sources, job.audio_rate, bus.clone());
    let settings = RenderSettings {
        timebase: job.timeline.timebase,
        size: args.size,
        frames: args.frames,
        tempo: job.tempo,
        audio_rate: Some(job.audio_rate),
        bus,
        layers: job.layers.clone(),
    };
    let started = std::time::Instant::now();
    let info = rastersong_engine::render(&backend, &tracks, &job.graph, &settings, &mut sink)?;
    sink.finish()?;

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
