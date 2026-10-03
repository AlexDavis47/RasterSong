#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use tracing_subscriber::EnvFilter;

/// Our crates at info, dependencies (wgpu in particular is very chatty) at warn.
const DEFAULT_LOG_FILTER: &str =
    "warn,rastersong=info,rastersong_engine=info,rastersong_media=info";

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER)),
        )
        .init();

    let backend = rastersong_engine::init()?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "RasterSong",
        options,
        Box::new(|_cc| Ok(Box::new(App { backend }))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

#[derive(Debug)]
struct App {
    backend: rastersong_engine::BackendInfo,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(format!("RasterSong {}", env!("CARGO_PKG_VERSION")));
            ui.label(format!(
                "FFmpeg avcodec {} ({})",
                self.backend.library("avcodec").version,
                self.backend.license()
            ));
        });
    }
}
