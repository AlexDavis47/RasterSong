//! `cargo xtask fetch-ffmpeg`: installs the pinned LGPL FFmpeg into `third_party/ffmpeg/`.
//!
//! Windows and Linux use prebuilt LGPL shared builds from BtbN/FFmpeg-Builds. Only month-end
//! autobuilds are pinned, because BtbN keeps those long-term and prunes the rest. macOS has no
//! trustworthy prebuilt LGPL shared build, so it is built from the official source release.
//!
//! The FFmpeg major version must match the `ffmpeg-next` major version in the workspace Cargo.toml.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::util::{download, extract, run, single_subdir, workspace_root};

/// Official source release. Used for macOS builds, and the version all platforms are based on.
const SOURCE_VERSION: &str = "9.0.2";
const SOURCE_SHA256: &str = "8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e";

const BTBN_RELEASE: &str = "autobuild-2026-09-30-13-08";
const BTBN_BUILD: &str = "n9.0.2-17-g2a571b6068";

struct Prebuilt {
    os: &'static str,
    arch: &'static str,
    platform: &'static str,
    extension: &'static str,
    sha256: &'static str,
}

const PREBUILT: &[Prebuilt] = &[
    Prebuilt {
        os: "windows",
        arch: "x86_64",
        platform: "win64",
        extension: "zip",
        sha256: "7157177b8a6cb2174c1650ba8c71b363f2c78cba5330f88c4c02cf5b2b880646",
    },
    Prebuilt {
        os: "windows",
        arch: "aarch64",
        platform: "winarm64",
        extension: "zip",
        sha256: "f3133abf5c2b6ae45bd428d480cf830a9fbda3a3866dc249110e85f99f13b8ce",
    },
    Prebuilt {
        os: "linux",
        arch: "x86_64",
        platform: "linux64",
        extension: "tar.xz",
        sha256: "7a1ba2af36e5beeb98151e766a903b1d115f2206a86571f17f873cd7292c424f",
    },
    Prebuilt {
        os: "linux",
        arch: "aarch64",
        platform: "linuxarm64",
        extension: "tar.xz",
        sha256: "1aa02c59afe06dea2602b40517a5cb75c0ff6d524f217b5aea1f0ca07977cd51",
    },
];

/// Written into the install directory so re-running is a no-op until the pin changes.
const STAMP_FILE: &str = ".rastersong-pin";

pub fn fetch(force: bool) -> Result<()> {
    let third_party = workspace_root().join("third_party");
    let install_dir = third_party.join("ffmpeg");
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);

    let prebuilt = PREBUILT.iter().find(|p| p.os == os && p.arch == arch);
    let pin = match prebuilt {
        Some(p) => format!("btbn {BTBN_RELEASE} {BTBN_BUILD} {}", p.platform),
        None => format!("source {SOURCE_VERSION}"),
    };
    let stamp = install_dir.join(STAMP_FILE);
    if !force && fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == pin) {
        println!("FFmpeg is up to date in {} ({pin})", install_dir.display());
        return Ok(());
    }

    let work_dir = third_party.join(".download");
    if work_dir.exists() {
        fs::remove_dir_all(&work_dir)?;
    }
    fs::create_dir_all(&work_dir)?;
    if install_dir.exists() {
        fs::remove_dir_all(&install_dir).context("removing the previous FFmpeg install")?;
    }

    match (prebuilt, os) {
        (Some(p), _) => install_prebuilt(p, &work_dir, &install_dir)?,
        (None, "macos") => build_from_source(&work_dir, &install_dir)?,
        (None, _) => {
            bail!(
                "no pinned FFmpeg for {os}/{arch}; set FFMPEG_DIR to an LGPL FFmpeg {SOURCE_VERSION} build"
            )
        }
    }

    fs::write(&stamp, &pin)?;
    fs::remove_dir_all(&work_dir).ok();
    println!("FFmpeg installed in {} ({pin})", install_dir.display());
    Ok(())
}

fn install_prebuilt(p: &Prebuilt, work_dir: &Path, install_dir: &Path) -> Result<()> {
    let major_minor = SOURCE_VERSION.rsplit_once('.').unwrap().0;
    let file = format!(
        "ffmpeg-{BTBN_BUILD}-{}-lgpl-shared-{major_minor}.{}",
        p.platform, p.extension
    );
    let url =
        format!("https://github.com/BtbN/FFmpeg-Builds/releases/download/{BTBN_RELEASE}/{file}");
    let archive = work_dir.join(&file);
    download(&url, &archive, p.sha256)?;

    let unpacked = work_dir.join("unpacked");
    extract(&archive, &unpacked)?;
    fs::rename(single_subdir(&unpacked)?, install_dir)?;
    Ok(())
}

fn build_from_source(work_dir: &Path, install_dir: &Path) -> Result<()> {
    let file = format!("ffmpeg-{SOURCE_VERSION}.tar.xz");
    let archive = work_dir.join(&file);
    download(
        &format!("https://ffmpeg.org/releases/{file}"),
        &archive,
        SOURCE_SHA256,
    )?;

    let unpacked = work_dir.join("unpacked");
    extract(&archive, &unpacked)?;
    let source = single_subdir(&unpacked)?;

    // LGPL configuration: never add --enable-gpl or --enable-nonfree here.
    let mut configure = Command::new("./configure");
    configure
        .current_dir(&source)
        .arg(format!("--prefix={}", install_dir.display()))
        .args([
            "--enable-shared",
            "--disable-static",
            "--disable-doc",
            "--disable-ffplay",
            "--disable-debug",
        ]);
    if Command::new("nasm").arg("-v").output().is_err() {
        configure.arg("--disable-x86asm");
    }
    println!("Configuring FFmpeg {SOURCE_VERSION} (this build takes several minutes)");
    run(&mut configure)?;

    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    run(Command::new("make")
        .current_dir(&source)
        .arg(format!("-j{jobs}")))?;
    run(Command::new("make").current_dir(&source).arg("install"))?;
    Ok(())
}
