//! Media file identifier system
//!
//! Manages a registry of video and audio files with unique identifiers.

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Unique identifier for a media file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaId(Uuid);

impl MediaId {
    fn new() -> Self {
        MediaId(Uuid::new_v4())
    }
}

/// Information about a registered media file
#[derive(Debug, Clone)]
pub struct MediaFileInfo {
    pub id: MediaId,
    pub path: PathBuf,
    pub media_type: MediaType,
    pub offset_seconds: f64,
}

/// Type of media file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Video,
    Audio,
}

/// Internal store for registered media files
struct MediaStore {
    files: HashMap<MediaId, MediaFileInfo>,
}

impl MediaStore {
    fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    fn register(&mut self, path: PathBuf, media_type: MediaType) -> MediaId {
        let id = MediaId::new();
        let info = MediaFileInfo {
            id,
            path,
            media_type,
            offset_seconds: 0.0,
        };
        self.files.insert(id, info);
        id
    }

    fn set_offset(&mut self, id: &MediaId, offset_seconds: f64) {
        if let Some(info) = self.files.get_mut(id) {
            info.offset_seconds = offset_seconds;
        }
    }

    fn get(&self, id: &MediaId) -> Option<&MediaFileInfo> {
        self.files.get(id)
    }

    fn list(&self) -> Vec<MediaId> {
        self.files.keys().copied().collect()
    }
}

// Global media store (thread-safe via Mutex)
use std::sync::{Mutex, OnceLock};

fn get_store() -> &'static Mutex<MediaStore> {
    static STORE: OnceLock<Mutex<MediaStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(MediaStore::new()))
}

/// Register a video file and return its identifier
pub fn register_video_file<P: AsRef<Path>>(path: P) -> Result<MediaId> {
    let path = path.as_ref().to_path_buf();
    if !path.exists() {
        anyhow::bail!("File does not exist: {:?}", path);
    }
    let mut store = get_store().lock().unwrap();
    Ok(store.register(path, MediaType::Video))
}

/// Register an audio file and return its identifier
pub fn register_audio_file<P: AsRef<Path>>(path: P) -> Result<MediaId> {
    let path = path.as_ref().to_path_buf();
    if !path.exists() {
        anyhow::bail!("File does not exist: {:?}", path);
    }
    let mut store = get_store().lock().unwrap();
    Ok(store.register(path, MediaType::Audio))
}

/// Get information about a registered media file
pub fn get_file_info(id: &MediaId) -> Option<MediaFileInfo> {
    let store = get_store().lock().unwrap();
    store.get(id).cloned()
}

/// List all registered media file identifiers
pub fn list_media_files() -> Vec<MediaId> {
    let store = get_store().lock().unwrap();
    store.list()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_register_video_file() {
        let video_path = Path::new("../../test_assets/test.mp4");
        if !video_path.exists() {
            eprintln!("Test video not found, skipping test");
            return;
        }

        let id = register_video_file(video_path).unwrap();
        assert!(get_file_info(&id).is_some());

        let info = get_file_info(&id).unwrap();
        assert_eq!(info.media_type, MediaType::Video);
        assert_eq!(info.path, video_path);
    }

    #[test]
    fn test_register_audio_file() {
        let audio_path = Path::new("../../test_assets/test_modulator.wav");
        if !audio_path.exists() {
            eprintln!("Test audio not found, skipping test");
            return;
        }

        let id = register_audio_file(audio_path).unwrap();
        assert!(get_file_info(&id).is_some());

        let info = get_file_info(&id).unwrap();
        assert_eq!(info.media_type, MediaType::Audio);
        assert_eq!(info.path, audio_path);
    }

    #[test]
    fn test_list_media_files() {
        let video_path = Path::new("../../test_assets/test.mp4");
        let audio_path = Path::new("../../test_assets/test_modulator.wav");

        if !video_path.exists() || !audio_path.exists() {
            eprintln!("Test files not found, skipping test");
            return;
        }

        let video_id = register_video_file(video_path).unwrap();
        let audio_id = register_audio_file(audio_path).unwrap();

        let list = list_media_files();
        println!("Media files: {:?}", list);
        assert!(list.contains(&video_id));
        assert!(list.contains(&audio_id));
    }

    #[test]
    fn test_register_nonexistent_file() {
        let result = register_video_file("nonexistent.mp4");
        assert!(result.is_err());
    }
}
