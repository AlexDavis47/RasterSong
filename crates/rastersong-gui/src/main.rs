#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use rastersong_engine::{FfmpegBackend, PROJECT_EXTENSION};
use rastersong_gui::{App, AudioOut, Settings};
use tracing_subscriber::EnvFilter;

/// Our crates at info, dependencies (wgpu in particular is very chatty) at warn.
const DEFAULT_LOG_FILTER: &str =
    "warn,rastersong=info,rastersong_gui=info,rastersong_engine=info,rastersong_media=info";

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER)),
        )
        .init();

    let backend_info = rastersong_engine::init()?;
    let backend = Arc::new(FfmpegBackend::new()?);
    // `rastersong <project or video>` opens it on startup.
    let open: Option<PathBuf> = std::env::args_os().nth(1).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "RasterSong",
        options,
        Box::new(move |cc| {
            let mut app = App::with_starter_project(backend, Some(backend_info), AudioOut::start());
            if let Some(settings) = cc
                .storage
                .and_then(|s| eframe::get_value::<Settings>(s, Settings::STORAGE_KEY))
            {
                app.set_settings(settings);
            }
            if let Some(path) = open {
                if path.extension().is_some_and(|e| e == PROJECT_EXTENSION) {
                    app.open_project(&path);
                } else {
                    app.open_video(path);
                }
            }
            Ok(Box::new(Window(app)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

struct Window(App);

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, Settings::STORAGE_KEY, self.0.settings());
    }
}
