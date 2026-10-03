//! Video decoding against the generated fixtures. The core property: random access to frame
//! `i` returns exactly the bytes sequential decoding produces for frame `i`, for every fixture.
//! That holds for any codec without hand-made expected output.

mod common;

use common::{fixture, hash, open, shuffled};
use rastersong_media::{MediaBackend, MediaError, Rational, Rotation, VideoSource};

const VIDEO_FIXTURES: &[(&str, usize, u32, u32)] = &[
    // name, frames, width, height
    ("bframes.mp4", 90, 320, 240),
    ("open_gop.ts", 90, 320, 240),
    ("frame_index.mkv", 300, 64, 48),
    ("vfr.mkv", 60, 320, 240),
    ("odd_size.mkv", 48, 321, 241),
    ("video_only.mp4", 60, 320, 240),
    ("rotated.mp4", 90, 240, 320),
];

fn sequential_hashes(video: &mut dyn VideoSource) -> Vec<u64> {
    (0..video.info().frame_count)
        .map(|i| hash(&video.frame(i).unwrap().data))
        .collect()
}

#[test]
fn reports_frame_count_and_size() {
    for &(name, frames, width, height) in VIDEO_FIXTURES {
        let video = open(name);
        let info = video.info();
        assert_eq!(
            (info.frame_count, info.width, info.height),
            (frames, width, height),
            "{name}"
        );
    }
}

#[test]
fn random_access_matches_sequential_decode() {
    for &(name, ..) in VIDEO_FIXTURES {
        let expected = sequential_hashes(open(name).as_mut());
        let n = expected.len();

        let mut orders = vec![
            ("shuffled", shuffled(n, 0x5eed)),
            ("reverse", (0..n).rev().collect()),
        ];
        // Strided jumps forward, which mix the forward-decode and seek paths.
        orders.push((
            "strided",
            (0..n).step_by(7).chain((1..n).step_by(5)).collect(),
        ));

        for (label, order) in orders {
            let mut video = open(name);
            for i in order {
                let frame = video.frame(i).unwrap();
                assert_eq!(
                    hash(&frame.data),
                    expected[i],
                    "{name}: frame {i} ({label} order)"
                );
            }
        }
    }
}

#[test]
fn frames_have_the_right_index() {
    // Each frame of this fixture stores its own index: R = i % 256, G = i / 256.
    let mut video = open("frame_index.mkv");
    for i in shuffled(300, 7) {
        let frame = video.frame(i).unwrap();
        let [r, g, _] = frame.pixel(10, 10);
        assert_eq!(r as usize + 256 * g as usize, i);
    }
}

#[test]
fn repeated_requests_return_the_same_frame() {
    let mut video = open("bframes.mp4");
    let a = video.frame(42).unwrap();
    let b = video.frame(42).unwrap();
    assert!(std::sync::Arc::ptr_eq(&a, &b));
}

#[test]
fn output_size_scales_frames() {
    let mut video = open("bframes.mp4");
    video.set_output_size(Some((64, 48)));
    let expected = sequential_hashes(video.as_mut());
    assert_eq!(video.frame(10).unwrap().data.len(), 64 * 48 * 3);

    let mut video = open("bframes.mp4");
    video.set_output_size(Some((64, 48)));
    for i in shuffled(90, 3) {
        assert_eq!(
            hash(&video.frame(i).unwrap().data),
            expected[i],
            "frame {i}"
        );
    }
}

#[test]
fn rotated_video_is_turned_upright() {
    // rotated.mp4 is a stream copy of bframes.mp4 with a 90° counter-clockwise display matrix.
    let mut original = open("bframes.mp4");
    let mut rotated = open("rotated.mp4");
    assert_eq!(rotated.info().rotation, Rotation::Cw270);
    for i in [0, 13, 89] {
        let src = original.frame(i).unwrap();
        let dst = rotated.frame(i).unwrap();
        assert_eq!((dst.width, dst.height), (src.height, src.width));
        for (x, y) in [(0, 0), (5, 17), (319, 239), (100, 3)] {
            // Counter-clockwise: the source pixel (x, y) moves to (y, width - 1 - x).
            assert_eq!(
                dst.pixel(y, src.width - 1 - x),
                src.pixel(x, y),
                "frame {i} ({x}, {y})"
            );
        }
    }
}

#[test]
fn variable_frame_rate_timestamps() {
    let video = open("vfr.mkv");
    assert!((video.frame_time(29) - 29.0 / 30.0).abs() < 1e-3);
    assert!((video.frame_time(30) - 1.0).abs() < 1e-3);
    assert!((video.frame_time(31) - (1.0 + 1.0 / 15.0)).abs() < 1e-3);
}

#[test]
fn constant_frame_rate() {
    let video = open("bframes.mp4");
    assert_eq!(video.info().frame_rate, Rational::new(30, 1));
    assert!((video.frame_time(45) - 1.5).abs() < 1e-6);
}

#[test]
fn truncated_file_decodes_what_is_there() {
    // truncated.mp4 is the first 60% of bframes.mp4's bytes, with the index up front, so the
    // frames that survive must match the original exactly.
    let mut original = open("bframes.mp4");
    let mut truncated = open("truncated.mp4");
    let count = truncated.info().frame_count;
    assert!(count > 0 && count < 90, "{count} frames");

    let mut decoded = 0;
    for i in 0..count {
        match truncated.frame(i) {
            Ok(frame) => {
                assert_eq!(frame.data, original.frame(i).unwrap().data, "frame {i}");
                decoded += 1;
            }
            Err(MediaError::FrameUnavailable(_)) => {}
            Err(e) => panic!("frame {i}: {e}"),
        }
    }
    assert!(
        decoded * 10 >= count * 8,
        "only {decoded} of {count} frames decoded"
    );
}

#[test]
fn out_of_range_frames_are_an_error() {
    let mut video = open("video_only.mp4");
    assert!(matches!(
        video.frame(60),
        Err(MediaError::FrameOutOfRange {
            index: 60,
            count: 60
        })
    ));
}

#[test]
fn audio_only_files_have_no_video() {
    for name in ["audio_only.wav", "audio_only.m4a"] {
        assert!(matches!(
            common::backend().open_video(&fixture(name)),
            Err(MediaError::NoVideoStream(_))
        ));
    }
}

#[test]
fn missing_files_fail_to_open() {
    let result = common::backend().open_video(std::path::Path::new("does/not/exist.mp4"));
    assert!(matches!(result, Err(MediaError::Open { .. })));
}
