//! MediaStore - registry for managing threaded media files
//!
//! This module provides a central store for loaded media files.
//! Each file runs in its own worker thread for non-blocking decode operations.

use super::audio_decoder::AudioDecoder;
use super::ffmpeg;
use super::frame_cache::FrameCache;
use super::frame_metadata::FrameMetadataCache;
use super::media_file::{MediaFile, MediaMetadata};
use super::media_id::MediaId;
use super::media_worker::{MediaWorker, spawn_worker};
use super::video_decoder::VideoDecoder;
use anyhow::{Context, Result};
use crossbeam::channel;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::thread::JoinHandle;

/// MediaStore implementation with threaded media files
pub struct MediaStore {
    /// Map of MediaId to MediaFile
    files: HashMap<MediaId, MediaFile>,
    /// Map of MediaId to worker thread handles
    workers: HashMap<MediaId, JoinHandle<()>>,
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
            workers: HashMap::new(),
        }
    }

    /// Load a media file and return its MediaId
    ///
    /// This opens the file, creates decoders, spawns a worker thread,
    /// and returns a MediaFile that communicates with the worker.
    pub fn load_media(&mut self, path: &Path) -> Result<MediaId> {
        println!("[MediaStore] Loading media file: {:?}", path);

        // Validate path exists
        if !path.exists() {
            anyhow::bail!("File does not exist: {:?}", path);
        }

        // Ensure FFmpeg is initialized
        println!("[MediaStore] Initializing FFmpeg");
        ffmpeg::init()?;

        // Create new MediaId
        let id = MediaId::new();
        println!("[MediaStore] Created MediaId: {}", id);

        // Open the format context
        println!("[MediaStore] Opening format context");
        let mut format_context = ffmpeg_next::format::input(path)
            .with_context(|| format!("Failed to open media file: {:?}", path))?;

        // Try to create video decoder and scan metadata
        println!("[MediaStore] Creating video decoder");
        let mut video_decoder = VideoDecoder::new(&format_context).ok();
        let metadata_cache = if let Some(ref mut decoder) = video_decoder {
            println!("[MediaStore] Scanning video metadata (this may take a moment)...");
            // Scan entire video to build metadata cache for efficient seeking
            decoder
                .scan_metadata(&mut format_context)
                .with_context(|| format!("Failed to scan video metadata: {:?}", path))?;
            println!("[MediaStore] Video metadata scan complete");
            // Clone metadata cache before moving decoder to worker
            // MediaFile owns its own copy (read-only, only used by main thread)
            Some(decoder.metadata_cache().clone())
        } else {
            None
        };

        // Try to create audio decoder
        println!("[MediaStore] Creating audio decoder");
        let audio_decoder = AudioDecoder::new(&format_context).ok();

        // At least one decoder should be present
        if video_decoder.is_none() && audio_decoder.is_none() {
            anyhow::bail!("No video or audio streams found in file: {:?}", path);
        }

        // Extract metadata
        let duration = if let Some(ref decoder) = video_decoder {
            decoder.duration()
        } else if let Some(ref decoder) = audio_decoder {
            decoder.duration()
        } else {
            0.0
        };

        let video_info = video_decoder
            .as_ref()
            .map(|decoder| (decoder.width(), decoder.height(), decoder.fps()));

        let audio_info = audio_decoder
            .as_ref()
            .map(|decoder| (decoder.sample_rate(), decoder.channels()));

        let metadata = MediaMetadata {
            duration,
            video_info,
            audio_info,
        };

        // Create shared frame cache
        println!("[MediaStore] Creating shared frame cache");
        let frame_cache = Arc::new(FrameCache::default());

        // Create request/response channels
        println!("[MediaStore] Creating request/response channels");
        let (request_tx, request_rx) = channel::unbounded();

        // Create worker
        println!("[MediaStore] Creating MediaWorker");
        let worker = MediaWorker::new(
            id,
            path.to_path_buf(),
            format_context,
            video_decoder,
            audio_decoder,
            frame_cache.clone(),
            request_rx,
        );

        // Spawn worker thread
        println!("[MediaStore] Spawning worker thread");
        let worker_handle = spawn_worker(worker);

        // Create MediaFile
        println!("[MediaStore] Creating MediaFile");
        let media_file = MediaFile::new(
            id,
            path.to_path_buf(),
            request_tx,
            frame_cache,
            metadata_cache,
            metadata,
        );

        // Store media file and worker handle
        println!("[MediaStore] Storing MediaFile and worker handle");
        self.workers.insert(id, worker_handle);
        self.files.insert(id, media_file);

        println!(
            "[MediaStore] Media file loaded successfully with ID: {}",
            id
        );
        Ok(id)
    }

    /// Get a reference to a MediaFile by its MediaId
    pub fn get_media(&self, id: &MediaId) -> Option<&MediaFile> {
        self.files.get(id)
    }

    /// List all MediaIds currently in the store
    pub fn list_media(&self) -> Vec<MediaId> {
        self.files.keys().copied().collect()
    }

    /// Remove a media file from the store and shutdown its worker
    pub fn remove_media(&mut self, id: &MediaId) -> bool {
        // Send shutdown request to worker
        if let Some(media_file) = self.files.get(id) {
            media_file.shutdown();
        }

        // Remove from files
        let removed = self.files.remove(id).is_some();

        // Wait for worker thread to finish
        if let Some(handle) = self.workers.remove(id) {
            let _ = handle.join();
        }

        removed
    }

    /// Shutdown all workers and clean up
    ///
    /// This should be called before dropping MediaStore to ensure
    /// all worker threads are properly shut down.
    pub fn shutdown(&mut self) {
        // Send shutdown to all workers
        for media_file in self.files.values() {
            media_file.shutdown();
        }

        // Clear files
        self.files.clear();

        // Wait for all workers to finish
        for (_, handle) in self.workers.drain() {
            let _ = handle.join();
        }
    }
}

impl Drop for MediaStore {
    fn drop(&mut self) {
        // Ensure all workers are shut down
        self.shutdown();
    }
}
