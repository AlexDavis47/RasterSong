//! Decoded audio kept on disk: each audio stream is decoded once into an uncompressed `f32` file
//! in the cache directory and read back through a memory map, so a project's audio costs address
//! space rather than memory, and opening it again costs no decoding.
//!
//! Files are named by the content hash of the source (and the decoding options), so a renamed or
//! copied source finds its cache file, and a changed one gets a new file. A missing cache file is
//! simply decoded again; the cache can be cleared at any time when nothing has it open.

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};

use crate::{AudioClip, AudioOptions, MediaBackend, MediaError, Samples};

/// Start of every cache file.
const MAGIC: &[u8; 8] = b"RSAUDIO\0";
/// Bumped whenever the file layout or the decoding changes, which orphans older files.
const VERSION: u32 = 1;
/// Written natively, so a file from a machine of the other endianness reads as a mismatch.
const ENDIAN_MARK: u32 = 0x0102_0304;
/// Bytes before the samples: magic, version, endian mark, rate, channels, reserved, sample count.
const HEADER_LEN: usize = 32;

/// The directory of decoded audio, and how to fill it.
#[derive(Debug, Clone)]
pub struct AudioCache {
    dir: PathBuf,
}

impl AudioCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The cache under the user's cache directory (`%LOCALAPPDATA%\RasterSong\cache` on Windows,
    /// `~/Library/Caches/RasterSong` on macOS, `$XDG_CACHE_HOME/rastersong` or
    /// `~/.cache/rastersong` elsewhere), or `RASTERSONG_CACHE_DIR` when set. `None` when no such
    /// directory can be found.
    pub fn in_user_dir() -> Option<Self> {
        user_cache_dir().map(Self::new)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The audio of `path`, decoded to the cache the first time and memory-mapped from it. If
    /// the cache can't be written or read, the audio is decoded into memory instead (and a
    /// warning logged), so a full or read-only disk costs memory, not the audio.
    pub fn load(
        &self,
        backend: &dyn MediaBackend,
        path: &Path,
        options: AudioOptions,
    ) -> Result<AudioClip, MediaError> {
        match self.load_mapped(backend, path, options) {
            Ok(clip) => Ok(clip),
            Err(CacheError::Media(e)) => Err(e),
            Err(CacheError::Io(e)) => {
                tracing::warn!(
                    path = %path.display(),
                    cache = %self.dir.display(),
                    "audio cache unavailable, decoding into memory: {e}"
                );
                backend.load_audio(path, options)
            }
        }
    }

    /// Deletes every cache file. Clips mapped from them stay readable until dropped on systems
    /// that allow it; on Windows, files still mapped are left in place.
    pub fn clear(&self) -> io::Result<()> {
        for sub in ["audio", "hashes"] {
            let Ok(entries) = fs::read_dir(self.dir.join(sub)) else {
                continue;
            };
            for entry in entries.flatten() {
                if let Err(e) = fs::remove_file(entry.path()) {
                    tracing::debug!(path = %entry.path().display(), "not removed: {e}");
                }
            }
        }
        Ok(())
    }

    /// The cache file for `path`'s audio with `options`.
    pub fn file_for(&self, path: &Path, options: AudioOptions) -> io::Result<PathBuf> {
        let content = self.content_hash(path)?;
        let mut key = Sha256::new();
        key.update(b"rastersong audio");
        key.update(VERSION.to_le_bytes());
        key.update(content);
        key.update(options.sample_rate.unwrap_or(0).to_le_bytes());
        key.update(options.channels.unwrap_or(0).to_le_bytes());
        // Left out for the best stream, so its files keep their names.
        if let Some(stream) = options.stream {
            key.update(b"stream");
            key.update((stream as u64).to_le_bytes());
        }
        Ok(self
            .dir
            .join("audio")
            .join(format!("{}.f32", hex(&key.finalize()))))
    }

    fn load_mapped(
        &self,
        backend: &dyn MediaBackend,
        path: &Path,
        options: AudioOptions,
    ) -> Result<AudioClip, CacheError> {
        let file = self.file_for(path, options)?;
        match open(&file) {
            Ok(clip) => return Ok(clip),
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                tracing::debug!(file = %file.display(), "rebuilding cache file: {e}");
            }
            Err(_) => {}
        }
        write(backend, path, options, &file)?;
        Ok(open(&file)?)
    }

    /// SHA-256 of the file's bytes. Hashing a long video takes a while, so the hash is remembered
    /// against the file's path, size and modified time, and only recomputed when one changes.
    fn content_hash(&self, path: &Path) -> io::Result<[u8; 32]> {
        let meta = fs::metadata(path)?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        let stamp = format!("{} {modified}", meta.len());
        let absolute = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        let memo = self
            .dir
            .join("hashes")
            .join(hex(&Sha256::digest(absolute.to_string_lossy().as_bytes())));
        if let Ok(text) = fs::read_to_string(&memo)
            && let Some((saved, hash)) = text.trim().rsplit_once(' ')
            && saved == stamp
            && let Some(hash) = unhex(hash)
        {
            return Ok(hash);
        }
        let mut hasher = Sha256::new();
        let mut file = File::open(path)?;
        let mut buf = vec![0; 1 << 20];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let hash: [u8; 32] = hasher.finalize().into();
        // Only a convenience: a memo that can't be written means hashing again next time.
        let _ = fs::create_dir_all(memo.parent().expect("has a parent"))
            .and_then(|()| write_atomically(&memo, format!("{stamp} {}", hex(&hash)).as_bytes()));
        Ok(hash)
    }
}

enum CacheError {
    /// The source can't be decoded: no cache would help.
    Media(MediaError),
    /// The cache itself failed.
    Io(io::Error),
}

impl From<io::Error> for CacheError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Decodes `path` into the cache file `file`, written under a temporary name and renamed into
/// place, so a cache file is either complete or absent.
fn write(
    backend: &dyn MediaBackend,
    path: &Path,
    options: AudioOptions,
    file: &Path,
) -> Result<(), CacheError> {
    let dir = file.parent().expect("cache files are in a directory");
    fs::create_dir_all(dir)?;
    let temp = temporary(file);
    let result = (|| {
        let mut out = BufWriter::new(File::create(&temp)?);
        out.write_all(&[0; HEADER_LEN])?;
        let mut count = 0u64;
        let mut io_error = None;
        let decoded = backend.decode_audio(path, options, &mut |chunk| {
            let bytes: Vec<u8> = chunk.iter().flat_map(|x| x.to_ne_bytes()).collect();
            out.write_all(&bytes).map_err(|e| {
                io_error = Some(e);
                MediaError::Decode(String::new())
            })?;
            count += chunk.len() as u64;
            Ok(())
        });
        if let Some(e) = io_error {
            return Err(CacheError::Io(e));
        }
        let (rate, channels) = decoded.map_err(CacheError::Media)?;
        out.seek(SeekFrom::Start(0))?;
        out.write_all(&header(rate, channels, count))?;
        out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    if let Err(e) = fs::rename(&temp, file) {
        let _ = fs::remove_file(&temp);
        // Another process finished the same file first (Windows refuses to replace it).
        if !file.exists() {
            return Err(e.into());
        }
    }
    Ok(())
}

fn header(rate: u32, channels: u32, count: u64) -> [u8; HEADER_LEN] {
    let mut h = [0; HEADER_LEN];
    h[..8].copy_from_slice(MAGIC);
    h[8..12].copy_from_slice(&VERSION.to_ne_bytes());
    h[12..16].copy_from_slice(&ENDIAN_MARK.to_ne_bytes());
    h[16..20].copy_from_slice(&rate.to_ne_bytes());
    h[20..24].copy_from_slice(&channels.to_ne_bytes());
    h[24..32].copy_from_slice(&count.to_ne_bytes());
    h
}

/// Maps a cache file, checking its header.
fn open(file: &Path) -> io::Result<AudioClip> {
    let invalid = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_owned());
    let mut f = File::open(file)?;
    let mut h = [0; HEADER_LEN];
    f.read_exact(&mut h)?;
    let u32_at = |i: usize| u32::from_ne_bytes(h[i..i + 4].try_into().expect("4 bytes"));
    if &h[..8] != MAGIC || u32_at(8) != VERSION || u32_at(12) != ENDIAN_MARK {
        return Err(invalid("not a cache file of this version"));
    }
    let (rate, channels) = (u32_at(16), u32_at(20));
    let count = u64::from_ne_bytes(h[24..32].try_into().expect("8 bytes"));
    let count = usize::try_from(count).map_err(|_| invalid("too long"))?;
    if rate == 0 || channels == 0 {
        return Err(invalid("no rate or channels"));
    }
    Ok(AudioClip {
        sample_rate: rate,
        channels,
        samples: Samples::map(&f, HEADER_LEN, count)?,
    })
}

/// A name next to `file` that no other writer uses.
fn temporary(file: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = file.file_name().unwrap_or_default().to_owned();
    name.push(format!(".{}-{n}.tmp", std::process::id()));
    file.with_file_name(name)
}

fn write_atomically(file: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = temporary(file);
    fs::write(&temp, bytes)?;
    fs::rename(&temp, file).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    let mut out = [0; 32];
    if text.len() != 64 {
        return None;
    }
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

fn user_cache_dir() -> Option<PathBuf> {
    use std::env::var_os;
    if let Some(dir) = var_os("RASTERSONG_CACHE_DIR").filter(|d| !d.is_empty()) {
        return Some(dir.into());
    }
    let home = || var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        var_os("LOCALAPPDATA")
            .filter(|d| !d.is_empty())
            .map(|d| PathBuf::from(d).join("RasterSong").join("cache"))
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library").join("Caches").join("RasterSong"))
    } else {
        var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|d| d.is_absolute())
            .or_else(|| home().map(|h| h.join(".cache")))
            .map(|d| d.join("rastersong"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FakeBackend;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("rastersong-cache-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn clip() -> AudioClip {
        AudioClip {
            sample_rate: 8,
            channels: 2,
            samples: vec![0.0, 1.0, -0.5, 0.25, 0.75, -1.0].into(),
        }
    }

    #[test]
    fn audio_is_decoded_once_and_mapped() {
        let temp = TempDir::new("once");
        let source = temp.0.join("song.wav");
        fs::write(&source, b"some audio").unwrap();
        let backend = FakeBackend::new().with_audio(&source, clip());
        let cache = AudioCache::new(temp.0.join("cache"));

        let loaded = cache
            .load(&backend, &source, AudioOptions::default())
            .unwrap();
        assert!(loaded.samples.is_mapped());
        assert_eq!(loaded, clip());
        let file = cache.file_for(&source, AudioOptions::default()).unwrap();
        assert_eq!(
            fs::metadata(&file).unwrap().len(),
            (HEADER_LEN + 6 * 4) as u64
        );

        // Found again without decoding: a backend that no longer knows the file still loads it.
        let again = cache
            .load(&FakeBackend::new(), &source, AudioOptions::default())
            .unwrap();
        assert_eq!(again, clip());

        // A copy of the source has the same content, so the same cache file.
        let copy = temp.0.join("copy.wav");
        fs::copy(&source, &copy).unwrap();
        assert_eq!(
            cache.file_for(&copy, AudioOptions::default()).unwrap(),
            file
        );
        // Changed content is a new file.
        fs::write(&source, b"other audio!").unwrap();
        assert_ne!(
            cache.file_for(&source, AudioOptions::default()).unwrap(),
            file
        );

        // Cleared, it is decoded again.
        cache.clear().unwrap();
        drop((loaded, again));
        assert!(!file.exists() || cfg!(windows));
    }

    #[test]
    fn a_damaged_cache_file_is_rebuilt() {
        let temp = TempDir::new("damaged");
        let source = temp.0.join("song.wav");
        fs::write(&source, b"some audio").unwrap();
        let backend = FakeBackend::new().with_audio(&source, clip());
        let cache = AudioCache::new(temp.0.join("cache"));
        let file = cache.file_for(&source, AudioOptions::default()).unwrap();
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"RSAUDIO\0 truncated").unwrap();
        let loaded = cache
            .load(&backend, &source, AudioOptions::default())
            .unwrap();
        assert_eq!(loaded, clip());
        assert!(loaded.samples.is_mapped());
    }

    #[test]
    fn an_unusable_cache_falls_back_to_memory_and_decode_errors_pass_through() {
        let temp = TempDir::new("unusable");
        let source = temp.0.join("song.wav");
        fs::write(&source, b"some audio").unwrap();
        let backend = FakeBackend::new().with_audio(&source, clip());
        // The cache directory is a file, so nothing can be written under it.
        let blocked = temp.0.join("blocked");
        fs::write(&blocked, b"").unwrap();
        let cache = AudioCache::new(&blocked);
        let loaded = cache
            .load(&backend, &source, AudioOptions::default())
            .unwrap();
        assert_eq!(loaded, clip());
        assert!(!loaded.samples.is_mapped());

        let cache = AudioCache::new(temp.0.join("cache"));
        let unknown = FakeBackend::new();
        assert!(matches!(
            cache.load(&unknown, &source, AudioOptions::default()),
            Err(MediaError::Open { .. })
        ));
        // A missing source fails as it would without the cache.
        assert!(
            cache
                .load(
                    &backend,
                    &temp.0.join("missing.wav"),
                    AudioOptions::default()
                )
                .is_err()
        );
    }
}
