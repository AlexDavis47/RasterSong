//! End-to-end golden renders: the CLI renders each example graph and selected frames are compared
//! to reference images in `tests/golden/`, with small tolerances for floating-point differences
//! between platforms.
//!
//! After an intended change to how something renders, look at the new frames, then update the
//! references with `RASTERSONG_BLESS=1 cargo test -p rastersong-cli --test golden`.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

const GRAPHS: &[&str] = &["am_bands", "bass_wave", "packed_crush"];
const FRAMES: &[usize] = &[0, 10, 45];

/// Share of samples allowed to differ by more than `LARGE_DIFF` (e.g. a pixel crossing a
/// bit-crush threshold on one platform but not another).
const MAX_LARGE_DIFF_SHARE: f64 = 0.002;
const LARGE_DIFF: u8 = 8;
const MAX_MEAN_DIFF: f64 = 0.5;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    let path = workspace().join("fixtures").join(name);
    assert!(
        path.exists(),
        "missing fixture {}; run `cargo xtask fixtures`",
        path.display()
    );
    path
}

fn read_png(path: &Path) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(
        File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
    ));
    let mut reader = decoder.read_info().unwrap();
    let mut data = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut data).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgb, "{}", path.display());
    data.truncate(info.buffer_size());
    (info.width, info.height, data)
}

#[test]
fn example_graphs_match_golden_frames() {
    let bless = std::env::var_os("RASTERSONG_BLESS").is_some();
    let mut failures = Vec::new();

    for graph in GRAPHS {
        let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("golden")
            .join(graph);
        let _ = std::fs::remove_dir_all(&out);
        let status = Command::new(env!("CARGO_BIN_EXE_rastersong-cli"))
            .arg("render")
            .arg(fixture("rgb_pattern.mkv"))
            .arg(fixture("music.wav"))
            .arg(
                workspace()
                    .join("examples/graphs")
                    .join(format!("{graph}.json")),
            )
            .arg(&out)
            .args(["--frames", "46"])
            .status()
            .unwrap();
        assert!(status.success(), "rendering {graph} failed");

        for &frame in FRAMES {
            let name = format!("frame_{frame:05}.png");
            let rendered = out.join(&name);
            let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/golden")
                .join(graph)
                .join(&name);
            if bless {
                std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
                std::fs::copy(&rendered, &golden).unwrap();
                continue;
            }

            let (w, h, actual) = read_png(&rendered);
            let (gw, gh, expected) = read_png(&golden);
            if (w, h) != (gw, gh) {
                failures.push(format!("{graph}/{name}: size {w}x{h}, expected {gw}x{gh}"));
                continue;
            }
            let diffs: Vec<u8> = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| a.abs_diff(*b))
                .collect();
            let mean = diffs.iter().map(|&d| f64::from(d)).sum::<f64>() / diffs.len() as f64;
            let large =
                diffs.iter().filter(|&&d| d > LARGE_DIFF).count() as f64 / diffs.len() as f64;
            if mean > MAX_MEAN_DIFF || large > MAX_LARGE_DIFF_SHARE {
                failures.push(format!(
                    "{graph}/{name}: mean difference {mean:.3}, {:.3}% of samples off by more than {LARGE_DIFF} (rendered: {})",
                    large * 100.0,
                    rendered.display()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "golden frames differ:\n{}",
        failures.join("\n")
    );
}
