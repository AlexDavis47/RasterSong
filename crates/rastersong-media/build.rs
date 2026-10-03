//! Copies the FFmpeg shared libraries from `FFMPEG_DIR` next to the binaries and test executables
//! Cargo builds, so `cargo run` and `cargo test` work without touching PATH or LD_LIBRARY_PATH.
//! Linking itself is handled by ffmpeg-sys-next; release packaging lays out the libraries separately.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo::rerun-if-env-changed=FFMPEG_DIR");
    let Some(ffmpeg_dir) = env::var_os("FFMPEG_DIR").map(PathBuf::from) else {
        return;
    };
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let (lib_dir, is_runtime_lib): (PathBuf, fn(&str) -> bool) = match target_os.as_str() {
        "windows" => (ffmpeg_dir.join("bin"), |n| n.ends_with(".dll")),
        "macos" => (ffmpeg_dir.join("lib"), |n| n.ends_with(".dylib")),
        // Only the SONAME files (`libavcodec.so.62`), not the dev symlink or the fully versioned name.
        _ => (ffmpeg_dir.join("lib"), |n| {
            n.split_once(".so.").is_some_and(|(_, v)| !v.contains('.'))
        }),
    };
    let Ok(entries) = fs::read_dir(&lib_dir) else {
        return;
    };
    println!("cargo::rerun-if-changed={}", lib_dir.display());

    // OUT_DIR is <target>/<profile>/build/<package>-<hash>/out.
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let profile_dir = out_dir.ancestors().nth(3).unwrap();
    let destinations = [
        profile_dir.to_path_buf(),
        profile_dir.join("deps"),
        profile_dir.join("examples"),
    ];

    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_str().is_some_and(is_runtime_lib) {
            continue;
        }
        for dest in &destinations {
            copy_if_changed(&entry.path(), &dest.join(&name));
        }
    }
}

fn copy_if_changed(src: &Path, dest: &Path) {
    let src_meta = fs::metadata(src).unwrap();
    if let Ok(dest_meta) = fs::metadata(dest)
        && dest_meta.len() == src_meta.len()
        && dest_meta.modified().ok() >= src_meta.modified().ok()
    {
        return;
    }
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    fs::copy(src, dest)
        .unwrap_or_else(|e| panic!("copying {} to {}: {e}", src.display(), dest.display()));
}
