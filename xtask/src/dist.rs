//! `cargo xtask dist`: builds the app in release mode and packages it as a zip for testers.
//!
//! The package is a flat folder holding `rastersong.exe`, the FFmpeg DLLs it links and the license
//! files. It is a stopgap for user testing until real installers (see "Packaging" in the readme).

use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use zip::write::SimpleFileOptions;

use crate::util::{run, workspace_root};

/// An explicit target puts the build in its own directory, so the static CRT flag below applies
/// only to it and doesn't invalidate the regular `target/release` build.
const TARGET: &str = "x86_64-pc-windows-msvc";

/// The FFmpeg libraries the app loads: those enabled by the `ffmpeg-next` features in the workspace
/// Cargo.toml, plus avutil. avdevice and avfilter are fetched but unused.
const FFMPEG_LIBS: &[&str] = &["avcodec", "avformat", "avutil", "swresample", "swscale"];

pub fn package() -> Result<()> {
    if !cfg!(windows) {
        bail!("`cargo xtask dist` only packages Windows builds so far");
    }
    let root = workspace_root();

    // Link the C runtime statically, so the exe runs on machines without the VC++ redistributable.
    // The FFmpeg DLLs are MinGW builds and don't need it.
    run(Command::new(env!("CARGO"))
        .current_dir(&root)
        .args([
            "build",
            "--release",
            "-p",
            "rastersong-gui",
            "--target",
            TARGET,
        ])
        .env(
            "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS",
            "-C target-feature=+crt-static",
        ))?;
    let build_dir = root.join("target").join(TARGET).join("release");

    let name = format!("RasterSong-{}-windows-x64", version_label(&root));
    let dist_dir = root.join("target").join("dist");
    let package_dir = dist_dir.join(&name);
    if package_dir.exists() {
        fs::remove_dir_all(&package_dir)?;
    }
    fs::create_dir_all(&package_dir)?;

    copy(&build_dir.join("rastersong.exe"), &package_dir)?;
    for entry in fs::read_dir(&build_dir)? {
        let path = entry?.path();
        let file_name = path.file_name().unwrap().to_string_lossy();
        // `avcodec-63.dll` -> `avcodec`
        let is_needed = file_name
            .strip_suffix(".dll")
            .and_then(|stem| stem.rsplit_once('-'))
            .is_some_and(|(lib, _)| FFMPEG_LIBS.contains(&lib));
        if is_needed {
            copy(&path, &package_dir)?;
        }
    }
    copy(&root.join("LICENSE"), &package_dir)?;
    fs::copy(
        root.join("third_party/ffmpeg/LICENSE.txt"),
        package_dir.join("FFmpeg-LICENSE.txt"),
    )
    .context("copying the FFmpeg license (run `cargo xtask fetch-ffmpeg` first)")?;

    let zip_path = dist_dir.join(format!("{name}.zip"));
    write_zip(&package_dir, &name, &zip_path)?;
    println!("Packaged {}", zip_path.display());
    Ok(())
}

/// The workspace version plus the commit, so tester reports can be traced to a build.
fn version_label(root: &Path) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let git = |args: &[&str]| {
        Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let Some(commit) = git(&["rev-parse", "--short", "HEAD"]) else {
        return version.to_owned();
    };
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    format!("{version}-{commit}{}", if dirty { "-dirty" } else { "" })
}

fn copy(src: &Path, dest_dir: &Path) -> Result<()> {
    fs::copy(src, dest_dir.join(src.file_name().unwrap()))
        .with_context(|| format!("copying {}", src.display()))?;
    Ok(())
}

/// Zips the files in `dir` under a top-level folder `prefix`, so unzipping yields one folder.
fn write_zip(dir: &Path, prefix: &str, dest: &Path) -> Result<()> {
    let mut zip = zip::ZipWriter::new(BufWriter::new(File::create(dest)?));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let file_name = entry.file_name();
        zip.start_file(format!("{prefix}/{}", file_name.to_string_lossy()), options)?;
        io::copy(&mut File::open(entry.path())?, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}
