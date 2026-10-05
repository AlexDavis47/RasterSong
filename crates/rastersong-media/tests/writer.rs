//! The lossless writer round-trips through our own decoder bit-exactly.

use std::path::PathBuf;

use rastersong_media::{
    AudioClip, AudioOptions, FfmpegBackend, LosslessWriter, MediaBackend, Rational,
};

fn temp_file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}

fn pattern(index: usize, width: u32, height: u32) -> Vec<u8> {
    (0..width * height * 3)
        .map(|i| (i as usize * 7 + index * 13) as u8)
        .collect()
}

#[test]
fn frames_and_audio_round_trip() {
    let (width, height, frames) = (33, 17, 12);
    let frame_rate = Rational::new(24, 1);
    let clip = AudioClip {
        sample_rate: 8000,
        channels: 2,
        samples: (0..8000 * 2)
            .map(|i| ((i / 2) as f32 * 0.01).sin() * if i % 2 == 0 { 0.5 } else { -0.25 })
            .collect(),
    };
    let path = temp_file("round_trip.mkv");

    let mut writer = LosslessWriter::create(&path, width, height, frame_rate, Some(&clip)).unwrap();
    for i in 0..frames {
        writer.write_frame(&pattern(i, width, height)).unwrap();
    }
    writer.finish().unwrap();

    let backend = FfmpegBackend::new().unwrap();
    let mut video = backend.open_video(&path).unwrap();
    let info = video.info().clone();
    assert_eq!(
        (info.width, info.height, info.frame_count),
        (width, height, frames)
    );
    assert_eq!(info.frame_rate, frame_rate);
    for i in (0..frames).rev() {
        assert_eq!(
            video.frame(i).unwrap().data,
            pattern(i, width, height),
            "frame {i}"
        );
    }

    // Audio is written up to the end of the video: 12 frames at 24 fps = 0.5 s.
    let audio = backend.load_audio(&path, AudioOptions::default()).unwrap();
    assert_eq!((audio.sample_rate, audio.channels), (8000, 2));
    assert_eq!(audio.frames(), 4000);
    for (i, (&got, &want)) in audio.samples.iter().zip(&clip.samples).enumerate() {
        assert!(
            (got - want).abs() <= 1.0 / 32767.0,
            "sample {i}: {got} vs {want}"
        );
    }
}

#[test]
fn video_without_audio() {
    let path = temp_file("video_only_out.mkv");
    let mut writer =
        LosslessWriter::create(&path, 8, 8, Rational::new(30_000, 1001), None).unwrap();
    writer.write_frame(&pattern(0, 8, 8)).unwrap();
    writer.finish().unwrap();

    let backend = FfmpegBackend::new().unwrap();
    assert_eq!(backend.open_video(&path).unwrap().info().frame_count, 1);
    assert!(backend.load_audio(&path, AudioOptions::default()).is_err());
}

#[test]
fn streamed_audio_is_written_as_far_as_the_video() {
    // 10 frames at 25 fps with 320 mono samples each at 8 kHz, pushed before each frame.
    let path = temp_file("streamed_audio.mkv");
    let mut writer =
        LosslessWriter::create_streaming(&path, 8, 8, Rational::new(25, 1), 8000, 1).unwrap();
    let sample = |i: usize| ((i % 50) as f32 / 50.0) - 0.5;
    for frame in 0..10 {
        let block: Vec<f32> = (frame * 320..(frame + 1) * 320).map(sample).collect();
        writer.push_audio(&block);
        writer.write_frame(&pattern(frame, 8, 8)).unwrap();
    }
    writer.finish().unwrap();

    let audio = FfmpegBackend::new()
        .unwrap()
        .load_audio(&path, AudioOptions::default())
        .unwrap();
    assert_eq!(
        (audio.sample_rate, audio.channels, audio.frames()),
        (8000, 1, 3200)
    );
    for (i, &got) in audio.samples.iter().enumerate() {
        assert!((got - sample(i)).abs() <= 1.0 / 32767.0, "sample {i}");
    }
}
