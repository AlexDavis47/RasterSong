//! Shared audio samples, held in memory or read straight from a memory-mapped file.

use std::fmt::Debug;
use std::fs::File;
use std::io;
use std::ops::Deref;
use std::sync::Arc;

use memmap2::Mmap;

/// Interleaved `f32` samples that read as a slice. Cloning shares them rather than copying, so
/// every reader of a track (the renderer, the preview mixer, the waveform) reads one copy.
#[derive(Clone)]
pub struct Samples(Storage);

#[derive(Clone)]
enum Storage {
    Owned(Arc<[f32]>),
    /// `len` samples starting `offset` bytes into the map.
    Mapped {
        map: Arc<Mmap>,
        offset: usize,
        len: usize,
    },
}

impl Samples {
    /// Maps `len` native-endian `f32` samples starting `offset` bytes into `file`.
    ///
    /// The file must not change while it is mapped: the slice would change under its readers.
    /// Cache files are written once under a temporary name and renamed into place, so they don't.
    pub fn map(file: &File, offset: usize, len: usize) -> io::Result<Self> {
        if !offset.is_multiple_of(align_of::<f32>()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "samples must start on a 4-byte boundary",
            ));
        }
        let bytes = len
            .checked_mul(size_of::<f32>())
            .and_then(|b| b.checked_add(offset))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "too many samples"))?;
        // SAFETY: see above; the file is never written while mapped.
        let map = unsafe { Mmap::map(file)? };
        if map.len() < bytes {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the file is shorter than its samples",
            ));
        }
        Ok(Self(Storage::Mapped {
            map: Arc::new(map),
            offset,
            len,
        }))
    }

    /// Whether the samples are read from a memory-mapped file.
    pub fn is_mapped(&self) -> bool {
        matches!(self.0, Storage::Mapped { .. })
    }
}

impl Deref for Samples {
    type Target = [f32];

    fn deref(&self) -> &[f32] {
        match &self.0 {
            Storage::Owned(samples) => samples,
            Storage::Mapped { len: 0, .. } => &[],
            Storage::Mapped { map, offset, len } => {
                let bytes = &map[*offset..*offset + len * size_of::<f32>()];
                // SAFETY: the bytes are in bounds (checked in `map`), aligned (a map starts on a
                // page boundary and `offset` is a multiple of 4), and any bit pattern is an f32.
                unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<f32>(), *len) }
            }
        }
    }
}

impl From<Vec<f32>> for Samples {
    fn from(samples: Vec<f32>) -> Self {
        Self(Storage::Owned(samples.into()))
    }
}

impl FromIterator<f32> for Samples {
    fn from_iter<I: IntoIterator<Item = f32>>(iter: I) -> Self {
        Self(Storage::Owned(iter.into_iter().collect()))
    }
}

impl<'a> IntoIterator for &'a Samples {
    type Item = &'a f32;
    type IntoIter = std::slice::Iter<'a, f32>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl Default for Samples {
    fn default() -> Self {
        Vec::new().into()
    }
}

impl PartialEq for Samples {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl Debug for Samples {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = if self.is_mapped() { "mapped" } else { "owned" };
        write!(f, "Samples({} {kind})", self.len())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn mapped_samples_read_as_a_slice() {
        let dir = std::env::temp_dir().join(format!("rastersong-samples-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("samples.f32");
        let mut file = File::create(&path).unwrap();
        file.write_all(&[0; 8]).unwrap();
        for x in [0.5f32, -0.25, 1.0] {
            file.write_all(&x.to_ne_bytes()).unwrap();
        }
        drop(file);
        let file = File::open(&path).unwrap();
        let samples = Samples::map(&file, 8, 3).unwrap();
        assert!(samples.is_mapped());
        assert_eq!(*samples, [0.5, -0.25, 1.0]);
        assert_eq!(samples, Samples::from(vec![0.5, -0.25, 1.0]));
        assert!(Samples::map(&file, 8, 4).is_err());
        assert!(Samples::map(&file, 2, 1).is_err());
        assert!(Samples::map(&file, 20, 0).unwrap().is_empty());
        drop((samples, file));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
