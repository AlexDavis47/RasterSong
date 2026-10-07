//! Developer tasks. Run with `cargo xtask <task>`.

mod dist;
mod docs;
mod ffmpeg;
mod fixtures;
mod fmt;
mod release_ffmpeg;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
struct Xtask {
    #[command(subcommand)]
    task: Task,
}

#[derive(Debug, Subcommand)]
enum Task {
    /// Download (or on macOS, build) the pinned LGPL FFmpeg into third_party/ffmpeg.
    FetchFfmpeg {
        /// Reinstall even if the pinned version is already present.
        #[arg(long)]
        force: bool,
    },
    /// Build the minimal LGPL FFmpeg that release packages ship into third_party/ffmpeg-release.
    BuildFfmpeg {
        /// Rebuild even if the configuration hasn't changed.
        #[arg(long)]
        force: bool,
    },
    /// Generate the media test fixtures into fixtures/ using the fetched FFmpeg.
    Fixtures,
    /// Render the node reference (docs/nodes.md) from the node definitions.
    Docs {
        /// Fail instead of writing if the file is out of date (for CI).
        #[arg(long)]
        check: bool,
    },
    /// Format the workspace and the node files `cargo fmt` can't reach, as CI checks them.
    Fmt {
        /// Fail instead of writing if anything is unformatted.
        #[arg(long)]
        check: bool,
    },
    /// Build the app in release mode with the release FFmpeg and package it for testers in
    /// target/dist: a zip on Windows, a universal app bundle zip on macOS, an AppImage on Linux.
    Dist,
}

fn main() -> Result<()> {
    match Xtask::parse().task {
        Task::FetchFfmpeg { force } => ffmpeg::fetch(force),
        Task::BuildFfmpeg { force } => release_ffmpeg::build(force).map(drop),
        Task::Fixtures => fixtures::generate(),
        Task::Docs { check } => docs::generate(check),
        Task::Fmt { check } => fmt::format(check),
        Task::Dist => dist::package(),
    }
}
