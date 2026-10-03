use anyhow::Result;
use clap::{Parser, Subcommand};
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
