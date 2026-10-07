//! Listening to a connection: the sound of what it carries, rendered ahead of a position.
//!
//! Like a tap, listening is answered by the renderer the service keeps for taps, so it never
//! touches the cache or the render-ahead. What the connection carries goes through the same
//! sanitize, resample and clip path as the Audio Output node, so what is heard is what an Audio
//! Output wired to the connection would play.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::AudioBlock;

/// Seconds of sound kept rendered ahead of the listening position.
pub const AHEAD_SECS: f64 = 1.0;
/// Frames kept behind the position, so a small step back still has sound.
pub const BEHIND_FRAMES: usize = 30;

/// The connection to listen to: output `output` of node `node`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ListenTarget {
    pub node: String,
    pub output: usize,
}

/// The sound rendered so far for the current target, one block per video frame. A block with no
/// samples marks a frame whose connection isn't rendered, so it is not asked for again.
#[derive(Debug, Default)]
pub(crate) struct Listened {
    pub blocks: BTreeMap<usize, Arc<AudioBlock>>,
}

impl Listened {
    /// Keeps the frames near `frame`.
    pub fn trim(&mut self, frame: usize, ahead: usize) {
        let from = frame.saturating_sub(BEHIND_FRAMES);
        let to = frame.saturating_add(ahead + BEHIND_FRAMES);
        self.blocks.retain(|&f, _| (from..=to).contains(&f));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimming_keeps_the_frames_around_the_position() {
        let mut listened = Listened::default();
        let block = Arc::new(AudioBlock {
            start: 0,
            sample_rate: 48_000,
            channels: 1,
            samples: Vec::new(),
        });
        for f in [0, 50, 100, 140, 400] {
            listened.blocks.insert(f, block.clone());
        }
        listened.trim(100, 30);
        assert_eq!(
            listened.blocks.keys().copied().collect::<Vec<_>>(),
            [100, 140]
        );
    }
}
