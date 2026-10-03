//! Developer tasks. Run with `cargo xtask <task>`.

mod ffmpeg;
mod fixtures;
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
    /// Generate the media test fixtures into fixtures/ using the fetched FFmpeg.
    Fixtures,
}

fn main() -> Result<()> {
    match Xtask::parse().task {
        Task::FetchFfmpeg { force } => ffmpeg::fetch(force),
        Task::Fixtures => fixtures::generate(),
    }
}
