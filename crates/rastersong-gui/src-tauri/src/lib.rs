// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use std::sync::mpsc;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn open_video_dialog(window: tauri::Window) -> Result<Option<String>, String> {
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
        Some(path) => Ok(Some(path.to_string())),
        None => Ok(None),
    }
}

#[tauri::command]
async fn open_audio_dialog(window: tauri::Window) -> Result<Option<String>, String> {
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
        Some(path) => Ok(Some(path.to_string())),
        None => Ok(None),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            open_video_dialog,
            open_audio_dialog
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
