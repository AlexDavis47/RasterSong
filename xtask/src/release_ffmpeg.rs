//! `cargo xtask build-ffmpeg`: builds the FFmpeg that release packages ship into
//! `third_party/ffmpeg-release/`.
//!
//! The development FFmpeg (`fetch-ffmpeg`) is a broad third-party build carrying dozens of external
//! libraries, each with its own license and source obligations. This one is built from the official
//! source release on every platform with a minimal LGPL v2.1-or-later configuration: no external
//! libraries except zlib, no programs, no avdevice or avfilter, and only the encoders the app uses.
//! The package then needs one license, one source tarball and one configure line per platform.
//!
//! Windows builds with MSYS2's UCRT64 toolchain. macOS builds an arm64 and an x86_64 slice and
//! merges them into universal libraries.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};

use crate::ffmpeg::{SOURCE_VERSION, fetch_source, source_url};
use crate::util::{copy_dir, run, workspace_root};

/// The FFmpeg libraries the app loads: those enabled by the `ffmpeg-next` features in the workspace
/// Cargo.toml, plus avutil.
pub const LIBS: &[&str] = &["avcodec", "avformat", "avutil", "swresample", "swscale"];

/// Every encoder the app uses. Add an encoder here when an export format needs it.
const ENCODERS: &[&str] = &[
    "ffv1",
    "prores_ks",
    "pcm_s16le",
    "pcm_s24le",
    "pcm_f32le",
    "aac",
];

/// Whether to include FFmpeg's HEVC decoder. HEVC is covered by patent pools, separately from
/// FFmpeg's copyright license; whether to keep shipping it is to be settled before a paid release.
const HEVC_DECODER: bool = true;

pub const MACOS_DEPLOYMENT_TARGET: &str = "11.0";

/// The build information and license notice shipped next to the libraries.
pub const BUILD_INFO_FILE: &str = "FFMPEG-BUILD.txt";
pub const LICENSE_FILE: &str = "LICENSE.txt";

/// Written into the install directory so re-running is a no-op until the configuration changes.
const STAMP_FILE: &str = ".rastersong-release-pin";

pub fn install_dir() -> PathBuf {
    workspace_root().join("third_party").join("ffmpeg-release")
}

/// The source tarball the release libraries were built from, kept for publishing with releases.
pub fn source_archive() -> PathBuf {
    install_dir().join(format!("ffmpeg-{SOURCE_VERSION}.tar.xz"))
}

/// One build of FFmpeg: the host's, or one architecture of a universal macOS build.
#[derive(Debug, Clone, Copy)]
enum Slice {
    Host,
    Macos {
        arch: &'static str,
        clang_arch: &'static str,
    },
}

fn slices() -> Vec<Slice> {
    if cfg!(target_os = "macos") {
        vec![
            Slice::Macos {
                arch: "aarch64",
                clang_arch: "arm64",
            },
            Slice::Macos {
                arch: "x86_64",
                clang_arch: "x86_64",
            },
        ]
    } else {
        vec![Slice::Host]
    }
}

pub fn build(force: bool) -> Result<PathBuf> {
    let install_dir = install_dir();
    let third_party = workspace_root().join("third_party");
    let work_dir = third_party.join(".release-build");

    // The configure lines identify the build. On Windows they contain the work directory, so
    // compute them against its final path.
    let lines: Vec<String> = slices()
        .iter()
        .map(|&slice| configure_args(slice, &work_dir).join(" "))
        .collect();
    let pin = format!("{SOURCE_VERSION}\n{}", lines.join("\n"));
    let stamp = install_dir.join(STAMP_FILE);
    if !force && fs::read_to_string(&stamp).is_ok_and(|s| s == pin) {
        println!("Release FFmpeg is up to date in {}", install_dir.display());
        return Ok(install_dir);
    }

    if work_dir.exists() {
        fs::remove_dir_all(&work_dir)?;
    }
    fs::create_dir_all(&work_dir)?;
    if install_dir.exists() {
        fs::remove_dir_all(&install_dir).context("removing the previous release FFmpeg")?;
    }

    let (archive, source) = fetch_source(&work_dir)?;
    if cfg!(windows) {
        prepare_static_libs(&work_dir)?;
    }

    let slice_dirs: Vec<PathBuf> = slices()
        .into_iter()
        .map(|slice| {
            let prefix = match slice {
                Slice::Host => install_dir.clone(),
                Slice::Macos { arch, .. } => work_dir.join(format!("slice-{arch}")),
            };
            build_slice(slice, &source, &work_dir, &prefix)?;
            check_dependencies(&prefix)?;
            Ok(prefix)
        })
        .collect::<Result<_>>()?;
    if cfg!(target_os = "macos") {
        merge_universal(&slice_dirs, &install_dir)?;
    }
    if cfg!(windows) {
        // FFmpeg installs the MSVC import libraries next to the DLLs; ffmpeg-sys-next looks in lib/.
        for entry in fs::read_dir(install_dir.join("bin"))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "lib") {
                fs::rename(
                    &path,
                    install_dir.join("lib").join(path.file_name().unwrap()),
                )?;
            }
        }
    }

    fs::copy(
        source.join("COPYING.LGPLv2.1"),
        install_dir.join(LICENSE_FILE),
    )?;
    fs::write(
        install_dir.join(BUILD_INFO_FILE),
        build_info(&lines, &work_dir)?,
    )?;
    fs::rename(&archive, source_archive())?;
    fs::write(&stamp, &pin)?;
    fs::remove_dir_all(&work_dir).ok();
    println!("Release FFmpeg installed in {}", install_dir.display());
    Ok(install_dir)
}

fn configure_args(slice: Slice, work_dir: &Path) -> Vec<String> {
    let mut args: Vec<String> = [
        // Only what is enabled below; never --enable-gpl, --enable-version3 or --enable-nonfree.
        "--disable-autodetect",
        "--disable-programs",
        "--disable-doc",
        "--disable-debug",
        "--enable-shared",
        "--disable-static",
        "--disable-network",
        "--disable-avdevice",
        "--disable-avfilter",
        "--disable-encoders",
        "--enable-zlib",
    ]
    .map(String::from)
    .into();
    args.push(format!("--enable-encoder={}", ENCODERS.join(",")));
    if !HEVC_DECODER {
        args.extend(["--disable-decoder=hevc", "--disable-parser=hevc"].map(String::from));
    }
    match slice {
        Slice::Host if cfg!(windows) => {
            // The DLLs must only depend on system libraries, not on MinGW's runtime DLLs.
            let static_libs = shell_path(&work_dir.join("static"));
            args.extend([
                "--enable-w32threads".into(),
                format!("--extra-cflags=-I{static_libs}/include"),
                format!("--extra-ldflags=-static-libgcc -L{static_libs}/lib"),
            ]);
        }
        Slice::Host => args.push("--enable-pthreads".into()),
        Slice::Macos { arch, clang_arch } => args.extend([
            "--enable-pthreads".into(),
            "--enable-cross-compile".into(),
            "--target-os=darwin".into(),
            format!("--arch={arch}"),
            format!("--cc=clang -arch {clang_arch}"),
            // Libraries are found through the app's rpath (Contents/Frameworks in the bundle).
            "--install-name-dir=@rpath".into(),
        ]),
    }
    args
}

fn build_slice(slice: Slice, source: &Path, work_dir: &Path, prefix: &Path) -> Result<()> {
    if let Slice::Macos { clang_arch, .. } = slice {
        println!("Building the {clang_arch} slice");
    }
    // Build out of tree so slices don't share objects.
    let build_dir = work_dir.join(format!(
        "build-{}",
        prefix.file_name().unwrap().to_string_lossy()
    ));
    fs::create_dir_all(&build_dir)?;

    let mut args = vec![format!("--prefix={}", shell_path(prefix))];
    args.extend(configure_args(slice, work_dir));
    let configure = format!(
        "{}/configure {}",
        shell_path(source),
        args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")
    );
    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    let script = format!("{configure} && make -j{jobs} && make install");

    println!("Configuring FFmpeg {SOURCE_VERSION} (this build takes several minutes)");
    let mut cmd = shell(&script);
    cmd.current_dir(&build_dir)
        .env("MACOSX_DEPLOYMENT_TARGET", MACOS_DEPLOYMENT_TARGET);
    run(&mut cmd)
}

/// Libraries from MSYS2 that the Windows DLLs link statically, with their license files: zlib, and
/// winpthreads, which avutil needs for its POSIX time functions.
const WINDOWS_STATIC_LIBS: &[(&str, &[&str], &str)] = &[
    ("libz.a", &["libz.a"], "zlib/LICENSE"),
    (
        "libwinpthread.a",
        &["libwinpthread.a", "libpthread.a"],
        "winpthreads/COPYING",
    ),
];

/// MSYS2 provides these libraries as both static and import libraries, and the linker prefers the
/// import library. Copy only the static ones into a directory searched first, so they end up inside
/// the FFmpeg DLLs.
fn prepare_static_libs(work_dir: &Path) -> Result<()> {
    let ucrt = msys2_root().join("ucrt64");
    let dest = work_dir.join("static");
    fs::create_dir_all(dest.join("include"))?;
    fs::create_dir_all(dest.join("lib"))?;
    for header in ["zlib.h", "zconf.h"] {
        fs::copy(
            ucrt.join("include").join(header),
            dest.join("include").join(header),
        )
        .with_context(|| format!("copying {header} (install mingw-w64-ucrt-x86_64-zlib)"))?;
    }
    for (lib, names, _) in WINDOWS_STATIC_LIBS {
        for name in *names {
            fs::copy(ucrt.join("lib").join(lib), dest.join("lib").join(name))
                .with_context(|| format!("copying {lib} from MSYS2"))?;
        }
    }
    Ok(())
}

/// Fails if the built libraries link anything other than each other and system libraries, which
/// would mean a dependency the package doesn't ship (or license) slipped in.
fn check_dependencies(prefix: &Path) -> Result<()> {
    let mut bad = Vec::new();
    if cfg!(windows) {
        for path in files_with_extension(&prefix.join("bin"), "dll")? {
            let out = output(&mut shell(&format!(
                "objdump -p {}",
                quote(&shell_path(&path))
            )))?;
            for dll in out
                .lines()
                .filter_map(|l| l.trim().strip_prefix("DLL Name: "))
            {
                let lower = dll.to_ascii_lowercase();
                let ours = LIBS.iter().any(|lib| lower.starts_with(&format!("{lib}-")));
                if !ours && (lower.starts_with("lib") || lower.starts_with("zlib")) {
                    bad.push(format!("{} needs {dll}", path.display()));
                }
            }
        }
    } else if cfg!(target_os = "macos") {
        for lib in LIBS {
            let path = prefix.join(format!("lib/lib{lib}.dylib"));
            let out = output(Command::new("otool").arg("-L").arg(&path))?;
            for dep in out
                .lines()
                .skip(1)
                .filter_map(|l| l.split_whitespace().next())
            {
                if !["/usr/lib/", "/System/", "@rpath/"]
                    .iter()
                    .any(|p| dep.starts_with(p))
                {
                    bad.push(format!("{} needs {dep}", path.display()));
                }
            }
        }
    } else {
        const SYSTEM: &[&str] = &[
            "libc.so",
            "libm.so",
            "libz.so",
            "libpthread.so",
            "libdl.so",
            "ld-linux",
        ];
        for lib in LIBS {
            let path = prefix.join(format!("lib/lib{lib}.so"));
            let out = output(Command::new("readelf").arg("-d").arg(&path))?;
            for dep in out.lines().filter(|l| l.contains("(NEEDED)")) {
                let name = dep.split('[').nth(1).unwrap_or(dep).trim_end_matches(']');
                let ours = LIBS.iter().any(|l| name.starts_with(&format!("lib{l}.so")));
                if !ours && !SYSTEM.iter().any(|s| name.starts_with(s)) {
                    bad.push(format!("{} needs {name}", path.display()));
                }
            }
        }
    }
    ensure!(
        bad.is_empty(),
        "unexpected FFmpeg dependencies:\n  {}",
        bad.join("\n  ")
    );
    Ok(())
}

/// Merges the per-architecture builds into universal libraries in `dest`.
fn merge_universal(slices: &[PathBuf], dest: &Path) -> Result<()> {
    copy_dir(&slices[0].join("include"), &dest.join("include"))?;
    fs::create_dir_all(dest.join("lib"))?;
    for lib in LIBS {
        let link_name = format!("lib{lib}.dylib");
        let first = slices[0].join("lib").join(&link_name);
        // The install name (`@rpath/libavcodec.62.dylib`) is the file name the app loads.
        let install_name = output(Command::new("otool").arg("-D").arg(&first))?;
        let file_name = install_name
            .lines()
            .last()
            .and_then(|l| l.trim().strip_prefix("@rpath/"))
            .with_context(|| format!("unexpected install name for {link_name}: {install_name}"))?
            .to_owned();
        let mut lipo = Command::new("lipo");
        lipo.arg("-create");
        for slice in slices {
            lipo.arg(slice.join("lib").join(&link_name));
        }
        run(lipo.arg("-output").arg(dest.join("lib").join(&file_name)))?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&file_name, dest.join("lib").join(&link_name))?;
    }
    Ok(())
}

fn build_info(configure_lines: &[String], work_dir: &Path) -> Result<String> {
    let platform = match std::env::consts::OS {
        "macos" => "macOS (universal: arm64 and x86_64 built separately and merged)",
        "windows" => "Windows (MSYS2 UCRT64 toolchain)",
        _ => "Linux",
    };
    let mut info = format!(
        "RasterSong uses libraries from the FFmpeg project (https://ffmpeg.org) under the GNU Lesser\n\
         General Public License, version 2.1 or later. The license text is in FFmpeg-LICENSE.txt.\n\
         RasterSong does not own FFmpeg; its copyright belongs to the FFmpeg developers.\n\
         \n\
         The libraries were built, unmodified, from the official FFmpeg {SOURCE_VERSION} source release:\n\
         {}\n\
         which is also published alongside every RasterSong release that ships these libraries.\n\
         \n\
         Platform: {platform}\n\
         Configuration (relative to the FFmpeg source directory):\n",
        source_url()
    );
    for line in configure_lines {
        let line = line.replace(&shell_path(work_dir), "<build dir>");
        info.push_str(&format!("  ./configure --prefix=<install dir> {line}\n"));
    }
    info.push_str(
        "\nThe libraries are loaded dynamically and may be replaced with your own build of the same\n\
         FFmpeg major version: replace the av*/sw* library files that ship with RasterSong.\n",
    );
    if cfg!(windows) {
        info.push_str(
            "\nThe DLLs include these libraries from MSYS2, linked statically, under their own licenses:\n",
        );
        let licenses = msys2_root().join("ucrt64/share/licenses");
        for (lib, _, license) in WINDOWS_STATIC_LIBS {
            let text = fs::read_to_string(licenses.join(license))
                .with_context(|| format!("reading the license of {lib}"))?;
            info.push_str(&format!("\n----- {lib} ({license}) -----\n\n{text}"));
        }
    }
    Ok(info)
}

/// A shell command line run by `sh` (MSYS2's bash on Windows, with the UCRT64 toolchain).
fn shell(script: &str) -> Command {
    if cfg!(windows) {
        let mut cmd = Command::new(msys2_root().join("usr/bin/bash.exe"));
        cmd.args(["-lc", script])
            .env("MSYSTEM", "UCRT64")
            // Stay in the current directory and keep the Windows PATH (for nasm, if installed there).
            .env("CHERE_INVOKING", "1")
            .env("MSYS2_PATH_TYPE", "inherit");
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]);
        cmd
    }
}

fn msys2_root() -> PathBuf {
    std::env::var_os("MSYS2_ROOT").map_or_else(|| PathBuf::from(r"C:\msys64"), PathBuf::from)
}

/// A path as the shell sees it: `C:\a\b` becomes `/c/a/b` under MSYS2.
fn shell_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows)
        && let Some((drive, rest)) = s.split_once(":/")
    {
        return format!("/{}/{rest}", drive.to_ascii_lowercase());
    }
    s
}

fn quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

fn output(cmd: &mut Command) -> Result<String> {
    let out = cmd
        .output()
        .with_context(|| format!("failed to start {cmd:?}"))?;
    if !out.status.success() {
        bail!(
            "{cmd:?} failed with {}:\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn files_with_extension(dir: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == extension) {
            files.push(path);
        }
    }
    Ok(files)
}
