//! Main application structure for RasterSong EGUI frontend

use eframe::egui;
use rastersong::media::{self, MediaId, VideoFrame};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Main application state and UI
pub struct RasterSongApp {
    // Video state
    video_id: Option<MediaId>,
    video_path: Option<PathBuf>,
    video_duration: f64,
    video_fps: f64,
    video_width: u32,
    video_height: u32,

    // Playback state
    is_playing: bool,
    current_time: f64,
    last_frame_time: Option<Instant>,

    // Display state
    current_frame_texture: Option<egui::TextureHandle>,
    error_message: Option<String>,
}

impl Default for RasterSongApp {
    fn default() -> Self {
        Self {
            video_id: None,
            video_path: None,
            video_duration: 0.0,
            video_fps: 30.0,
            video_width: 0,
            video_height: 0,
            is_playing: false,
            current_time: 0.0,
            last_frame_time: None,
            current_frame_texture: None,
            error_message: None,
        }
    }
}

impl RasterSongApp {
    fn load_video(&mut self, path: PathBuf) {
        self.error_message = None;

        match media::load_media(&path) {
            Ok(media_id) => {
                // Get video info
                if let Some((width, height, fps)) = media::get_video_info(&media_id) {
                    if let Some((_, _, duration)) = media::get_media_info(&media_id) {
                        self.video_id = Some(media_id);
                        self.video_path = Some(path);
                        self.video_width = width;
                        self.video_height = height;
                        self.video_fps = fps;
                        self.video_duration = duration;
                        self.current_time = 0.0;
                        self.is_playing = false;
                        self.last_frame_time = None;
                        self.current_frame_texture = None;
                        // Load first frame immediately
                        // Note: We'll need to update the frame in the UI update loop
                    } else {
                        self.error_message = Some("Failed to get video duration".to_string());
                    }
                } else {
                    self.error_message = Some("Failed to get video info".to_string());
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to load video: {}", e));
            }
        }
    }

    fn update_frame(&mut self, ctx: &egui::Context) {
        if let Some(video_id) = &self.video_id {
            // Decode the frame at the current timestamp
            match media::decode_frame(video_id, self.current_time) {
                Ok(frame) => {
                    self.display_frame(ctx, &frame);
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to decode frame: {}", e));
                }
            }
        }
    }

    fn display_frame(&mut self, ctx: &egui::Context, frame: &VideoFrame) {
        let (width, height) = frame.dimensions();
        let data = frame.data();

        // Convert RGBA bytes to egui::ColorImage
        let pixels: Vec<egui::Color32> = data
            .chunks_exact(4)
            .map(|chunk| {
                egui::Color32::from_rgba_unmultiplied(chunk[0], chunk[1], chunk[2], chunk[3])
            })
            .collect();

        let size = [width as usize, height as usize];
        let color_image = egui::ColorImage {
            size,
            pixels,
            source_size: egui::Vec2::new(width as f32, height as f32),
        };

        // Create or update texture
        if let Some(texture) = &mut self.current_frame_texture {
            texture.set(color_image, egui::TextureOptions::LINEAR);
        } else {
            let texture =
                ctx.load_texture("video_frame", color_image, egui::TextureOptions::LINEAR);
            self.current_frame_texture = Some(texture);
        }
    }

    fn update_playback(&mut self, ctx: &egui::Context) {
        if !self.is_playing || self.video_id.is_none() {
            return;
        }

        let now = Instant::now();
        if let Some(last_time) = self.last_frame_time {
            let elapsed = now.duration_since(last_time).as_secs_f64();
            self.current_time += elapsed;

            if self.current_time >= self.video_duration {
                self.current_time = self.video_duration;
                self.is_playing = false;
            }
        }
        self.last_frame_time = Some(now);

        // Update frame
        self.update_frame(ctx);

        // Request repaint for smooth playback
        ctx.request_repaint_after(Duration::from_secs_f64(1.0 / self.video_fps));
    }
}

impl eframe::App for RasterSongApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Update playback if playing
        self.update_playback(ctx);

        // Load initial frame if video is loaded but no frame is displayed
        if self.video_id.is_some() && self.current_frame_texture.is_none() {
            self.update_frame(ctx);
        }

        // Top menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Load Video").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter(
                            "Video Files",
                            &["mp4", "avi", "mov", "mkv", "webm", "flv", "wmv", "m4v"],
                        )
                        .add_filter("All Files", &["*"])
                        .pick_file()
                    {
                        self.load_video(path);
                    }
                }

                if let Some(path) = &self.video_path {
                    ui.label(format!(
                        "Video: {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ));
                }
            });
        });

        // Main content area
        egui::CentralPanel::default().show(ctx, |ui| {
            // Show error if any
            if let Some(error) = &self.error_message {
                ui.colored_label(egui::Color32::RED, format!("Error: {}", error));
            }

            // Video display area
            if let Some(texture) = &self.current_frame_texture {
                let available_size = ui.available_size();
                let texture_size = texture.size_vec2();

                // Scale to fit while maintaining aspect ratio
                let scale = (available_size.x / texture_size.x)
                    .min(available_size.y / texture_size.y)
                    .min(1.0); // Don't scale up

                let display_size = texture_size * scale;

                ui.centered_and_justified(|ui| {
                    ui.image((texture.id(), display_size));
                });

                // Video info
                ui.separator();
                ui.label(format!(
                    "Size: {}x{} | FPS: {:.2} | Duration: {:.2}s",
                    self.video_width, self.video_height, self.video_fps, self.video_duration
                ));
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("No video loaded. Click 'Load Video' to select a file.");
                });
            }
        });

        // Playback controls at bottom
        if self.video_id.is_some() {
            egui::TopBottomPanel::bottom("playback_controls").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Play/Pause button
                    if ui
                        .button(if self.is_playing {
                            "⏸ Pause"
                        } else {
                            "▶ Play"
                        })
                        .clicked()
                    {
                        self.is_playing = !self.is_playing;
                        if self.is_playing {
                            self.last_frame_time = Some(Instant::now());
                        }
                    }

                    // Stop button
                    if ui.button("⏹ Stop").clicked() {
                        self.is_playing = false;
                        self.current_time = 0.0;
                        self.last_frame_time = None;
                        if self.video_id.is_some() {
                            self.update_frame(ctx);
                        }
                    }

                    // Time slider
                    let slider_response = ui.add(
                        egui::Slider::new(&mut self.current_time, 0.0..=self.video_duration)
                            .text("Time")
                            .custom_formatter(|n, _| format!("{:.2}s", n)),
                    );

                    // Update frame if slider was dragged
                    if slider_response.drag_stopped() || slider_response.changed() {
                        self.update_frame(ctx);
                    }

                    // Time display
                    ui.label(format!(
                        "{:.2}s / {:.2}s",
                        self.current_time, self.video_duration
                    ));
                });
            });
        }
    }
}
