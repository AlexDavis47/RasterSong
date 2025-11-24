//! Main application structure for RasterSong EGUI frontend

use eframe::egui;
use rastersong::media::{FrameReceiver, LoadMediaReceiver, MediaId, MediaStore, VideoFrame};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Main application state and UI
pub struct RasterSongApp {
    // Media Store
    media_store: MediaStore,

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

    // Async decode state
    pending_frame_receiver: Option<(f64, FrameReceiver)>,

    // Async loading state
    pending_load_receiver: Option<(PathBuf, LoadMediaReceiver)>,

    // Prefetch state
    last_prefetched_gop: Option<usize>,
}

impl Default for RasterSongApp {
    fn default() -> Self {
        Self {
            media_store: MediaStore::new(),
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
            pending_frame_receiver: None,
            pending_load_receiver: None,
            last_prefetched_gop: None,
        }
    }
}

impl RasterSongApp {
    fn load_video(&mut self, path: PathBuf) {
        self.error_message = None;
        self.pending_load_receiver = None;

        // Start async loading
        match self.media_store.load_media_async(&path) {
            Ok(receiver) => {
                // Store receiver to poll in update loop
                self.pending_load_receiver = Some((path.clone(), receiver));
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to start loading video: {}", e));
            }
        }
    }

    fn check_loading_status(&mut self, ctx: &egui::Context) {
        let receiver_result = self.pending_load_receiver.as_mut().map(|(path, receiver)| {
            (path.clone(), receiver.try_receive())
        });

        if let Some((path, result)) = receiver_result {
            match result {
                Ok(Some(loaded_data)) => {
                    let media_id = loaded_data.id;
                    println!("[GUI] Media loaded successfully! ID: {}", media_id);
                    
                    // Store the loaded media in the store
                    self.media_store.store_loaded_media(loaded_data);
                    
                    // Get video info
                    let media_info = self
                        .media_store
                        .get_media(&media_id)
                        .and_then(|f| f.video_info().map(|(w, h, fps)| (w, h, fps, f.duration())));

                    if let Some((width, height, fps, duration)) = media_info {
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
                        self.error_message = Some("Failed to get video info".to_string());
                    }
                    
                    // Clear loading receiver
                    self.pending_load_receiver = None;
                }
                Ok(None) => {
                    // Still loading, request repaint to check again soon
                    ctx.request_repaint_after(Duration::from_millis(16)); // ~60fps polling
                }
                Err(e) => {
                    println!("[GUI] Error loading media: {}", e);
                    self.error_message = Some(format!("Failed to load video: {}", e));
                    self.pending_load_receiver = None;
                }
            }
        }
    }

    fn update_frame(&mut self, ctx: &egui::Context) {
        if let Some(video_id) = &self.video_id {
            // Fast path: try to get frame directly from cache first
            let cached_frame = self
                .media_store
                .get_media(video_id)
                .and_then(|media_file| media_file.get_frame(self.current_time));

            if let Some(frame) = cached_frame {
                // Frame is cached, display immediately (<1ms)
                self.display_frame(ctx, &frame);
                self.pending_frame_receiver = None;
                return;
            }

            // Slow path: frame not cached, check if we have a pending decode
            let should_start_new_decode =
                if let Some((requested_time, receiver)) = &mut self.pending_frame_receiver {
                    // Check if this pending request is for the current timestamp
                    let is_for_current_time = (*requested_time - self.current_time).abs() < 0.001;

                    if is_for_current_time {
                        // Check if frame is ready (non-blocking)
                        match receiver.try_receive() {
                            Ok(Some(frame)) => {
                                println!("[GUI] Frame received from worker! Displaying frame");
                                self.display_frame(ctx, &frame);
                                self.pending_frame_receiver = None;
                                return;
                            }
                            Ok(None) => {
                                // Frame not ready yet, keep waiting (don't start new decode)
                                // Continue to show current frame if available
                                return;
                            }
                            Err(e) => {
                                println!("[GUI] Error receiving frame: {}", e);
                                self.error_message = Some(format!("Failed to decode frame: {}", e));
                                self.pending_frame_receiver = None;
                                true // Start new decode after error
                            }
                        }
                    } else {
                        // Timestamp changed, cancel old request and start new one
                        self.pending_frame_receiver = None;
                        true
                    }
                } else {
                    // No pending decode, start a new one
                    true
                };

            // Start a new decode if needed
            if should_start_new_decode {
                if let Some(media_file) = self.media_store.get_media(video_id) {
                    match media_file.decode_frame_async(self.current_time) {
                        Ok(receiver) => {
                            // Check immediately if frame is ready (decode_frame_async checks cache first,
                            // so cached frames will be available immediately)
                            match receiver.try_receive() {
                                Ok(Some(frame)) => {
                                    // Frame was cached and is ready immediately
                                    self.display_frame(ctx, &frame);
                                    self.pending_frame_receiver = None;
                                }
                                Ok(None) => {
                                    // Frame not ready yet, store receiver to poll later
                                    self.pending_frame_receiver =
                                        Some((self.current_time, receiver));
                                }
                                Err(e) => {
                                    println!("[GUI] Error receiving frame: {}", e);
                                    self.error_message =
                                        Some(format!("Failed to decode frame: {}", e));
                                    self.pending_frame_receiver = None;
                                }
                            }
                        }
                        Err(e) => {
                            println!("[GUI] Failed to start decode: {}", e);
                            self.error_message = Some(format!("Failed to start decode: {}", e));
                        }
                    }
                } else {
                    self.error_message = Some("Media file not found in store".to_string());
                }
            }
        }
    }

    /// Prefetch GOPs ahead of current playback position (manual prefetching)
    ///
    /// This prefetches the next 1-2 GOPs ahead of the current frame to ensure
    /// smooth playback. Only prefetches when playing forward, not when seeking.
    fn prefetch_gops_ahead(&mut self) {
        if !self.is_playing || self.video_id.is_none() {
            return;
        }

        // Get current GOP index first (without holding borrow)
        let current_gop = self
            .video_id
            .and_then(|id| self.media_store.get_media(&id))
            .and_then(|media_file| media_file.get_gop_index(self.current_time));

        // Only prefetch if we've moved to a new GOP
        if let Some(current_gop) = current_gop {
            if self.last_prefetched_gop != Some(current_gop) {
                self.last_prefetched_gop = Some(current_gop);

                // Prefetch next 1-2 GOPs ahead
                let prefetch_count = 2;
                for i in 1..=prefetch_count {
                    let gop_to_prefetch = current_gop + i;

                    // Check if already cached and start decode if needed
                    if let Some(media_file) =
                        self.video_id.and_then(|id| self.media_store.get_media(&id))
                    {
                        if !media_file.is_gop_decoded(gop_to_prefetch) {
                            // Start async GOP decode
                            if let Ok(_receiver) = media_file.decode_gop_async(gop_to_prefetch) {
                                println!("[GUI] Prefetching GOP {}", gop_to_prefetch);
                                // Note: We don't need to track these receivers since
                                // we only care that they're decoded, not when they complete
                            }
                        }
                    }
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

        // Prefetch GOPs ahead for smooth playback
        self.prefetch_gops_ahead();

        // Request repaint for smooth playback
        ctx.request_repaint_after(Duration::from_secs_f64(1.0 / self.video_fps));
    }
}

impl eframe::App for RasterSongApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for pending media loading
        self.check_loading_status(ctx);

        // Update playback if playing
        self.update_playback(ctx);

        // Always check for pending frames, even when playback is stopped
        // This ensures frames decode after seeking are displayed
        if self.pending_frame_receiver.is_some() {
            self.update_frame(ctx);
            // If still waiting for frame, request another repaint soon
            if self.pending_frame_receiver.is_some() {
                ctx.request_repaint_after(Duration::from_millis(16)); // ~60fps polling
            }
        }

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

            // Show loading indicator if loading
            if self.pending_load_receiver.is_some() {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.spinner();
                        ui.add_space(10.0);
                        if let Some((path, _)) = &self.pending_load_receiver {
                            ui.label(format!(
                                "Loading: {}",
                                path.file_name().unwrap_or_default().to_string_lossy()
                            ));
                        }
                        ui.label("Pre-processing media...");
                    });
                });
                return;
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
                        self.last_prefetched_gop = None; // Reset prefetch state
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

                    // Update frame when slider dragging stops (not on every change to avoid spam)
                    if slider_response.drag_stopped() {
                        // Reset prefetch state when seeking (manual control)
                        self.last_prefetched_gop = None;

                        // Check if pending receiver is for current timestamp and has frame ready
                        let frame_already_displayed = if let Some((requested_time, receiver)) =
                            &mut self.pending_frame_receiver
                        {
                            let is_for_current_time =
                                (*requested_time - self.current_time).abs() < 0.001;
                            if is_for_current_time {
                                // Check if frame is ready before canceling
                                match receiver.try_receive() {
                                    Ok(Some(frame)) => {
                                        // Frame is ready! Display it immediately
                                        self.display_frame(ctx, &frame);
                                        self.pending_frame_receiver = None;
                                        true // Frame already displayed
                                    }
                                    Ok(None) => {
                                        // Frame not ready yet, keep the receiver (don't cancel)
                                        false
                                    }
                                    Err(_) => {
                                        // Error, cancel and start new decode
                                        self.pending_frame_receiver = None;
                                        false
                                    }
                                }
                            } else {
                                // Different timestamp, cancel old request
                                self.pending_frame_receiver = None;
                                false
                            }
                        } else {
                            // No pending receiver
                            false
                        };

                        // Only call update_frame if we haven't already displayed the frame
                        if !frame_already_displayed {
                            // Force immediate frame update after seeking stops
                            // This ensures the frame is displayed even if playback is stopped
                            self.update_frame(ctx);
                        }
                        // Request repaint to ensure frame is displayed immediately
                        ctx.request_repaint();
                    } else if slider_response.changed() {
                        // While dragging, only update if frame is cached (fast path)
                        // This allows smooth seeking through cached frames without blocking
                        self.last_prefetched_gop = None;
                        if let Some(video_id) = &self.video_id {
                            if let Some(media_file) = self.media_store.get_media(video_id) {
                                if let Some(frame) = media_file.get_frame(self.current_time) {
                                    // Frame is cached, display immediately
                                    self.display_frame(ctx, &frame);
                                    // Cancel any pending decode since we have the frame
                                    self.pending_frame_receiver = None;
                                }
                                // If not cached, don't start decode while dragging (wait for drag_stopped)
                            }
                        }
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
