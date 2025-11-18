// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use serde::Serialize;
use std::sync::mpsc;
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
struct MediaInfo {
    path: String,
    id: String,
    duration: f64,
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn open_video_dialog(window: tauri::Window) -> Result<Option<MediaInfo>, String> {
    let (tx, rx) = mpsc::channel();

    window
        .dialog()
        .file()
        .add_filter(
            "Video Files",
            &["mp4", "avi", "mov", "mkv", "webm", "flv", "wmv", "m4v"],
        )
        .add_filter("All Files", &["*"])
        .pick_file(move |file_path| {
            let _ = tx.send(file_path);
        });

    let file_path = tauri::async_runtime::spawn_blocking(move || {
        rx.recv()
            .map_err(|_| "Failed to receive file path".to_string())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    match file_path {
        Some(path) => {
            let path_str = path.to_string();

            // Load the video file - this automatically extracts all metadata
            let media_id = rastersong::media::load_media(&path_str)
                .map_err(|e| format!("Failed to load video file: {}", e))?;

            // Get the duration from the loaded media
            let duration = rastersong::media::get_video_duration(media_id)
                .map_err(|e| format!("Failed to get video duration: {}", e))?;

            Ok(Some(MediaInfo {
                path: path_str,
                id: media_id.to_string(),
                duration,
            }))
        }
        None => Ok(None),
    }
}

#[tauri::command]
async fn open_audio_dialog(window: tauri::Window) -> Result<Option<MediaInfo>, String> {
    let (tx, rx) = mpsc::channel();

    window
        .dialog()
        .file()
        .add_filter(
            "Audio Files",
            &["wav", "mp3", "flac", "ogg", "aac", "m4a", "wma", "opus"],
        )
        .add_filter("All Files", &["*"])
        .pick_file(move |file_path| {
            let _ = tx.send(file_path);
        });

    let file_path = tauri::async_runtime::spawn_blocking(move || {
        rx.recv()
            .map_err(|_| "Failed to receive file path".to_string())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    match file_path {
        Some(path) => {
            let path_str = path.to_string();

            // Load the audio file - this automatically extracts all metadata
            let media_id = rastersong::media::load_media(&path_str)
                .map_err(|e| format!("Failed to load audio file: {}", e))?;

            // Get the duration from the loaded media
            let duration = rastersong::media::get_audio_duration(media_id)
                .map_err(|e| format!("Failed to get audio duration: {}", e))?;

            Ok(Some(MediaInfo {
                path: path_str,
                id: media_id.to_string(),
                duration,
            }))
        }
        None => Ok(None),
    }
}

#[tauri::command]
fn remove_media(id: String) -> Result<bool, String> {
    let media_id = rastersong::media::MediaId::from_string(&id)
        .map_err(|e| format!("Invalid media ID: {}", e))?;

    Ok(rastersong::media::remove_media_file(&media_id))
}

#[derive(Serialize)]
struct FrameBoundaries {
    start: f64,
    end: f64,
}

#[tauri::command]
fn get_frame_boundaries(id: String, timestamp: f64) -> Result<FrameBoundaries, String> {
    let media_id = rastersong::media::MediaId::from_string(&id)
        .map_err(|e| format!("Invalid media ID: {}", e))?;

    let boundaries = rastersong::media::get_frame_boundaries(&media_id, timestamp)
        .ok_or_else(|| "Media has no video stream or not found".to_string())?;

    Ok(FrameBoundaries {
        start: boundaries.0,
        end: boundaries.1,
    })
}

/// Get a decoded video frame at a specific timestamp
///
/// Returns a SerializableVideoFrame with base64-encoded RGBA pixel data,
/// ready to display on a canvas in the GUI.
#[tauri::command]
fn get_frame_at_timestamp(
    id: String,
    timestamp: f64,
) -> Result<rastersong::media::SerializableVideoFrame, String> {
    let media_id = rastersong::media::MediaId::from_string(&id)
        .map_err(|e| format!("Invalid media ID: {}", e))?;

    // Get video info to calculate frame boundaries
    let video_info = rastersong::media::get_video_info(&media_id)
        .ok_or_else(|| "Media has no video stream or not found".to_string())?;

    let (_width, _height, fps) = video_info;
    let frame_duration = 1.0 / fps;

    // Calculate the exact frame start time
    let frame_number = (timestamp / frame_duration).floor();
    let frame_start = frame_number * frame_duration;

    // Decode just a small window around the target frame (only 2-3 frames)
    let decode_start = (frame_start).max(0.0);
    let decode_end = frame_start + frame_duration;

    // This will only decode ~2-3 frames instead of many
    let frames = rastersong::media::decode_frames(&media_id, decode_start, decode_end)
        .map_err(|e| format!("Failed to decode frame: {}", e))?;

    // Print number of frames decoded
    println!("Decoded {} frames", frames.len());

    // Find the frame closest to our target timestamp
    let target_frame = frames
        .iter()
        .min_by(|a, b| {
            let a_diff = (a.timestamp() - timestamp).abs();
            let b_diff = (b.timestamp() - timestamp).abs();
            a_diff.partial_cmp(&b_diff).unwrap()
        })
        .ok_or_else(|| format!("No frame found near timestamp {}s", timestamp))?;

    Ok(target_frame.to_serializable())
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize FFmpeg
    rastersong::media::init().expect("Failed to initialize FFmpeg");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            open_video_dialog,
            open_audio_dialog,
            remove_media,
            get_frame_boundaries,
            get_frame_at_timestamp
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
