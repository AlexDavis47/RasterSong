mod components;

use components::{Controls, EffectsPanel, Timeline, VideoViewer};
use iced::widget::{Column, container, row};
use iced::{Element, Length, Task};
use rastersong::{EffectSettings, MappingMode, MediaPlayer, VideoSignalProcessor};
use std::path::PathBuf;
use std::time::Duration;

fn main() -> iced::Result {
    // Initialize the core library
    rastersong::init();

    iced::application("RasterSong", update, view).run_with(new)
}

struct App {
    player: Option<MediaPlayer>,
    processor: Option<VideoSignalProcessor>,
    effects: EffectSettings,
    current_frame: Option<iced::widget::image::Handle>,
    is_playing: bool,
    position: f32, // Position in seconds
    duration: f32, // Duration in seconds
    video_path: Option<PathBuf>,
    video_width: u32,
    video_height: u32,
    video_fps: f32,
    preview_effects: bool,     // Enable real-time effects preview (expensive!)
    mapping_mode: MappingMode, // Accurate or Bugged encoding mode
}

#[derive(Debug, Clone)]
enum Message {
    LoadVideo(String),
    OpenModulatorDialog,
    LoadModulator(String),
    PlayPause,
    Seek(f32),
    EffectsChanged(EffectSettings),
    TogglePreview(bool),
    ToggleMappingMode(bool), // true = Bugged, false = Accurate
    Tick,
}

fn new() -> (App, Task<Message>) {
    (
        App {
            player: None,
            processor: None,
            effects: EffectSettings::default(),
            current_frame: None,
            is_playing: false,
            position: 0.0,
            duration: 0.0,
            video_path: None,
            video_width: 0,
            video_height: 0,
            video_fps: 30.0,
            preview_effects: false, // Disabled by default for performance
            mapping_mode: MappingMode::Bugged, // Default to Bugged for glitchy effects
        },
        Task::done(Message::Tick),
    )
}

fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::LoadVideo(path) => {
            match MediaPlayer::new(&path) {
                Ok(player) => {
                    // Get duration (pipeline is already in PAUSED state)
                    if let Some(dur_ns) = player.get_duration() {
                        app.duration = dur_ns as f32 / 1_000_000_000.0;
                        println!("Loaded video: duration = {:.2}s", app.duration);
                    } else {
                        eprintln!("Warning: Could not get video duration");
                    }

                    app.player = Some(player);
                    app.video_path = Some(PathBuf::from(path));
                    app.position = 0.0;
                    app.is_playing = false;

                    // Update frame - this will create the processor when first frame is available
                    update_frame(app);

                    // If we still don't have dimensions, we'll get them on first actual frame
                    if app.video_width == 0 {
                        println!("⚠ Waiting for first frame to determine video dimensions...");
                    }
                }
                Err(e) => {
                    eprintln!("Failed to load video: {}", e);
                }
            }
            Task::none()
        }
        Message::OpenModulatorDialog => {
            // Open file picker in async task
            Task::future(async {
                if let Some(path) = rfd::AsyncFileDialog::new()
                    .add_filter("Audio Files", &["wav"])
                    .set_title("Select Modulator Audio File")
                    .pick_file()
                    .await
                {
                    Message::LoadModulator(path.path().display().to_string())
                } else {
                    Message::Tick // User cancelled
                }
            })
        }
        Message::LoadModulator(path) => {
            println!("Loading modulator audio: {}", path);

            // Check if we have a processor
            if app.processor.is_none() {
                println!("ERROR: No processor! Load a video first (play the video to initialize).");
                return Task::none();
            }

            if let Some(processor) = &mut app.processor {
                // Load audio file
                match rastersong::audio_loader::load_audio_file(&path) {
                    Ok(audio_data) => {
                        processor.load_modulator(audio_data);
                        println!("✓ Loaded modulator from: {}", path);
                        println!("✓ Processor has modulator: {}", processor.has_modulator());

                        // Re-render current frame with effects
                        update_frame(app);
                    }
                    Err(e) => {
                        eprintln!("ERROR: Failed to load audio file: {}", e);
                    }
                }
            }
            Task::none()
        }
        Message::PlayPause => {
            if let Some(player) = &app.player {
                if app.is_playing {
                    println!("Pausing playback");
                    let _ = player.pause();
                    app.is_playing = false;
                } else {
                    println!("Starting playback");
                    let _ = player.play();
                    app.is_playing = true;
                }
            }
            Task::none()
        }
        Message::Seek(position) => {
            if let Some(player) = &app.player {
                let position_ns = (position * 1_000_000_000.0) as u64;
                let _ = player.seek(position_ns);
                app.position = position;
                update_frame(app);
            }
            Task::none()
        }
        Message::EffectsChanged(new_effects) => {
            app.effects = new_effects;
            // Re-render current frame with new effects if preview enabled or paused
            if app.preview_effects || !app.is_playing {
                update_frame(app);
            }
            Task::none()
        }
        Message::TogglePreview(enabled) => {
            app.preview_effects = enabled;
            if enabled {
                println!("⚠ Live preview enabled - may cause performance issues!");
            } else {
                println!("✓ Live preview disabled - effects only shown when paused");
            }
            // Update frame to show/hide effects immediately
            update_frame(app);
            Task::none()
        }
        Message::ToggleMappingMode(use_bugged) => {
            app.mapping_mode = if use_bugged {
                MappingMode::Bugged
            } else {
                MappingMode::Accurate
            };

            println!("Switched to {:?} mode", app.mapping_mode);

            // Recreate processor with new mapping mode if video is loaded
            if app.video_width > 0 && app.video_height > 0 {
                let new_processor = VideoSignalProcessor::new(
                    app.video_width,
                    app.video_height,
                    app.video_fps,
                    app.mapping_mode,
                );

                // Preserve modulator if one was loaded
                if let Some(ref old_processor) = app.processor {
                    if old_processor.has_modulator() {
                        println!(
                            "⚠ Note: Modulator preserved but you may need to reload for best results"
                        );
                    }
                }

                app.processor = Some(new_processor);

                // Re-render frame with new mode
                update_frame(app);
            }

            Task::none()
        }
        Message::Tick => {
            if app.is_playing {
                // Update position
                if let Some(player) = &app.player {
                    if let Some(pos_ns) = player.get_position() {
                        app.position = pos_ns as f32 / 1_000_000_000.0;
                    }
                }

                // Update frame
                update_frame(app);

                // Stop at end (only if we have a valid duration)
                if app.duration > 0.0 && app.position >= app.duration {
                    println!("Reached end of video, stopping playback");
                    app.is_playing = false;
                    if let Some(player) = &app.player {
                        let _ = player.pause();
                    }
                }
            }

            // Schedule next tick
            Task::future(async {
                tokio::time::sleep(Duration::from_millis(33)).await; // ~30 FPS
                Message::Tick
            })
        }
    }
}

fn update_frame(app: &mut App) {
    if let Some(player) = &app.player {
        if let Some(mut frame) = player.get_current_frame() {
            // Initialize processor if we haven't yet (now that we have frame dimensions)
            if app.processor.is_none() && app.video_width == 0 {
                app.video_width = frame.width;
                app.video_height = frame.height;
                println!(
                    "✓ Video dimensions: {}x{} @ {} fps",
                    app.video_width, app.video_height, app.video_fps
                );

                // Create video signal processor with current mapping mode
                let processor = VideoSignalProcessor::new(
                    app.video_width,
                    app.video_height,
                    app.video_fps,
                    app.mapping_mode,
                );
                app.processor = Some(processor);
                println!(
                    "✓ Created video signal processor (mode: {:?})",
                    app.mapping_mode
                );
            }

            // Apply effects only if:
            // 1. Preview is enabled during playback, OR
            // 2. Video is paused (not playing)
            let should_process_effects = app.preview_effects || !app.is_playing;

            if should_process_effects {
                if let Some(processor) = &app.processor {
                    if processor.has_modulator() {
                        // Process frame with effects at current time position
                        match processor.process_frame_at_time(
                            &frame.data,
                            &app.effects,
                            app.position,
                        ) {
                            Ok(processed_data) => {
                                frame.data = processed_data;
                            }
                            Err(e) => {
                                eprintln!("Error processing frame: {}", e);
                            }
                        }
                    }
                }
            }

            app.current_frame = Some(iced::widget::image::Handle::from_rgba(
                frame.width,
                frame.height,
                frame.data,
            ));
        }
    }
}

fn view(app: &App) -> Element<'_, Message> {
    let main_content = Column::new()
        .spacing(10)
        .padding(20)
        .push(VideoViewer::view(&app.current_frame))
        .push(Controls::view(
            app.is_playing,
            app.player.is_some(),
            Message::PlayPause,
            Message::LoadVideo("test.mp4".to_string()),
        ))
        .push(Timeline::view(app.position, app.duration, Message::Seek));

    // Side panel with effects
    let has_modulator = app.processor.as_ref().map_or(false, |p| p.has_modulator());

    // File picker button
    let load_button =
        iced::widget::button("Load Modulator Audio (.wav)").on_press(Message::OpenModulatorDialog);

    // Preview toggle with warning
    let preview_toggle = row![
        iced::widget::checkbox("Live Preview", app.preview_effects)
            .on_toggle(Message::TogglePreview),
        iced::widget::text(if app.preview_effects {
            " ⚠ Performance impact!"
        } else {
            " (Effects when paused)"
        })
        .size(12)
        .color(if app.preview_effects {
            iced::Color::from_rgb(1.0, 0.6, 0.0)
        } else {
            iced::Color::from_rgb(0.6, 0.6, 0.6)
        })
    ]
    .spacing(5)
    .align_y(iced::Alignment::Center);

    // Mapping mode toggle
    let is_bugged = matches!(app.mapping_mode, MappingMode::Bugged);
    let mapping_toggle = row![
        iced::widget::checkbox("Bugged Mode", is_bugged).on_toggle(Message::ToggleMappingMode),
        iced::widget::text(if is_bugged {
            " (Glitchy)"
        } else {
            " (Accurate)"
        })
        .size(12)
        .color(iced::Color::from_rgb(0.6, 0.6, 0.6))
    ]
    .spacing(5)
    .align_y(iced::Alignment::Center);

    let side_panel = Column::new()
        .spacing(10)
        .padding(20)
        .push(load_button)
        .push(
            iced::widget::text(format!(
                "Status: Processor={} Modulator={}",
                if app.processor.is_some() {
                    "✓"
                } else {
                    "✗"
                },
                if has_modulator { "✓" } else { "✗" }
            ))
            .size(12),
        )
        .push(preview_toggle)
        .push(mapping_toggle)
        .push(EffectsPanel::view(
            &app.effects,
            has_modulator,
            Message::EffectsChanged,
        ));

    let layout = row![
        container(main_content)
            .width(Length::FillPortion(7))
            .height(Length::Fill),
        container(side_panel)
            .width(Length::FillPortion(3))
            .height(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.1, 0.1, 0.1,
                ))),
                ..Default::default()
            }),
    ];

    container(layout).into()
}
