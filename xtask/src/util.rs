use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};

/// The workspace root (the parent of `xtask/`).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Runs a command, failing with its full command line if it exits unsuccessfully.
pub fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("failed to start {cmd:?}"))?;
    ensure!(status.success(), "{cmd:?} failed with {status}");
    Ok(())
}

/// Downloads `url` to `dest` and checks its SHA-256.
pub fn download(url: &str, dest: &Path, sha256: &str) -> Result<()> {
    // Windows ships curl.exe in System32. Name it explicitly so a different curl earlier in PATH
    // (e.g. from Git or MSYS) can't change behavior.
    let curl = if cfg!(windows) {
        r"C:\Windows\System32\curl.exe"
    } else {
        "curl"
    };
    println!("Downloading {url}");
    run(Command::new(curl)
        .args([
            "--fail",
            "--location",
            "--retry",
            "3",
            "--progress-bar",
            "--output",
        ])
        .arg(dest)
        .arg(url))?;

    let actual = sha256_file(dest)?;
    if !actual.eq_ignore_ascii_case(sha256) {
        fs::remove_file(dest).ok();
        bail!("checksum mismatch for {url}\n  expected {sha256}\n  actual   {actual}");
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    io::copy(&mut BufReader::new(File::open(path)?), &mut hasher)?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Extracts a `.zip` or `.tar.xz` archive into `dest`.
pub fn extract(archive: &Path, dest: &Path) -> Result<()> {
    println!("Extracting {}", archive.display());
    fs::create_dir_all(dest)?;
    let name = archive.file_name().unwrap().to_string_lossy();
    if name.ends_with(".zip") {
        zip::ZipArchive::new(File::open(archive)?)?.extract(dest)?;
    } else if name.ends_with(".tar.xz") {
        let tar_path = archive.with_extension("");
        {
            let mut tar_file = BufWriter::new(File::create(&tar_path)?);
            lzma_rs::xz_decompress(&mut BufReader::new(File::open(archive)?), &mut tar_file)
                .map_err(|e| anyhow::anyhow!("decompressing {name}: {e:?}"))?;
        }
        tar::Archive::new(File::open(&tar_path)?).unpack(dest)?;
        fs::remove_file(&tar_path)?;
    } else {
        bail!("unsupported archive format: {name}");
    }
    Ok(())
}

/// Copies the directory `src` to `dest` recursively.
pub fn copy_dir(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)
                .with_context(|| format!("copying {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Returns the single directory an archive extracted into.
pub fn single_subdir(dir: &Path) -> Result<PathBuf> {
    let mut dirs = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path());
    match (dirs.next(), dirs.next()) {
        (Some(only), None) => Ok(only),
        _ => bail!(
            "expected exactly one top-level directory in {}",
            dir.display()
        ),
    }
}
