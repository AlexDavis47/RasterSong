//! MediaStore - registry for managing threaded media files
//!
//! This module provides a central store for loaded media files.
//! Each file runs in its own worker thread for non-blocking decode operations.

use super::audio_decoder::AudioDecoder;
use super::ffmpeg;
use super::frame_cache::FrameCache;
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

/// Result of async media loading containing all data needed to store the media
pub struct LoadedMediaData {
    pub id: MediaId,
    pub media_file: MediaFile,
    pub worker_handle: JoinHandle<()>,
}

/// Receiver for async media loading operations
///
/// This wraps the internal channel receiver to avoid exposing crossbeam types.
/// Use `try_receive()` to check if loading is complete without blocking.
pub struct LoadMediaReceiver(crossbeam::channel::Receiver<Result<LoadedMediaData>>);

impl LoadMediaReceiver {
    /// Try to receive the loaded media data from a load request (non-blocking)
    ///
    /// # Returns
    /// - `Ok(Some(data))` if loading is complete, containing MediaId, MediaFile, and worker handle
    /// - `Ok(None)` if loading is still in progress
    /// - `Err(e)` if there was an error or channel disconnected
    pub fn try_receive(&self) -> Result<Option<LoadedMediaData>> {
        match self.0.try_recv() {
            Ok(result) => {
                println!("[LoadMediaReceiver] Media loaded from worker");
                result.map(Some)
            }
            Err(crossbeam::channel::TryRecvError::Empty) => {
                // Loading not complete yet
                Ok(None)
            }
            Err(crossbeam::channel::TryRecvError::Disconnected) => {
                anyhow::bail!("Loading thread disconnected")
            }
        }
    }
}

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

    /// Load a media file asynchronously and return a receiver
    ///
    /// This spawns a background thread that opens the file, creates decoders,
    /// scans metadata, spawns a worker thread, and returns loaded media data when complete.
    /// The GUI can poll the receiver to check loading status without blocking.
    ///
    /// # Returns
    /// A `LoadMediaReceiver` that can be polled with `try_receive()` to check
    /// if loading is complete. Returns `Ok(None)` while loading, `Ok(Some(data))`
    /// when complete (containing MediaId, MediaFile, and worker handle), or `Err(e)` on error.
    ///
    /// After receiving the data, call `store_loaded_media()` to store it in the MediaStore.
    pub fn load_media_async(&mut self, path: &Path) -> Result<LoadMediaReceiver> {
        println!(
            "[MediaStore] Starting async load for media file: {:?}",
            path
        );

        // Validate path exists (quick check before spawning thread)
        if !path.exists() {
            anyhow::bail!("File does not exist: {:?}", path);
        }

        // Create channel for loading result
        let (result_tx, result_rx) = channel::bounded(1);
        let path_buf = path.to_path_buf();

        // Spawn background thread to do the heavy loading work
        std::thread::spawn(move || {
            let result = Self::load_media_internal(&path_buf);
            let _ = result_tx.send(result);
        });

        Ok(LoadMediaReceiver(result_rx))
    }

    /// Internal method that performs the actual media loading work
    ///
    /// This runs in a background thread to avoid blocking the GUI.
    fn load_media_internal(path: &Path) -> Result<LoadedMediaData> {
        println!("[MediaStore] Loading media file: {:?}", path);

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

        println!(
            "[MediaStore] Media file loaded successfully with ID: {}",
            id
        );

        Ok(LoadedMediaData {
            id,
            media_file,
            worker_handle,
        })
    }

    /// Store a loaded media file (called after async loading completes)
    ///
    /// This is called from the main thread after receiving the LoadedMediaData from
    /// the async load operation.
    pub fn store_loaded_media(&mut self, data: LoadedMediaData) {
        self.workers.insert(data.id, data.worker_handle);
        self.files.insert(data.id, data.media_file);
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
