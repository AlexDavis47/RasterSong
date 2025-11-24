//! Frame metadata cache for efficient video seeking and GOP management

use std::collections::HashMap;

/// Metadata for a single video frame
#[derive(Debug, Clone)]
pub struct FrameMetadata {
    /// Frame number in the stream (0-indexed)
    pub frame_number: usize,
    /// Presentation timestamp in stream time_base units
    pub pts: i64,
    /// Timestamp in seconds
    pub timestamp: f64,
    /// Whether this frame is a keyframe (I-frame)
    pub is_keyframe: bool,
    /// GOP (Group of Pictures) ID this frame belongs to
    pub gop_id: usize,
    /// File offset of the packet (if available)
    pub file_offset: Option<i64>,
}

/// Cache of all frame metadata for efficient seeking
#[derive(Clone)]
pub struct FrameMetadataCache {
    /// All frame metadata, indexed by frame number
    frames: Vec<FrameMetadata>,
    /// Keyframe positions (frame numbers that are keyframes)
    keyframe_indices: Vec<usize>,
    /// Map from GOP ID to the frames in that GOP
    gop_frames: HashMap<usize, Vec<usize>>,
    /// Total number of GOPs in the video
    num_gops: usize,
}

impl FrameMetadataCache {
    /// Create a new empty metadata cache
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            keyframe_indices: Vec::new(),
            gop_frames: HashMap::new(),
            num_gops: 0,
        }
    }

    /// Create a cache with pre-allocated capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            frames: Vec::with_capacity(capacity),
            keyframe_indices: Vec::new(),
            gop_frames: HashMap::new(),
            num_gops: 0,
        }
    }

    /// Add a frame to the cache
    pub fn add_frame(&mut self, metadata: FrameMetadata) {
        if metadata.is_keyframe {
            self.keyframe_indices.push(metadata.frame_number);
            self.num_gops = metadata.gop_id + 1;
        }

        // Track frames in GOP
        self.gop_frames
            .entry(metadata.gop_id)
            .or_insert_with(Vec::new)
            .push(metadata.frame_number);

        self.frames.push(metadata);
    }

    /// Get metadata for a specific frame number
    pub fn get_frame(&self, frame_number: usize) -> Option<&FrameMetadata> {
        self.frames.get(frame_number)
    }

    /// Get the keyframe before or at the given frame number (O(log n))
    pub fn get_keyframe_before(&self, frame_number: usize) -> Option<&FrameMetadata> {
        // Binary search for the keyframe at or before frame_number
        let idx = self
            .keyframe_indices
            .binary_search(&frame_number)
            .unwrap_or_else(|idx| idx.saturating_sub(1));

        if idx < self.keyframe_indices.len() {
            let keyframe_num = self.keyframe_indices[idx];
            if keyframe_num <= frame_number {
                return self.get_frame(keyframe_num);
            }
        }

        None
    }

    /// Get the keyframe before or at the given timestamp (O(log n))
    pub fn get_keyframe_before_timestamp(&self, timestamp: f64) -> Option<&FrameMetadata> {
        // Find frame number closest to timestamp
        let frame_number = self
            .frames
            .iter()
            .position(|f| f.timestamp >= timestamp)
            .unwrap_or(self.frames.len().saturating_sub(1));

        self.get_keyframe_before(frame_number)
    }

    /// Get all frames in a specific GOP
    pub fn get_frames_in_gop(&self, gop_id: usize) -> Option<Vec<&FrameMetadata>> {
        self.gop_frames.get(&gop_id).map(|frame_nums| {
            frame_nums
                .iter()
                .filter_map(|&num| self.get_frame(num))
                .collect()
        })
    }

    /// Get the GOP size (number of frames in GOP)
    pub fn get_gop_size(&self, gop_id: usize) -> usize {
        self.gop_frames
            .get(&gop_id)
            .map(|frames| frames.len())
            .unwrap_or(0)
    }

    /// Get the GOP ID for a frame number
    pub fn get_gop_id(&self, frame_number: usize) -> Option<usize> {
        self.get_frame(frame_number).map(|f| f.gop_id)
    }

    /// Get total number of frames
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Get total number of GOPs
    pub fn gop_count(&self) -> usize {
        self.num_gops
    }

    /// Get all keyframe positions
    pub fn keyframe_positions(&self) -> &[usize] {
        &self.keyframe_indices
    }

    /// Check if a frame is a keyframe
    pub fn is_keyframe(&self, frame_number: usize) -> bool {
        self.get_frame(frame_number)
            .map(|f| f.is_keyframe)
            .unwrap_or(false)
    }

    /// Get frame by timestamp (finds closest frame)
    pub fn get_frame_by_timestamp(&self, timestamp: f64) -> Option<&FrameMetadata> {
        self.frames.iter().min_by(|a, b| {
            let a_diff = (a.timestamp - timestamp).abs();
            let b_diff = (b.timestamp - timestamp).abs();
            a_diff.partial_cmp(&b_diff).unwrap()
        })
    }

    /// Get frames in a time range
    pub fn get_frames_in_range(&self, start_time: f64, end_time: f64) -> Vec<&FrameMetadata> {
        self.frames
            .iter()
            .filter(|f| f.timestamp >= start_time && f.timestamp < end_time)
            .collect()
    }
}

impl Default for FrameMetadataCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_cache() {
        let mut cache = FrameMetadataCache::new();

        // Add some frames (keyframe every 10 frames)
        for i in 0..30 {
            cache.add_frame(FrameMetadata {
                frame_number: i,
                pts: i as i64 * 1000,
                timestamp: i as f64 * 0.033, // ~30fps
                is_keyframe: i % 10 == 0,
                gop_id: i / 10,
                file_offset: Some(i as i64 * 4096),
            });
        }

        assert_eq!(cache.frame_count(), 30);
        assert_eq!(cache.gop_count(), 3);
        assert_eq!(cache.keyframe_positions().len(), 3);

        // Test keyframe lookup
        let keyframe = cache.get_keyframe_before(15).unwrap();
        assert_eq!(keyframe.frame_number, 10);
        assert!(keyframe.is_keyframe);

        // Test GOP size
        assert_eq!(cache.get_gop_size(0), 10);
        assert_eq!(cache.get_gop_size(1), 10);
        assert_eq!(cache.get_gop_size(2), 10);

        // Test GOP frames
        let gop_frames = cache.get_frames_in_gop(1).unwrap();
        assert_eq!(gop_frames.len(), 10);
        assert_eq!(gop_frames[0].frame_number, 10);
    }
}
