//! Full audio decoding against the generated fixtures: 440 Hz sine tones at amplitude 1/8, so
//! RMS 0.0884 per mono channel. audio_only.wav was upmixed to stereo at -3 dB per channel
//! (RMS 0.0625).

mod common;

use common::{backend, fixture};
use rastersong_media::{AudioClip, AudioOptions, MediaBackend, MediaError, StreamKind};

fn load(name: &str, options: AudioOptions) -> AudioClip {
    backend().load_audio(&fixture(name), options).unwrap()
}

/// RMS level and frequency (from zero crossings) of channel `channel`.
fn analyze(clip: &AudioClip, channel: usize) -> (f32, f64) {
    let samples: Vec<f32> = clip
        .samples
        .iter()
        .skip(channel)
        .step_by(clip.channels as usize)
        .copied()
        .collect();
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    let crossings = samples
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
    (rms, crossings as f64 / clip.duration_secs())
}

#[test]
fn decodes_pcm_exactly() {
    let clip = load("audio_only.wav", AudioOptions::default());
    assert_eq!(
        (clip.sample_rate, clip.channels, clip.frames()),
        (44_100, 2, 88_200)
    );
    for channel in 0..2 {
        let (rms, frequency) = analyze(&clip, channel);
        assert!((rms - 0.0625).abs() < 0.001, "rms {rms}");
        assert!((frequency - 440.0).abs() < 2.0, "frequency {frequency}");
    }
}

/// The samples of a WAV file's `data` chunk, read as little-endian `f32`.
fn wav_f32_samples(bytes: &[u8]) -> Vec<f32> {
    let mut at = 12; // "RIFF", size, "WAVE"
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if id == b"data" {
            return bytes[at + 8..at + 8 + size]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&b| f32::from_le_bytes(b))
                .collect();
        }
        at += 8 + size + size % 2;
    }
    panic!("no data chunk");
}

#[test]
fn float_audio_at_the_output_format_is_copied_exactly() {
    let clip = load("float.wav", AudioOptions::default());
    assert_eq!((clip.sample_rate, clip.channels), (48_000, 2));
    let expected = wav_f32_samples(&std::fs::read(fixture("float.wav")).unwrap());
    assert_eq!(clip.samples.len(), expected.len());
    assert!(*clip.samples == expected, "samples differ from the file");
}

#[test]
fn decodes_aac_without_priming_samples() {
    let clip = load("audio_only.m4a", AudioOptions::default());
    assert_eq!((clip.sample_rate, clip.channels), (48_000, 1));
    // The encoder's priming samples are trimmed via the container's edit list.
    assert!(
        clip.frames().abs_diff(96_000) <= 1024,
        "{} frames",
        clip.frames()
    );
    let (rms, frequency) = analyze(&clip, 0);
    assert!((rms - 0.0884).abs() < 0.005, "rms {rms}");
    assert!((frequency - 440.0).abs() < 2.0, "frequency {frequency}");
}

#[test]
fn resamples_and_remixes() {
    let clip = load(
        "audio_only.wav",
        AudioOptions {
            sample_rate: Some(48_000),
            channels: Some(1),
            stream: None,
        },
    );
    assert_eq!((clip.sample_rate, clip.channels), (48_000, 1));
    assert!(
        clip.frames().abs_diff(96_000) <= 64,
        "{} frames",
        clip.frames()
    );
    let (_, frequency) = analyze(&clip, 0);
    assert!((frequency - 440.0).abs() < 2.0, "frequency {frequency}");
}

#[test]
fn decodes_audio_from_video_files() {
    let clip = load("bframes.mp4", AudioOptions::default());
    assert_eq!(clip.sample_rate, 48_000);
    assert!(
        clip.frames().abs_diff(144_000) <= 1024,
        "{} frames",
        clip.frames()
    );
}

#[test]
fn audio_streams_are_found_from_the_header() {
    assert!(backend().has_audio(&fixture("bframes.mp4")));
    assert!(backend().has_audio(&fixture("audio_only.wav")));
    assert!(!backend().has_audio(&fixture("video_only.mp4")));
    let missing = fixture("audio_only.wav").with_file_name("does_not_exist.mp4");
    assert!(!backend().has_audio(&missing));
}

#[test]
fn video_only_files_have_no_audio() {
    assert!(matches!(
        backend().load_audio(&fixture("video_only.mp4"), AudioOptions::default()),
        Err(MediaError::NoAudioStream(_))
    ));
}

#[test]
fn streams_are_listed_and_chosen_by_index() {
    let path = fixture("two_audio.mkv");
    let streams = backend().streams(&path).unwrap();
    let summary: Vec<_> = streams
        .iter()
        .map(|s| {
            (
                s.index,
                s.kind.is_video(),
                s.title.as_deref(),
                s.language.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (0, true, None, None),
            (1, false, Some("Music"), Some("eng")),
            (2, false, Some("Voice"), None),
        ]
    );
    assert!(matches!(
        streams[0].kind,
        StreamKind::Video {
            width: 64,
            height: 48,
            ..
        }
    ));
    let load = |stream| {
        backend().load_audio(
            &path,
            AudioOptions {
                stream,
                ..AudioOptions::default()
            },
        )
    };
    assert_eq!(load(Some(1)).unwrap().channels, 1);
    assert_eq!(load(Some(2)).unwrap().channels, 2);
    // A video stream is not audio.
    assert!(matches!(load(Some(0)), Err(MediaError::NoAudioStream(_))));
    assert!(backend().open_video_stream(&path, Some(0)).is_ok());
    assert!(backend().open_video_stream(&path, Some(1)).is_err());
}

#[test]
fn audio_files_have_one_stream_and_missing_files_none() {
    let only_audio = backend().streams(&fixture("audio_only.m4a")).unwrap();
    assert_eq!(only_audio.len(), 1);
    assert!(!only_audio[0].kind.is_video());
    let missing = fixture("audio_only.m4a").with_file_name("does_not_exist.mp4");
    assert!(backend().streams(&missing).is_err());
}
