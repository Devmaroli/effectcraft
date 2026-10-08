//! Random-access file reader: open movies without `fs::read` of the whole file.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use filmcraft_media::{ByteReader, SharedReader, SharedSource};

use crate::{MediaError, Result};

/// A [`ByteReader`] over an on-disk file. Each `read_at` seeks and fills; concurrent decodes
/// take the mutex (FilmCraft's GOP cache is sequential per source).
pub struct FileReader {
    file: Mutex<File>,
    len: u64,
}

impl FileReader {
    /// Open `path` for random-access reads.
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|e| MediaError::Io(format!("{}: {e}", path.display())))?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(FileReader { file: Mutex::new(file), len })
    }
}

impl ByteReader for FileReader {
    fn len(&self) -> u64 {
        self.len
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
        let mut f = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        f.seek(SeekFrom::Start(offset))?;
        f.read_exact(buf)
    }
}

/// Open a movie/audio file by streaming (reader openers first; whole-file fallback for stills
/// and WAV). In-memory sources still use [`filmcraft_codecs::open_bytes`].
pub fn open_path(path: &str) -> Result<SharedSource> {
    let p = Path::new(path);
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let reader: SharedReader = Arc::new(FileReader::open(p)?);
    filmcraft_media::reader::open_reader(&name, reader, &filmcraft_codecs::reader_openers(), &filmcraft_codecs::openers()).map_err(MediaError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_reader_reads_ranges() {
        let dir = std::env::temp_dir().join(format!(
            "ec-stream-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("t.bin");
        std::fs::write(&p, b"0123456789abcdef").unwrap();
        let r = FileReader::open(&p).unwrap();
        let mut b = [0u8; 4];
        r.read_at(4, &mut b).unwrap();
        assert_eq!(&b, b"4567");
        assert_eq!(r.len(), 16);
        assert!(r.read_at(14, &mut b).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
