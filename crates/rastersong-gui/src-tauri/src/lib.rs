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

            // Register the video file first (without duration)
            let media_id = rastersong::media::register_video_file(&path_str, None)
                .map_err(|e| format!("Failed to register video file: {}", e))?;

            // Get the duration
            let duration = rastersong::media::get_video_duration(media_id)
                .map_err(|e| format!("Failed to get video duration: {}", e))?;

            // Update the registration with the duration by getting file info
            let file_info = rastersong::media::get_file_info(&media_id)
                .ok_or_else(|| "Failed to get file info".to_string())?;

            // Re-register with duration
            let media_id = rastersong::media::register_video_file(&file_info.path, Some(duration))
                .map_err(|e| format!("Failed to re-register video file: {}", e))?;

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

            // Register the audio file first (without duration)
            let media_id = rastersong::media::register_audio_file(&path_str, None)
                .map_err(|e| format!("Failed to register audio file: {}", e))?;

            // Get the duration
            let duration = rastersong::media::get_audio_duration(media_id)
                .map_err(|e| format!("Failed to get audio duration: {}", e))?;

            // Update the registration with the duration by getting file info
            let file_info = rastersong::media::get_file_info(&media_id)
                .ok_or_else(|| "Failed to get file info".to_string())?;

            // Re-register with duration
            let media_id = rastersong::media::register_audio_file(&file_info.path, Some(duration))
                .map_err(|e| format!("Failed to re-register audio file: {}", e))?;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize GStreamer
    rastersong::media::init().expect("Failed to initialize GStreamer");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            open_video_dialog,
            open_audio_dialog,
            remove_media
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
