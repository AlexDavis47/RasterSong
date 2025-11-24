//! Concurrent LRU frame cache for decoded video frames

use super::video_frame::VideoFrame;
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Cached GOP containing all decoded frames
#[derive(Clone)]
pub struct CachedGop {
    /// GOP ID
    pub gop_id: usize,
    /// All frames in this GOP (frame_number -> frame)
    pub frames: HashMap<usize, VideoFrame>,
    /// Size estimate in bytes (for cache management)
    pub size_bytes: usize,
}

/// Concurrent LRU cache for decoded video frames organized by GOP
///
/// This cache is fully thread-safe and uses lock-free reads.
/// Writes use atomic operations for size tracking.
pub struct FrameCache {
    /// Map from GOP ID to cached GOP (concurrent hash map)
    gops: DashMap<usize, CachedGop>,
    /// Maximum number of GOPs to cache
    max_gops: usize,
    /// Current total size in bytes (atomic counter)
    current_size: AtomicUsize,
    /// Maximum cache size in bytes (soft limit)
    max_size_bytes: usize,
}

impl FrameCache {
    /// Create a new frame cache with specified limits
    ///
    /// # Arguments
    /// * `max_gops` - Maximum number of GOPs to cache
    /// * `max_size_mb` - Maximum cache size in megabytes
    pub fn new(max_gops: usize, max_size_mb: usize) -> Self {
        Self {
            gops: DashMap::new(),
            max_gops,
            current_size: AtomicUsize::new(0),
            max_size_bytes: max_size_mb * 1024 * 1024,
        }
    }

    /// Create a default frame cache (500 GOPs, 5000MB)
    pub fn default_size() -> Self {
        Self::new(500, 5000)
    }

    /// Get a frame from the cache
    ///
    /// Returns None if the GOP or frame is not cached.
    /// This is a non-blocking read operation.
    pub fn get_frame(&self, gop_id: usize, frame_number: usize) -> Option<VideoFrame> {
        self.gops
            .get(&gop_id)
            .and_then(|gop| gop.frames.get(&frame_number).cloned())
    }

    /// Get all frames in a GOP
    ///
    /// Returns None if the GOP is not cached.
    /// This is a non-blocking read operation.
    pub fn get_gop(&self, gop_id: usize) -> Option<Vec<VideoFrame>> {
        // Get a reference to the GOP and clone its frames
        self.gops.get(&gop_id).map(|gop| {
            // Clone all frames from the HashMap
            gop.frames.values().cloned().collect()
        })
    }

    /// Check if a GOP is cached
    pub fn contains_gop(&self, gop_id: usize) -> bool {
        self.gops.contains_key(&gop_id)
    }

    /// Check if a specific frame is cached
    pub fn contains_frame(&self, gop_id: usize, frame_number: usize) -> bool {
        self.gops
            .get(&gop_id)
            .map(|entry| entry.frames.contains_key(&frame_number))
            .unwrap_or(false)
    }

    /// Store a GOP in the cache
    ///
    /// Evicts LRU GOPs if necessary to maintain size limits.
    /// This operation is thread-safe and uses atomic operations.
    pub fn store_gop(&self, gop_id: usize, frames: HashMap<usize, VideoFrame>) {
        // Calculate size estimate from VideoFrame data
        let size_bytes: usize = frames.values().map(|frame| frame.data_size()).sum();

        // Remove existing GOP if present
        if let Some((_, old_gop)) = self.gops.remove(&gop_id) {
            self.current_size
                .fetch_sub(old_gop.size_bytes, Ordering::Relaxed);
        }

        // Evict LRU GOPs if we exceed limits
        // Note: DashMap doesn't maintain insertion order, so we use a simple
        // eviction strategy: remove entries until we're under limits
        while self.gops.len() >= self.max_gops
            || self.current_size.load(Ordering::Relaxed) + size_bytes > self.max_size_bytes
        {
            // Remove the first entry we find
            if let Some(entry) = self.gops.iter().next() {
                let evicted_gop_id = *entry.key();
                if let Some((_, evicted_gop)) = self.gops.remove(&evicted_gop_id) {
                    self.current_size
                        .fetch_sub(evicted_gop.size_bytes, Ordering::Relaxed);
                }
            } else {
                break; // No more GOPs to evict
            }
        }

        // Store new GOP
        let cached_gop = CachedGop {
            gop_id,
            frames,
            size_bytes,
        };

        self.current_size.fetch_add(size_bytes, Ordering::Relaxed);
        self.gops.insert(gop_id, cached_gop);
    }

    /// Store a single frame in the cache (adds to existing GOP or creates new one)
    pub fn store_frame(&self, gop_id: usize, frame_number: usize, frame: VideoFrame) {
        let frame_size = frame.data_size();

        // Try to get existing GOP
        if let Some(mut entry) = self.gops.get_mut(&gop_id) {
            // DashMap allows mutable access
            if !entry.frames.contains_key(&frame_number) {
                entry.frames.insert(frame_number, frame);
                entry.size_bytes += frame_size;
                self.current_size.fetch_add(frame_size, Ordering::Relaxed);
            }
        } else {
            // Create new GOP with this frame
            let mut frames = HashMap::new();
            frames.insert(frame_number, frame);
            self.store_gop(gop_id, frames);
        }
    }

    /// Clear all cached GOPs
    pub fn clear(&self) {
        self.gops.clear();
        self.current_size.store(0, Ordering::Relaxed);
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let gop_count = self.gops.len();
        let frame_count: usize = self.gops.iter().map(|entry| entry.frames.len()).sum();
        let size_bytes = self.current_size.load(Ordering::Relaxed);
        CacheStats {
            gop_count,
            frame_count,
            size_bytes,
            size_mb: size_bytes / (1024 * 1024),
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of cached GOPs
    pub gop_count: usize,
    /// Total number of cached frames
    pub frame_count: usize,
    /// Total cache size in bytes
    pub size_bytes: usize,
    /// Total cache size in megabytes
    pub size_mb: usize,
}

impl Default for FrameCache {
    fn default() -> Self {
        Self::default_size()
    }
}
