//! LRU frame cache for decoded video frames

use super::video_frame::VideoFrame;
use std::collections::{HashMap, VecDeque};

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

/// LRU cache for decoded video frames organized by GOP
pub struct FrameCache {
    /// Map from GOP ID to cached GOP
    gops: HashMap<usize, CachedGop>,
    /// LRU queue of GOP IDs (most recently used at back)
    lru_queue: VecDeque<usize>,
    /// Maximum number of GOPs to cache
    max_gops: usize,
    /// Current total size in bytes
    current_size: usize,
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
            gops: HashMap::new(),
            lru_queue: VecDeque::new(),
            max_gops,
            current_size: 0,
            max_size_bytes: max_size_mb * 1024 * 1024,
        }
    }

    /// Create a default frame cache (100 GOPs, 500MB)
    pub fn default_size() -> Self {
        Self::new(100, 500)
    }

    /// Get a frame from the cache
    ///
    /// Returns None if the GOP or frame is not cached.
    /// Updates LRU on access.
    pub fn get_frame(&mut self, gop_id: usize, frame_number: usize) -> Option<VideoFrame> {
        if !self.gops.contains_key(&gop_id) {
            return None;
        }

        // Update LRU
        self.touch_gop(gop_id);

        // Get frame and clone it
        self.gops.get(&gop_id)?.frames.get(&frame_number).cloned()
    }

    /// Get all frames in a GOP
    ///
    /// Returns None if the GOP is not cached.
    /// Updates LRU on access.
    pub fn get_gop(&mut self, gop_id: usize) -> Option<Vec<VideoFrame>> {
        if !self.gops.contains_key(&gop_id) {
            return None;
        }

        // Update LRU
        self.touch_gop(gop_id);

        // Get GOP frames and clone them
        self.gops
            .get(&gop_id)
            .map(|gop| gop.frames.values().cloned().collect())
    }

    /// Check if a GOP is cached
    pub fn contains_gop(&self, gop_id: usize) -> bool {
        self.gops.contains_key(&gop_id)
    }

    /// Check if a specific frame is cached
    pub fn contains_frame(&self, gop_id: usize, frame_number: usize) -> bool {
        self.gops
            .get(&gop_id)
            .map(|gop| gop.frames.contains_key(&frame_number))
            .unwrap_or(false)
    }

    /// Store a GOP in the cache
    ///
    /// Evicts LRU GOPs if necessary to maintain size limits.
    pub fn store_gop(&mut self, gop_id: usize, frames: HashMap<usize, VideoFrame>) {
        // Calculate size estimate from VideoFrame data
        let size_bytes: usize = frames.values().map(|frame| frame.data_size()).sum();

        // Remove existing GOP if present
        if let Some(old_gop) = self.gops.remove(&gop_id) {
            self.current_size = self.current_size.saturating_sub(old_gop.size_bytes);
            self.lru_queue.retain(|&id| id != gop_id);
        }

        // Evict LRU GOPs if we exceed limits
        while self.gops.len() >= self.max_gops
            || self.current_size + size_bytes > self.max_size_bytes
        {
            if let Some(lru_gop_id) = self.lru_queue.pop_front() {
                if let Some(evicted_gop) = self.gops.remove(&lru_gop_id) {
                    self.current_size = self.current_size.saturating_sub(evicted_gop.size_bytes);
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

        self.current_size += size_bytes;
        self.gops.insert(gop_id, cached_gop);
        self.lru_queue.push_back(gop_id);
    }

    /// Store a single frame in the cache (adds to existing GOP or creates new one)
    pub fn store_frame(&mut self, gop_id: usize, frame_number: usize, frame: VideoFrame) {
        let frame_size = frame.data_size();

        if let Some(gop) = self.gops.get_mut(&gop_id) {
            // Add to existing GOP
            if !gop.frames.contains_key(&frame_number) {
                gop.frames.insert(frame_number, frame);
                gop.size_bytes += frame_size;
                self.current_size += frame_size;
            }
            self.touch_gop(gop_id);
        } else {
            // Create new GOP with this frame
            let mut frames = HashMap::new();
            frames.insert(frame_number, frame);
            self.store_gop(gop_id, frames);
        }
    }

    /// Update LRU for a GOP
    fn touch_gop(&mut self, gop_id: usize) {
        // Remove from current position
        self.lru_queue.retain(|&id| id != gop_id);
        // Add to back (most recently used)
        self.lru_queue.push_back(gop_id);
    }

    /// Clear all cached GOPs
    pub fn clear(&mut self) {
        self.gops.clear();
        self.lru_queue.clear();
        self.current_size = 0;
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            gop_count: self.gops.len(),
            frame_count: self.gops.values().map(|gop| gop.frames.len()).sum(),
            size_bytes: self.current_size,
            size_mb: self.current_size / (1024 * 1024),
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
