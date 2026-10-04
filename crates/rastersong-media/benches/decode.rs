//! Decode throughput on the generated fixtures (`cargo xtask fixtures` first).
//! Run with `cargo bench -p rastersong-media`.

use std::hint::black_box;
use std::path::PathBuf;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rastersong_media::{FfmpegBackend, MediaBackend};

fn fixture(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    assert!(
        path.exists(),
        "missing fixture {}; run `cargo xtask fixtures`",
        path.display()
    );
    path
}

fn decode(c: &mut Criterion) {
    let backend = FfmpegBackend::new().unwrap();
    let mut group = c.benchmark_group("decode");
    group.sample_size(10);

    // MPEG-4 with B-frames, 320×240, 90 frames.
    let path = fixture("bframes.mp4");
    let frames = backend.open_video(&path).unwrap().info().frame_count;

    group.throughput(Throughput::Elements(frames as u64));
    group.bench_function("open + sequential (bframes.mp4)", |b| {
        b.iter(|| {
            let mut video = backend.open_video(&path).unwrap();
            for i in 0..frames {
                black_box(video.frame(i).unwrap());
            }
        });
    });

    // Scattered access: every request seeks.
    let order: Vec<usize> = (0..frames).map(|i| (i * 37 + 11) % frames).collect();
    let mut video = backend.open_video(&path).unwrap();
    group.bench_function("random access (bframes.mp4)", |b| {
        b.iter(|| {
            for &i in &order {
                black_box(video.frame(i).unwrap());
            }
        });
    });

    group.throughput(Throughput::Elements(1));
    group.bench_function("load audio (music.wav, 2 s)", |b| {
        let audio = fixture("music.wav");
        b.iter(|| black_box(backend.load_audio(&audio, Default::default()).unwrap()));
    });
    group.finish();
}

criterion_group!(benches, decode);
criterion_main!(benches);
