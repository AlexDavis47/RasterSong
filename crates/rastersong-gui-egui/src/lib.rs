//! RasterSong EGUI Frontend
//!
//! This module provides the EGUI-based graphical user interface for RasterSong.
//! It is designed to handle high-throughput real-time previews that Tauri could not support.

pub mod app;
pub mod widgets;

// Re-export main app type
pub use app::RasterSongApp;

