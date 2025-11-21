//! MediaStore - registry for managing media files
//!
//! This module provides a central store for loaded media files.
//! Each file is identified by a unique MediaId.

use super::media_file::MediaFile;
use super::media_id::MediaId;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

/// MediaStore implementation
pub struct MediaStore {
    /// Map of MediaId to MediaFile
    files: HashMap<MediaId, MediaFile>,
}

impl Default for MediaStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaStore {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    /// Load a media file and return its MediaId
    pub fn load_media(&mut self, path: &Path) -> Result<MediaId> {
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
    pub fn get_media(&self, id: &MediaId) -> Option<&MediaFile> {
        self.files.get(id)
    }

    /// Get a mutable reference to a MediaFile by its MediaId
    pub fn get_media_mut(&mut self, id: &MediaId) -> Option<&mut MediaFile> {
        self.files.get_mut(id)
    }

    /// List all MediaIds currently in the store
    pub fn list_media(&self) -> Vec<MediaId> {
        self.files.keys().copied().collect()
    }

    /// Remove a media file from the store
    pub fn remove_media(&mut self, id: &MediaId) -> bool {
        self.files.remove(id).is_some()
    }
}
