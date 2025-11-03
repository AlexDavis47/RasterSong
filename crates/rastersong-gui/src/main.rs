mod components;

use components::{Controls, Timeline, VideoViewer};
use iced::widget::{Column, container};
use iced::{Element, Task};
use rastersong::MediaPlayer;
use std::path::PathBuf;
use std::time::Duration;

fn main() -> iced::Result {
    // Initialize the core library
    rastersong::init();

    iced::application("RasterSong", update, view).run_with(new)
}

struct App {
    player: Option<MediaPlayer>,
    current_frame: Option<iced::widget::image::Handle>,
    is_playing: bool,
    position: f32, // Position in seconds
    duration: f32, // Duration in seconds
    video_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
enum Message {
    LoadVideo(String),
    PlayPause,
    Seek(f32),
    Tick,
}

fn new() -> (App, Task<Message>) {
    (
        App {
            player: None,
            current_frame: None,
            is_playing: false,
            position: 0.0,
            duration: 0.0,
            video_path: None,
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

                    // Update frame immediately to show first frame
                    update_frame(app);
                }
                Err(e) => {
                    eprintln!("Failed to load video: {}", e);
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
        if let Some(frame) = player.get_current_frame() {
            app.current_frame = Some(iced::widget::image::Handle::from_rgba(
                frame.width,
                frame.height,
                frame.data,
            ));
        }
    }
}

fn view(app: &App) -> Element<'_, Message> {
    let mut content = Column::new().spacing(10).padding(20);

    // Video viewer component
    content = content.push(VideoViewer::view(&app.current_frame));

    // Controls component
    content = content.push(Controls::view(
        app.is_playing,
        app.player.is_some(),
        Message::PlayPause,
        Message::LoadVideo("test.mp4".to_string()),
    ));

    // Timeline component
    content = content.push(Timeline::view(app.position, app.duration, Message::Seek));

    container(content).into()
}
