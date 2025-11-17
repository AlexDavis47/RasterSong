//! MediaStore - singleton registry for managing media files

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use super::media_file::MediaFile;
use super::media_id::MediaId;

/// Internal MediaStore implementation
struct MediaStore {
    /// Map of MediaId to MediaFile
    files: HashMap<MediaId, MediaFile>,
}

impl MediaStore {
    fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    /// Load a media file and return its MediaId
    fn load_media(&mut self, path: &Path) -> Result<MediaId> {
        // Validate path exists
        if !path.exists() {
            anyhow::bail!("File does not exist: {:?}", path);
        }

        // Create new MediaId
        let id = MediaId::new();

        // Create MediaFile
        let media_file = MediaFile::open(path.to_path_buf(), id)
            .with_context(|| format!("Failed to load media file: {:?}", path))?;

        // Store in HashMap
        self.files.insert(id, media_file);

        Ok(id)
    }

    /// Get a reference to a MediaFile by its MediaId
    fn get_media(&self, id: &MediaId) -> Option<&MediaFile> {
        self.files.get(id)
    }

    /// Get a mutable reference to a MediaFile by its MediaId
    fn get_media_mut(&mut self, id: &MediaId) -> Option<&mut MediaFile> {
        self.files.get_mut(id)
    }

    /// List all MediaIds currently in the store
    fn list_media(&self) -> Vec<MediaId> {
        self.files.keys().copied().collect()
    }

    /// Remove a media file from the store
    fn remove_media(&mut self, id: &MediaId) -> bool {
        self.files.remove(id).is_some()
    }
}

/// Get the global MediaStore instance
fn get_store() -> &'static Mutex<MediaStore> {
    static STORE: OnceLock<Mutex<MediaStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(MediaStore::new()))
}

// Public API functions

/// Load a media file into the MediaStore
///
/// # Arguments
/// * `path` - Path to the media file
///
/// # Returns
/// MediaId that can be used to reference this file
pub fn load_media<P: AsRef<Path>>(path: P) -> Result<MediaId> {
    let mut store = get_store().lock().unwrap();
    store.load_media(path.as_ref())
}

/// Get a reference to a MediaFile by its MediaId
///
/// Note: This returns None while the lock is held. For actual operations,
/// use the provided helper functions instead.
pub fn get_media(_id: &MediaId) -> Option<MediaFile> {
    // Note: Can't return a reference due to mutex lock lifetime
    // This is a limitation - we'll need to work around it with helper functions
    let _store = get_store().lock().unwrap();
    // For now, we can't return a reference that outlives the lock
    // Will need to refactor the API
    None
}

/// List all MediaIds currently in the store
pub fn list_media() -> Vec<MediaId> {
    let store = get_store().lock().unwrap();
    store.list_media()
}

/// Remove a media file from the store
pub fn remove_media(id: &MediaId) -> bool {
    let mut store = get_store().lock().unwrap();
    store.remove_media(id)
}

/// Decode frames from a media file
///
/// # Arguments
/// * `id` - MediaId of the file
/// * `start_time` - Start time in seconds
/// * `end_time` - End time in seconds
pub fn decode_frames(
    id: &MediaId,
    start_time: f64,
    end_time: f64,
) -> Result<Vec<ffmpeg_next::frame::Video>> {
    let mut store = get_store().lock().unwrap();
    let media_file = store
        .get_media_mut(id)
        .context("MediaId not found in store")?;
    media_file.decode_frames(start_time, end_time)
}

/// Decode audio samples from a media file
///
/// # Arguments
/// * `id` - MediaId of the file
/// * `start_time` - Start time in seconds
/// * `end_time` - End time in seconds
pub fn decode_samples(
    id: &MediaId,
    start_time: f64,
    end_time: f64,
) -> Result<ffmpeg_next::frame::Audio> {
    let mut store = get_store().lock().unwrap();
    let media_file = store
        .get_media_mut(id)
        .context("MediaId not found in store")?;
    media_file.decode_samples(start_time, end_time)
}

/// Get metadata for a media file
///
/// Returns (has_video, has_audio, duration)
pub fn get_media_info(id: &MediaId) -> Option<(bool, bool, f64)> {
    let store = get_store().lock().unwrap();
    store.get_media(id).map(|media_file| {
        (
            media_file.has_video(),
            media_file.has_audio(),
            media_file.duration(),
        )
    })
}

/// Get video metadata for a media file
///
/// Returns (width, height, fps) if video stream exists
pub fn get_video_info(id: &MediaId) -> Option<(u32, u32, f64)> {
    let store = get_store().lock().unwrap();
    store
        .get_media(id)
        .and_then(|media_file| media_file.video_info())
}

/// Get audio metadata for a media file
///
/// Returns (sample_rate, channels) if audio stream exists
pub fn get_audio_info(id: &MediaId) -> Option<(u32, u16)> {
    let store = get_store().lock().unwrap();
    store
        .get_media(id)
        .and_then(|media_file| media_file.audio_info())
}

// ============================================
// Backward Compatibility Functions
// ============================================

/// Register a video file (alias for load_media)
pub fn register_video_file<P: AsRef<Path>>(path: P, _duration: Option<f64>) -> Result<MediaId> {
    load_media(path)
}

/// Register an audio file (alias for load_media - works for both video and audio)
pub fn register_audio_file<P: AsRef<Path>>(path: P, _duration: Option<f64>) -> Result<MediaId> {
    load_media(path)
}

/// Get video duration in seconds
pub fn get_video_duration(id: MediaId) -> Result<f64> {
    get_media_info(&id)
        .map(|(_, _, duration)| duration)
        .context("MediaId not found")
}

/// Get audio duration in seconds
pub fn get_audio_duration(id: MediaId) -> Result<f64> {
    get_media_info(&id)
        .map(|(_, _, duration)| duration)
        .context("MediaId not found")
}

/// MediaFileInfo for backward compatibility
#[derive(Debug, Clone)]
pub struct MediaFileInfo {
    pub id: MediaId,
    pub path: String,
}

/// Get file info (simplified version for backward compatibility)
pub fn get_file_info(id: &MediaId) -> Option<MediaFileInfo> {
    let store = get_store().lock().unwrap();
    store.get_media(id).map(|media_file| MediaFileInfo {
        id: *id,
        path: media_file.path().to_string_lossy().to_string(),
    })
}

/// Remove a media file (alias for remove_media)
pub fn remove_media_file(id: &MediaId) -> bool {
    remove_media(id)
}
