//! The frame index: built from packets alone (no decoding), it maps frame numbers to
//! presentation timestamps and tells the decoder which keyframe to start from for any frame.

/// What the index needs to know about one packet of the video stream, in decode order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PacketMeta {
    pub pts: Option<i64>,
    pub dts: Option<i64>,
    pub key: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Keyframe {
    pub pts: i64,
    /// Decode timestamp, if the container provides one. Some demuxers (e.g. MPEG-TS) seek by
    /// decode time, where seeking to `pts` would land after the keyframe.
    pub dts: Option<i64>,
    /// The keyframe's own frame number.
    pub frame: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrameIndex {
    /// Presentation timestamp of each frame, ascending. Frame `i` is `pts[i]`.
    pts: Vec<i64>,
    /// Keyframes, ascending by timestamp.
    keyframes: Vec<Keyframe>,
    /// For each frame, the keyframe (index into `keyframes`) that decoding must start from.
    start_key: Vec<usize>,
}

impl FrameIndex {
    /// Assumes one frame per packet, which holds for every modern container and codec.
    ///
    /// Frames that can't be decoded from a clean start are left out: anything before the first
    /// keyframe in decode order, and frames shown before the first keyframe (the leading
    /// frames of an open GOP whose references are missing).
    pub fn build(packets: impl IntoIterator<Item = PacketMeta>) -> Self {
        let mut pts = Vec::new();
        let mut keys = Vec::new();
        for packet in packets {
            let Some(t) = packet.pts.or(packet.dts) else {
                continue;
            };
            if packet.key {
                keys.push((t, packet.dts));
            } else if keys.is_empty() {
                continue;
            }
            pts.push(t);
        }

        keys.sort_unstable();
        keys.dedup_by_key(|(t, _)| *t);
        if let Some(&(first_key, _)) = keys.first() {
            pts.retain(|&t| t >= first_key);
        }
        pts.sort_unstable();
        pts.dedup();

        let keyframes: Vec<Keyframe> = keys
            .iter()
            .map(|&(t, dts)| Keyframe {
                pts: t,
                dts,
                frame: pts.binary_search(&t).expect("keyframes are frames"),
            })
            .collect();

        // Start from the last keyframe shown at or before the frame. For the leading frames of an
        // open GOP (decoded after a keyframe but shown before it) this is the previous keyframe,
        // whose GOP contains their references.
        let start_key = pts
            .iter()
            .map(|&t| keyframes.partition_point(|k| k.pts <= t) - 1)
            .collect();

        Self {
            pts,
            keyframes,
            start_key,
        }
    }

    pub fn len(&self) -> usize {
        self.pts.len()
    }

    pub fn pts(&self, frame: usize) -> i64 {
        self.pts[frame]
    }

    /// The frame shown at exactly `pts`, if any.
    pub fn frame_at(&self, pts: i64) -> Option<usize> {
        self.pts.binary_search(&pts).ok()
    }

    /// Index (into [`Self::keyframe`]) of the keyframe to start decoding from to reach `frame`.
    pub fn start_key(&self, frame: usize) -> usize {
        self.start_key[frame]
    }

    pub fn keyframe(&self, key: usize) -> Keyframe {
        self.keyframes[key]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(pts: i64, dts: i64) -> PacketMeta {
        PacketMeta {
            pts: Some(pts),
            dts: Some(dts),
            key: true,
        }
    }

    fn inter(pts: i64, dts: i64) -> PacketMeta {
        PacketMeta {
            pts: Some(pts),
            dts: Some(dts),
            key: false,
        }
    }

    #[test]
    fn orders_frames_by_presentation_time() {
        // I0 P3 B1 B2 in decode order.
        let index = FrameIndex::build([key(0, -1), inter(3, 0), inter(1, 1), inter(2, 2)]);
        assert_eq!(index.len(), 4);
        assert_eq!(
            (0..4).map(|i| index.pts(i)).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(index.frame_at(2), Some(2));
        assert_eq!(index.frame_at(7), None);
    }

    #[test]
    fn open_gop_leading_frames_start_from_the_previous_keyframe() {
        // GOP 1: I0 P3 B1 B2. GOP 2 (open): I6 B4 B5 P9 B7 B8, where B4/B5 are shown before I6
        // and reference P3.
        let index = FrameIndex::build([
            key(0, -1),
            inter(3, 0),
            inter(1, 1),
            inter(2, 2),
            key(6, 3),
            inter(4, 4),
            inter(5, 5),
            inter(9, 6),
            inter(7, 7),
            inter(8, 8),
        ]);
        let start = |frame| index.keyframe(index.start_key(frame));
        assert_eq!(start(3).pts, 0);
        assert_eq!(start(4).pts, 0, "leading B-frame needs the previous GOP");
        assert_eq!(start(5).pts, 0, "leading B-frame needs the previous GOP");
        assert_eq!(start(6).pts, 6);
        assert_eq!(start(9).pts, 6);
        assert_eq!(start(9).frame, 6);
    }

    #[test]
    fn drops_frames_that_cannot_be_decoded_from_a_clean_start() {
        // A stream cut mid-GOP: two inter frames, then an open-GOP keyframe whose leading
        // B-frame (pts 4) references data before the cut.
        let index = FrameIndex::build([
            inter(1, 0),
            inter(2, 1),
            key(5, 2),
            inter(4, 3),
            inter(6, 4),
        ]);
        assert_eq!(
            (0..index.len()).map(|i| index.pts(i)).collect::<Vec<_>>(),
            [5, 6]
        );
        assert_eq!(index.start_key(0), 0);
    }

    #[test]
    fn falls_back_to_dts_and_skips_untimed_packets() {
        let index = FrameIndex::build([
            PacketMeta {
                pts: None,
                dts: Some(0),
                key: true,
            },
            PacketMeta {
                pts: None,
                dts: None,
                key: false,
            },
            PacketMeta {
                pts: None,
                dts: Some(1),
                key: false,
            },
        ]);
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn empty_stream() {
        assert_eq!(FrameIndex::build([]).len(), 0);
    }
}
