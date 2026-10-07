//! Rendered frames, keyed so that nothing rendered for an old graph or scale is ever served.

use std::collections::BTreeMap;
use std::sync::Arc;

use rastersong_graph::{NodeCost, NodeMeters, OutputLevel, ParamLevel};

use crate::{AudioBlock, PreviewScale};

/// Identifies what a frame was rendered with. Any graph edit bumps `version`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub version: u64,
    pub scale: PreviewScale,
}

/// A rendered frame as packed RGB8.
#[derive(Clone, PartialEq)]
pub struct Frame {
    pub index: usize,
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    /// The level of every node output while rendering this frame.
    pub levels: Arc<[OutputLevel]>,
    /// The value of every modulated parameter while rendering this frame.
    pub params: Arc<[ParamLevel]>,
    /// How long every node took to process this frame.
    pub costs: Arc<[NodeCost]>,
    /// The meter values of the nodes that publish them.
    pub meters: Arc<[NodeMeters]>,
    /// The frame's rendered sound, when the graph has an audio output.
    pub audio: Option<Arc<AudioBlock>>,
}

impl Frame {
    /// Memory the frame's picture and sound take.
    fn bytes(&self) -> usize {
        self.rgb.len() + self.audio.as_ref().map_or(0, |a| a.samples.len() * 4)
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Frame({} at {}x{})", self.index, self.width, self.height)
    }
}

/// Frames for the current [`CacheKey`] within a memory budget.
///
/// Frames for any other key are dropped the moment the key changes, and inserts for a stale key
/// are refused, so a frame rendered before an edit can never be served after it. When over
/// budget, the frame farthest from the playhead goes first; frames behind the playhead count as
/// twice as far, since playback moves forward.
#[derive(Debug)]
pub struct FrameCache {
    key: CacheKey,
    frames: BTreeMap<usize, Arc<Frame>>,
    bytes: usize,
    budget: usize,
}

impl FrameCache {
    pub fn new(key: CacheKey, budget_bytes: usize) -> Self {
        Self {
            key,
            frames: BTreeMap::new(),
            bytes: 0,
            budget: budget_bytes,
        }
    }

    /// Changes the memory budget, dropping the frames farthest from `playhead` if it shrank.
    pub fn set_budget(&mut self, budget_bytes: usize, playhead: usize) {
        self.budget = budget_bytes;
        self.evict(playhead);
    }

    pub fn key(&self) -> CacheKey {
        self.key
    }

    /// Switches to `key`. Every frame rendered for a different key is dropped.
    pub fn set_key(&mut self, key: CacheKey) {
        if key != self.key {
            self.key = key;
            self.frames.clear();
            self.bytes = 0;
        }
    }

    pub fn get(&self, index: usize) -> Option<Arc<Frame>> {
        self.frames.get(&index).cloned()
    }

    pub fn contains(&self, index: usize) -> bool {
        self.frames.contains_key(&index)
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The cached frames as ranges of consecutive indices.
    pub fn ranges(&self) -> Vec<std::ops::Range<usize>> {
        let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
        for &index in self.frames.keys() {
            match ranges.last_mut() {
                Some(last) if last.end == index => last.end += 1,
                _ => ranges.push(index..index + 1),
            }
        }
        ranges
    }

    /// How many consecutive frames are cached starting at `index`.
    pub fn run_from(&self, index: usize) -> usize {
        self.frames
            .range(index..)
            .zip(index..)
            .take_while(|&((&cached, _), expected)| cached == expected)
            .count()
    }

    /// Stores a frame rendered for `key`. Returns false (and drops the frame) if `key` is stale.
    pub fn insert(&mut self, key: CacheKey, frame: Frame, playhead: usize) -> bool {
        if key != self.key {
            return false;
        }
        self.bytes += frame.bytes();
        if let Some(old) = self.frames.insert(frame.index, Arc::new(frame)) {
            self.bytes -= old.bytes();
        }
        self.evict(playhead);
        true
    }

    fn evict(&mut self, playhead: usize) {
        let distance = |index: usize| {
            if index < playhead {
                (playhead - index) * 2
            } else {
                index - playhead
            }
        };
        while self.bytes > self.budget && self.frames.len() > 1 {
            let first = *self.frames.keys().next().unwrap();
            let last = *self.frames.keys().next_back().unwrap();
            let victim = if distance(first) >= distance(last) {
                first
            } else {
                last
            };
            let frame = self.frames.remove(&victim).unwrap();
            self.bytes -= frame.bytes();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: CacheKey = CacheKey {
        version: 1,
        scale: PreviewScale::Full,
    };

    fn frame(index: usize) -> Frame {
        Frame {
            index,
            width: 1,
            height: 1,
            rgb: vec![index as u8; 3],
            levels: Arc::new([]),
            params: Arc::new([]),
            costs: Arc::new([]),
            meters: Arc::new([]),
            audio: None,
        }
    }

    #[test]
    fn stale_keys_are_never_served() {
        let mut cache = FrameCache::new(KEY, 1000);
        cache.insert(KEY, frame(0), 0);
        let edited = CacheKey { version: 2, ..KEY };
        cache.set_key(edited);
        assert!(
            cache.get(0).is_none(),
            "frames from the old version are gone"
        );
        assert!(
            !cache.insert(KEY, frame(1), 0),
            "late frames from the old version are refused"
        );
        assert!(cache.is_empty());
        assert!(cache.insert(edited, frame(1), 0));
        assert_eq!(cache.get(1).unwrap().rgb, [1, 1, 1]);
    }

    #[test]
    fn lists_cached_ranges() {
        let mut cache = FrameCache::new(KEY, 1000);
        for i in [0, 1, 2, 5, 7, 8] {
            cache.insert(KEY, frame(i), 0);
        }
        assert_eq!(cache.ranges(), [0..3, 5..6, 7..9]);
    }

    #[test]
    fn counts_consecutive_frames() {
        let mut cache = FrameCache::new(KEY, 1000);
        for i in [3, 4, 5, 7] {
            cache.insert(KEY, frame(i), 3);
        }
        assert_eq!(cache.run_from(3), 3);
        assert_eq!(cache.run_from(5), 1);
        assert_eq!(cache.run_from(6), 0);
        assert_eq!(cache.run_from(7), 1);
    }

    #[test]
    fn evicts_farthest_from_the_playhead_preferring_frames_behind() {
        // Room for 4 frames of 3 bytes.
        let mut cache = FrameCache::new(KEY, 12);
        for i in 0..10 {
            cache.insert(KEY, frame(i), 5);
        }
        assert_eq!(cache.bytes(), 12);
        let kept: Vec<usize> = (0..10).filter(|&i| cache.contains(i)).collect();
        // Distances from 5: frame 4 is 1 behind (counts as 2), 7 is 2 ahead, 8 is 3 ahead, 3 is 2
        // behind (counts as 4). The four nearest stay.
        assert_eq!(kept, [4, 5, 6, 7]);
    }

    #[test]
    fn replacing_a_frame_keeps_the_byte_count_right() {
        let mut cache = FrameCache::new(KEY, 1000);
        cache.insert(KEY, frame(0), 0);
        cache.insert(KEY, frame(0), 0);
        assert_eq!((cache.len(), cache.bytes()), (1, 3));
    }
}
