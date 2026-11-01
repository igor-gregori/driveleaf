//! Block access to a volume. The parser only ever sees this trait, so the same
//! code runs on a live Windows volume and on a disk image.

use std::fs::File;
use std::io;
use std::path::Path;

/// Buffer alignment that satisfies unbuffered reads on every sector size.
pub(crate) const BUFFER_ALIGNMENT: usize = 4096;

/// Random access reads.
///
/// Offsets and lengths are multiples of the volume sector size, and buffers
/// start on a 4 KiB boundary, as unbuffered Windows reads require.
pub trait BlockSource: Sync {
    /// Fills `buffer` from `offset`, failing on a short read.
    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()>;
}

/// A volume held in a regular file.
pub struct ImageFile {
    file: File,
}

impl ImageFile {
    pub fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path)?,
        })
    }
}

impl BlockSource for ImageFile {
    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        read_file_at(&self.file, offset, buffer)
    }
}

#[cfg(unix)]
fn read_file_at(file: &File, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buffer, offset)
}

#[cfg(windows)]
fn read_file_at(file: &File, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
    let mut done = 0;
    while done < buffer.len() {
        let position = offset + done as u64;
        match std::os::windows::fs::FileExt::seek_read(file, &mut buffer[done..], position)? {
            0 => return Err(io::ErrorKind::UnexpectedEof.into()),
            read => done += read,
        }
    }
    Ok(())
}

/// A volume held in memory.
impl BlockSource for [u8] {
    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        let start = usize::try_from(offset).map_err(|_| io::ErrorKind::UnexpectedEof)?;
        let end = start
            .checked_add(buffer.len())
            .ok_or(io::ErrorKind::UnexpectedEof)?;
        let bytes = self.get(start..end).ok_or(io::ErrorKind::UnexpectedEof)?;
        buffer.copy_from_slice(bytes);
        Ok(())
    }
}

/// A byte buffer starting on a [`BUFFER_ALIGNMENT`] boundary.
pub(crate) struct AlignedBuffer {
    storage: Vec<u8>,
    start: usize,
    len: usize,
}

impl AlignedBuffer {
    pub(crate) fn new(len: usize) -> Self {
        let storage = vec![0_u8; len + BUFFER_ALIGNMENT];
        let address = storage.as_ptr().addr();
        let start = address.next_multiple_of(BUFFER_ALIGNMENT) - address;
        Self {
            storage,
            start,
            len,
        }
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.storage[self.start..self.start + self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffers_are_aligned() {
        for len in [1, 512, 1024, 1 << 20] {
            let mut buffer = AlignedBuffer::new(len);
            let slice = buffer.as_mut_slice();
            assert_eq!(slice.len(), len);
            assert_eq!(slice.as_ptr().addr() % BUFFER_ALIGNMENT, 0);
        }
    }

    #[test]
    fn memory_source_rejects_reads_past_the_end() {
        let bytes = [1_u8, 2, 3, 4];
        let source: &[u8] = &bytes;
        let mut buffer = [0_u8; 2];
        source.read_exact_at(2, &mut buffer).unwrap();
        assert_eq!(buffer, [3, 4]);
        assert!(source.read_exact_at(3, &mut buffer).is_err());
        assert!(source.read_exact_at(u64::MAX, &mut buffer).is_err());
    }
}
