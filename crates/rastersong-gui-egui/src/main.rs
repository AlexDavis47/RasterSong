#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(rustdoc::missing_crate_level_docs)] // it's an example

use eframe::egui;
use rastersong::init as rastersong_init;
use rastersong_gui_egui::RasterSongApp;

fn main() -> eframe::Result {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).

    // Initialize RasterSong library
    if let Err(e) = rastersong_init() {
        eprintln!("Failed to initialize RasterSong: {}", e);
        // Continue anyway - the app can still run without full initialization
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("RasterSong"),
        ..Default::default()
    };

    eframe::run_native(
        "RasterSong",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);

            Ok(Box::<RasterSongApp>::default())
        }),
    )
}
