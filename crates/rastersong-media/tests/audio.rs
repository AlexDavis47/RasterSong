//! Full audio decoding against the generated fixtures: 440 Hz sine tones at amplitude 1/8, so
//! RMS 0.0884 per mono channel. audio_only.wav was upmixed to stereo at -3 dB per channel
//! (RMS 0.0625).

mod common;

use common::{backend, fixture};
use rastersong_media::{AudioClip, AudioOptions, MediaBackend, MediaError};

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
    assert!(clip.samples == expected, "samples differ from the file");
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
fn video_only_files_have_no_audio() {
    assert!(matches!(
        backend().load_audio(&fixture("video_only.mp4"), AudioOptions::default()),
        Err(MediaError::NoAudioStream(_))
    ));
}
