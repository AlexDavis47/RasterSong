//! `cargo xtask dist`: builds the app in release mode against the release FFmpeg
//! (`cargo xtask build-ffmpeg`) and packages it for testers in `target/dist/`:
//!
//! - Windows: a zip of a flat folder holding `rastersong.exe`, the FFmpeg DLLs and the notices.
//! - macOS: a zip of a universal `RasterSong.app` with the FFmpeg libraries in `Contents/Frameworks`,
//!   ad-hoc signed (no Apple developer account, so not notarized).
//! - Linux: an AppImage, made with `appimagetool` (from PATH, or the `APPIMAGETOOL` variable).
//!
//! Next to the package goes the FFmpeg source tarball the libraries were built from, which releases
//! publish alongside the packages. These are unsigned stopgaps for testing until real installers
//! (see "Packaging" in docs/development.md).

use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use zip::write::SimpleFileOptions;

use crate::release_ffmpeg::{self, LIBS};
use crate::util::{run, workspace_root};

const APP_NAME: &str = "RasterSong";
const BINARY: &str = "rastersong";
const BUNDLE_ID: &str = "io.github.alexdavis47.rastersong";
const ICON: &str = "crates/rastersong-gui/assets/icon.png";

pub fn package() -> Result<()> {
    let root = workspace_root();
    let ffmpeg_dir = release_ffmpeg::build(false)?;
    let dist_dir = root.join("target").join("dist");
    fs::create_dir_all(&dist_dir)?;
    let label = version_label(&root);

    let package = match std::env::consts::OS {
        "windows" => windows(&root, &ffmpeg_dir, &dist_dir, &label)?,
        "macos" => macos(&root, &ffmpeg_dir, &dist_dir, &label)?,
        "linux" => linux(&root, &ffmpeg_dir, &dist_dir, &label)?,
        os => bail!("`cargo xtask dist` doesn't package for {os}"),
    };

    let source = release_ffmpeg::source_archive();
    fs::copy(&source, dist_dir.join(source.file_name().unwrap()))?;
    println!("Packaged {}", package.display());
    Ok(())
}

fn windows(root: &Path, ffmpeg_dir: &Path, dist_dir: &Path, label: &str) -> Result<PathBuf> {
    // An explicit target puts the build in its own directory, so the static CRT flag below applies
    // only to it and doesn't invalidate the regular `target/release` build. The C runtime is linked
    // statically so the exe runs on machines without the VC++ redistributable; the FFmpeg DLLs
    // use the system's UCRT.
    let target = "x86_64-pc-windows-msvc";
    let build_dir = cargo_build(
        root,
        ffmpeg_dir,
        target,
        &[(
            "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS",
            "-C target-feature=+crt-static",
        )],
    )?;

    let name = format!("{APP_NAME}-{label}-windows-x64");
    let package_dir = fresh_dir(&dist_dir.join(&name))?;
    copy_into(&build_dir.join(format!("{BINARY}.exe")), &package_dir)?;
    for lib in ffmpeg_runtime_libs(ffmpeg_dir)? {
        copy_into(&lib, &package_dir)?;
    }
    write_notices(root, ffmpeg_dir, &package_dir)?;

    let zip_path = dist_dir.join(format!("{name}.zip"));
    write_zip(&package_dir, &name, &zip_path)?;
    Ok(zip_path)
}

fn macos(root: &Path, ffmpeg_dir: &Path, dist_dir: &Path, label: &str) -> Result<PathBuf> {
    let binaries = ["aarch64-apple-darwin", "x86_64-apple-darwin"]
        .map(|target| cargo_build(root, ffmpeg_dir, target, &[]).map(|d| d.join(BINARY)));

    let name = format!("{APP_NAME}-{label}-macos-universal");
    let staging = fresh_dir(&dist_dir.join(&name))?;
    let app = staging.join(format!("{APP_NAME}.app"));
    let contents = app.join("Contents");
    for dir in ["MacOS", "Frameworks", "Resources"] {
        fs::create_dir_all(contents.join(dir))?;
    }

    let mut lipo = Command::new("lipo");
    lipo.arg("-create");
    for binary in binaries {
        lipo.arg(binary?);
    }
    run(lipo.arg("-output").arg(contents.join("MacOS").join(BINARY)))?;
    // The binary finds these through its `@executable_path/../Frameworks` rpath (.cargo/config.toml).
    for lib in ffmpeg_runtime_libs(ffmpeg_dir)? {
        copy_into(&lib, &contents.join("Frameworks"))?;
    }
    write_notices(root, ffmpeg_dir, &contents.join("Resources"))?;
    write_icns(
        root,
        &staging,
        &contents.join("Resources").join("icon.icns"),
    )?;
    fs::write(contents.join("Info.plist"), info_plist())?;

    // Apple Silicon only runs signed code, and lipo output is unsigned. An ad-hoc signature
    // (no identity) satisfies that; Gatekeeper still asks testers to approve the app once.
    for entry in fs::read_dir(contents.join("Frameworks"))? {
        run(Command::new("codesign")
            .args(["--force", "--sign", "-"])
            .arg(entry?.path()))?;
    }
    run(Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(&app))?;

    let zip_path = dist_dir.join(format!("{name}.zip"));
    if zip_path.exists() {
        fs::remove_file(&zip_path)?;
    }
    // ditto keeps the bundle's symlinks, permissions and signatures intact.
    run(Command::new("ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(&app)
        .arg(&zip_path))?;
    Ok(zip_path)
}

fn linux(root: &Path, ffmpeg_dir: &Path, dist_dir: &Path, label: &str) -> Result<PathBuf> {
    let arch = std::env::consts::ARCH;
    let build_dir = cargo_build(root, ffmpeg_dir, &format!("{arch}-unknown-linux-gnu"), &[])?;

    let name = format!("{APP_NAME}-{label}-linux-{arch}");
    let app_dir = fresh_dir(&dist_dir.join(format!("{name}.AppDir")))?;
    // The FFmpeg libraries sit next to the binary, which finds them through its `$ORIGIN` rpath.
    let bin_dir = app_dir.join("usr/bin");
    fs::create_dir_all(&bin_dir)?;
    copy_into(&build_dir.join(BINARY), &bin_dir)?;
    for lib in ffmpeg_runtime_libs(ffmpeg_dir)? {
        copy_into(&lib, &bin_dir)?;
    }
    let doc_dir = app_dir.join("usr/share/doc").join(BINARY);
    fs::create_dir_all(&doc_dir)?;
    write_notices(root, ffmpeg_dir, &doc_dir)?;

    fs::copy(root.join(ICON), app_dir.join(format!("{BINARY}.png")))?;
    fs::write(
        app_dir.join(format!("{BINARY}.desktop")),
        format!(
            "[Desktop Entry]\nType=Application\nName={APP_NAME}\nExec={BINARY}\nIcon={BINARY}\n\
             Categories=AudioVideo;Video;\nTerminal=false\n"
        ),
    )?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(format!("usr/bin/{BINARY}"), app_dir.join("AppRun"))?;

    let appimage = dist_dir.join(format!("{name}.AppImage"));
    let tool = std::env::var_os("APPIMAGETOOL").unwrap_or_else(|| "appimagetool".into());
    run(Command::new(&tool)
        .env("ARCH", arch)
        .arg(&app_dir)
        .arg(&appimage))
    .context("running appimagetool (https://github.com/AppImage/appimagetool; set APPIMAGETOOL to its path)")?;
    Ok(appimage)
}

/// Builds the app for `target` against the release FFmpeg and returns the output directory.
fn cargo_build(
    root: &Path,
    ffmpeg_dir: &Path,
    target: &str,
    envs: &[(&str, &str)],
) -> Result<PathBuf> {
    run(Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "build",
            "--release",
            "-p",
            "rastersong-gui",
            "--target",
            target,
        ])
        .env("FFMPEG_DIR", ffmpeg_dir)
        .env(
            "MACOSX_DEPLOYMENT_TARGET",
            release_ffmpeg::MACOS_DEPLOYMENT_TARGET,
        )
        .envs(envs.iter().copied()))?;
    Ok(root.join("target").join(target).join("release"))
}

/// The FFmpeg shared libraries the app loads, under the file names it loads them by.
fn ffmpeg_runtime_libs(ffmpeg_dir: &Path) -> Result<Vec<PathBuf>> {
    let lib_dir = ffmpeg_dir.join(if cfg!(windows) { "bin" } else { "lib" });
    let mut libs = Vec::new();
    for entry in fs::read_dir(&lib_dir)? {
        let path = entry?.path();
        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        let lib = if cfg!(windows) {
            // `avcodec-62.dll` -> `avcodec`
            file_name
                .strip_suffix(".dll")
                .and_then(|stem| stem.rsplit_once('-'))
                .map(|(lib, _)| lib)
        } else if cfg!(target_os = "macos") {
            // `libavcodec.62.dylib` (the install name; `libavcodec.dylib` is a symlink to it)
            file_name
                .strip_suffix(".dylib")
                .and_then(|stem| stem.strip_prefix("lib"))
                .and_then(|stem| stem.split_once('.'))
                .map(|(lib, _)| lib)
        } else {
            // `libavcodec.so.62` (the SONAME; not the dev symlink or the fully versioned name)
            file_name
                .split_once(".so.")
                .filter(|(_, version)| !version.contains('.'))
                .and_then(|(stem, _)| stem.strip_prefix("lib"))
        };
        if lib.is_some_and(|lib| LIBS.contains(&lib)) {
            libs.push(path);
        }
    }
    if libs.len() != LIBS.len() {
        bail!(
            "expected the {} FFmpeg libraries {LIBS:?} in {}, found {libs:?}",
            LIBS.len(),
            lib_dir.display()
        );
    }
    Ok(libs)
}

/// RasterSong's license, FFmpeg's license and build information, and the Rust dependencies'
/// notices.
fn write_notices(root: &Path, ffmpeg_dir: &Path, dest: &Path) -> Result<()> {
    fs::copy(root.join("LICENSE"), dest.join("LICENSE.txt"))?;
    fs::copy(
        ffmpeg_dir.join(release_ffmpeg::LICENSE_FILE),
        dest.join("FFmpeg-LICENSE.txt"),
    )?;
    copy_into(&ffmpeg_dir.join(release_ffmpeg::BUILD_INFO_FILE), dest)?;
    run(Command::new(env!("CARGO"))
        .current_dir(root)
        .args(["about", "generate", "--manifest-path", "crates/rastersong-gui/Cargo.toml", "-o"])
        .arg(dest.join("THIRD-PARTY-NOTICES.html"))
        .arg("about.hbs"))
    .context("generating third-party notices (install with `cargo install cargo-about --locked --features cli`)")
}

fn write_icns(root: &Path, work_dir: &Path, dest: &Path) -> Result<()> {
    let iconset = work_dir.join("icon.iconset");
    fs::create_dir_all(&iconset)?;
    for size in [16, 32, 128, 256] {
        for (scale, suffix) in [(1, ""), (2, "@2x")] {
            let pixels = (size * scale).to_string();
            run(Command::new("sips")
                .args(["-z", &pixels, &pixels])
                .arg(root.join(ICON))
                .arg("--out")
                .arg(iconset.join(format!("icon_{size}x{size}{suffix}.png")))
                .stdout(std::process::Stdio::null()))?;
        }
    }
    run(Command::new("iconutil")
        .args(["-c", "icns", "-o"])
        .arg(dest)
        .arg(&iconset))?;
    fs::remove_dir_all(&iconset)?;
    Ok(())
}

fn info_plist() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let min_os = release_ffmpeg::MACOS_DEPLOYMENT_TARGET;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>{APP_NAME}</string>
    <key>CFBundleDisplayName</key><string>{APP_NAME}</string>
    <key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>
    <key>CFBundleExecutable</key><string>{BINARY}</string>
    <key>CFBundleIconFile</key><string>icon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>{version}</string>
    <key>CFBundleVersion</key><string>{version}</string>
    <key>LSMinimumSystemVersion</key><string>{min_os}</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#
    )
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

/// Creates `dir`, emptying it first if it exists.
fn fresh_dir(dir: &Path) -> Result<PathBuf> {
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::create_dir_all(dir)?;
    Ok(dir.to_path_buf())
}

/// Copies `src` into `dest_dir`, following symlinks.
fn copy_into(src: &Path, dest_dir: &Path) -> Result<()> {
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
