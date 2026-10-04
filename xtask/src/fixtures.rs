//! `cargo xtask fixtures`: generates small media files that exercise the hard cases of decoding.
//!
//! Fixtures use only encoders built into LGPL FFmpeg, so the fetched FFmpeg can generate them on
//! every platform. Media tests don't compare against expected bytes (they check random-access decode
//! against sequential decode), so fixtures don't need to be bit-identical across machines.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Result, ensure};

use crate::util::{run, workspace_root};

/// Flags shared by every encode, for reproducible output.
const COMMON: &[&str] = &[
    "-hide_banner",
    "-loglevel",
    "error",
    "-y",
    "-fflags",
    "+bitexact",
];
/// Output options for reproducible bytes. Encoders split work by thread count (MPEG-4 uses one
/// slice per thread), so a single thread keeps fixtures identical across machines and CI runners.
const BITEXACT_CODECS: &[&str] = &[
    "-flags:v",
    "+bitexact",
    "-flags:a",
    "+bitexact",
    "-threads",
    "1",
];

/// Test pattern with motion, so inter-frame codecs produce real P/B-frames.
const PATTERN: &str = "testsrc2=size=320x240:rate=30";
const TONE: &str = "sine=frequency=440:sample_rate=48000";

struct Fixture {
    name: &'static str,
    /// What the fixture covers.
    purpose: &'static str,
    args: &'static [&'static str],
}

#[rustfmt::skip]
const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "bframes.mp4",
        purpose: "MPEG-4 Part 2 with B-frames (decode order != presentation order) and AAC audio",
        args: &[
            "-f", "lavfi", "-i", PATTERN, "-f", "lavfi", "-i", TONE, "-t", "3",
            "-c:v", "mpeg4", "-q:v", "4", "-bf", "2", "-g", "15",
            "-c:a", "aac", "-b:a", "96k", "-movflags", "+faststart",
        ],
    },
    Fixture {
        name: "open_gop.ts",
        purpose: "MPEG-2 in MPEG-TS with open GOPs: B-frames after a keyframe reference the previous GOP",
        args: &[
            "-f", "lavfi", "-i", PATTERN, "-f", "lavfi", "-i", TONE, "-t", "3",
            "-c:v", "mpeg2video", "-q:v", "4", "-bf", "2", "-g", "12", "-flags:v", "-cgop",
            "-c:a", "mp2",
        ],
    },
    Fixture {
        name: "frame_index.mkv",
        purpose: "Lossless FFV1 where every pixel stores its frame index: R = index % 256, G = index / 256",
        args: &[
            "-f", "lavfi", "-i", "color=size=64x48:rate=30:duration=10",
            "-vf", r"format=gbrp,geq=r='mod(N\,256)':g='trunc(N/256)':b=128",
            "-c:v", "ffv1", "-g", "30",
        ],
    },
    Fixture {
        name: "vfr.mkv",
        purpose: "Variable frame rate: 30 fps for 1 s, then 15 fps for 2 s",
        args: &[
            "-f", "lavfi", "-i", "testsrc2=size=320x240:rate=30:duration=2",
            "-vf", r"setpts='if(lt(N\,30)\,N/30\,1+(N-30)/15)/TB'",
            "-fps_mode", "vfr", "-c:v", "mpeg4", "-q:v", "4", "-bf", "2",
        ],
    },
    Fixture {
        name: "odd_size.mkv",
        purpose: "Odd dimensions (321x241) in a 4:4:4 lossless codec",
        args: &[
            // testsrc2 rounds odd sizes down, so scale to the odd size afterwards.
            "-f", "lavfi", "-i", "testsrc2=size=320x240:rate=24:duration=2",
            "-vf", "scale=321:241", "-pix_fmt", "yuv444p", "-c:v", "ffv1",
        ],
    },
    Fixture {
        name: "video_only.mp4",
        purpose: "No audio stream; a single long GOP",
        args: &["-f", "lavfi", "-i", PATTERN, "-t", "2", "-c:v", "mpeg4", "-q:v", "4", "-g", "60"],
    },
    Fixture {
        name: "audio_only.wav",
        purpose: "No video stream; 16-bit PCM stereo at 44.1 kHz",
        args: &[
            "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100:duration=2",
            "-ac", "2", "-c:a", "pcm_s16le",
        ],
    },
    Fixture {
        name: "float.wav",
        purpose: "No video stream; 32-bit float PCM stereo at 48 kHz, already in the decoded format",
        args: &[
            "-f", "lavfi", "-i", "sine=frequency=330:sample_rate=48000:duration=1",
            "-ac", "2", "-c:a", "pcm_f32le",
        ],
    },
    Fixture {
        name: "audio_only.m4a",
        purpose: "No video stream; compressed AAC mono at 48 kHz (encoder delay and priming samples)",
        args: &["-f", "lavfi", "-i", TONE, "-t", "2", "-c:a", "aac", "-b:a", "96k"],
    },
    Fixture {
        name: "rgb_pattern.mkv",
        purpose: "Moving test pattern in lossless 8-bit RGB FFV1, for golden renders (decodes identically everywhere)",
        args: &[
            // Drawn natively in RGB. Converting from testsrc2's default YUV would go through
            // swscale's SIMD paths, which round differently on x86 and ARM.
            "-f", "lavfi", "-i", "testsrc2=size=160x120:rate=30:duration=2,format=bgr0",
            "-pix_fmt", "bgr0", "-c:v", "ffv1",
        ],
    },
    Fixture {
        name: "music.wav",
        purpose: "Modulator with separate bands: pulsing 60 Hz bass, 1 kHz mid (first second), 6 kHz treble (second second)",
        args: &[
            "-f", "lavfi", "-i",
            "aevalsrc=0.4*sin(2*PI*60*t)*(0.5+0.5*sin(2*PI*2*t))+0.3*sin(2*PI*1000*t)*lt(t\\,1)+0.2*sin(2*PI*6000*t)*gte(t\\,1):s=44100:d=2",
            "-c:a", "pcm_s16le",
        ],
    },
];

pub fn generate() -> Result<()> {
    let root = workspace_root();
    let ffmpeg_bin = root.join("third_party/ffmpeg/bin").join(if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    });
    ensure!(
        ffmpeg_bin.exists(),
        "{} not found; run `cargo xtask fetch-ffmpeg` first",
        ffmpeg_bin.display()
    );

    let out_dir = root.join("fixtures");
    fs::create_dir_all(&out_dir)?;
    let ffmpeg = |args: &[&str], output: &Path| {
        run(Command::new(&ffmpeg_bin)
            .args(COMMON)
            .args(args)
            .args(BITEXACT_CODECS)
            .arg(output))
    };

    for fixture in FIXTURES {
        println!("{:<16} {}", fixture.name, fixture.purpose);
        ffmpeg(fixture.args, &out_dir.join(fixture.name))?;
    }

    // Derived fixtures.
    println!(
        "{:<16} Display matrix rotated 90 degrees (stream copy of bframes.mp4)",
        "rotated.mp4"
    );
    let bframes = out_dir.join("bframes.mp4");
    run(Command::new(&ffmpeg_bin)
        .args(COMMON)
        .args(["-display_rotation", "90", "-i"])
        .arg(&bframes)
        .args(["-c", "copy"])
        .arg(out_dir.join("rotated.mp4")))?;

    println!(
        "{:<16} bframes.mp4 cut off at 60% of its length (index up front, data missing)",
        "truncated.mp4"
    );
    let bytes = fs::read(&bframes)?;
    fs::write(
        out_dir.join("truncated.mp4"),
        &bytes[..bytes.len() * 6 / 10],
    )?;

    println!("Fixtures written to {}", out_dir.display());
    Ok(())
}
