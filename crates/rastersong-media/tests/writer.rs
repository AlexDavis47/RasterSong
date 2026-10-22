//! The writer round-trips through our own decoder: bit-exactly for FFV1, closely for the lossy
//! formats.

use std::path::PathBuf;

use rastersong_media::{
    AudioClip, AudioOptions, ExportFormat, FfmpegBackend, FormatKind, MediaBackend, MediaWriter,
    ProResProfile, Quality, Rational, export_formats,
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

    let mut writer = MediaWriter::create(
        &path,
        ExportFormat::Ffv1,
        width,
        height,
        frame_rate,
        Some(&clip),
    )
    .unwrap();
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
    let mut writer = MediaWriter::create(
        &path,
        ExportFormat::Ffv1,
        8,
        8,
        Rational::new(30_000, 1001),
        None,
    )
    .unwrap();
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
    let mut writer = MediaWriter::create_streaming(
        &path,
        ExportFormat::Ffv1,
        8,
        8,
        Rational::new(25, 1),
        8000,
        1,
    )
    .unwrap();
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

/// A smooth gradient that moves with `index`, which lossy codecs keep close to the original.
fn gradient(index: usize, width: u32, height: u32) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height as usize {
        for x in 0..width as usize {
            rgb.extend([(x * 2 + index) as u8, (y * 2) as u8, 128]);
        }
    }
    rgb
}

/// Writes 10 frames and a second of stereo sound in `format`, reads them back and checks the
/// size, frame count, sound length and how far the pixels moved.
fn round_trip(format: ExportFormat, name: &str, max_error: f64) {
    let (width, height, frames) = (128, 96, 10);
    let frame_rate = Rational::new(25, 1);
    let clip = AudioClip {
        sample_rate: 48_000,
        channels: 2,
        samples: (0..48_000 * 2)
            .map(|i| ((i / 2) as f32 * 0.05).sin() * 0.5)
            .collect(),
    };
    let path = temp_file(name);
    let mut writer = MediaWriter::create(&path, format, width, height, frame_rate, Some(&clip))
        .unwrap_or_else(|e| panic!("{format:?}: {e}"));
    for i in 0..frames {
        writer.write_frame(&gradient(i, width, height)).unwrap();
    }
    writer.finish().unwrap();

    let backend = FfmpegBackend::new().unwrap();
    let mut video = backend.open_video(&path).unwrap();
    let info = video.info().clone();
    assert_eq!(
        (info.width, info.height, info.frame_count),
        (width, height, frames),
        "{format:?}"
    );
    for i in [0, frames / 2, frames - 1] {
        let got = video.frame(i).unwrap();
        let want = gradient(i, width, height);
        let error = got
            .data
            .iter()
            .zip(&want)
            .map(|(&a, &b)| f64::from(a.abs_diff(b)))
            .sum::<f64>()
            / want.len() as f64;
        assert!(
            error <= max_error,
            "{format:?} frame {i}: mean error {error}"
        );
    }

    // 10 frames at 25 fps = 0.4 s of sound.
    let audio = backend.load_audio(&path, AudioOptions::default()).unwrap();
    assert_eq!(
        (audio.sample_rate, audio.channels),
        (48_000, 2),
        "{format:?}"
    );
    let expected = 48_000 * 4 / 10;
    // AAC adds a little priming and padding.
    assert!(
        audio.frames().abs_diff(expected) <= 2048,
        "{format:?}: {} sound frames",
        audio.frames()
    );
}

#[test]
fn prores_round_trips() {
    for profile in ProResProfile::ALL {
        round_trip(
            ExportFormat::ProRes { profile },
            &format!("prores_{profile:?}.mov"),
            3.0,
        );
    }
}

#[test]
fn h264_round_trips_when_an_encoder_works_here() {
    let Some(h264) = export_formats().iter().find(|f| f.kind == FormatKind::H264) else {
        eprintln!("skipped: no H.264 encoder works on this machine");
        return;
    };
    eprintln!("H.264 through {}", h264.video_encoder);
    round_trip(
        ExportFormat::H264 {
            quality: Quality::High,
        },
        "h264.mp4",
        6.0,
    );
}

#[test]
fn the_lossless_formats_are_always_available() {
    let kinds: Vec<_> = export_formats().iter().map(|f| f.kind).collect();
    assert!(kinds.contains(&FormatKind::ProRes), "{kinds:?}");
    assert!(kinds.contains(&FormatKind::Ffv1), "{kinds:?}");
}

/// FFmpeg's encoders default to one thread; ProRes and FFV1 then encode on one core.
#[test]
fn the_cpu_encoders_use_more_than_one_thread() {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    if cores < 2 {
        eprintln!("skipped: one core");
        return;
    }
    for format in [
        ExportFormat::ProRes {
            profile: ProResProfile::Hq,
        },
        ExportFormat::Ffv1,
    ] {
        let path = temp_file(&format!("threads.{}", format.kind().extension()));
        let writer =
            MediaWriter::create(&path, format, 1920, 1080, Rational::new(25, 1), None).unwrap();
        assert!(
            writer.video_threads() > 1,
            "{format:?}: {} thread(s) on {cores} cores",
            writer.video_threads()
        );
    }
}

#[test]
fn odd_sizes_are_refused_where_chroma_is_subsampled() {
    let path = temp_file("odd.mov");
    let err = MediaWriter::create(
        &path,
        ExportFormat::ProRes {
            profile: ProResProfile::Hq,
        },
        33,
        17,
        Rational::new(25, 1),
        None,
    )
    .unwrap_err();
    assert!(err.to_string().contains("33×17"), "{err}");
}
