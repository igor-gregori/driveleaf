//! Capture and replay of what a scan reads from a volume.
//!
//! A capture holds the volume geometry plus every block a scan fetched, so the
//! same scan can run later from the file, on any platform and without
//! administrator rights. It contains every file name on the volume, so treat it
//! as private.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;

use crate::geometry::Geometry;
use crate::source::BlockSource;

const MAGIC: &[u8; 8] = b"DLMFTv01";
const NO_FREE_COUNT: u64 = u64::MAX;

/// Wraps a source and appends everything read through it to a capture file.
pub struct RecordingSource<'a, S: BlockSource + ?Sized> {
    inner: &'a S,
    out: Mutex<BufWriter<File>>,
}

impl<'a, S: BlockSource + ?Sized> RecordingSource<'a, S> {
    pub fn create(inner: &'a S, geometry: &Geometry, path: &Path) -> io::Result<Self> {
        let mut out = BufWriter::with_capacity(1 << 20, File::create(path)?);
        out.write_all(MAGIC)?;
        let fields = [
            u64::from(geometry.bytes_per_sector),
            u64::from(geometry.bytes_per_cluster),
            u64::from(geometry.bytes_per_record),
            geometry.mft_lcn,
            geometry.total_clusters,
            geometry.free_clusters.unwrap_or(NO_FREE_COUNT),
        ];
        for field in fields {
            out.write_all(&field.to_le_bytes())?;
        }
        Ok(Self {
            inner,
            out: Mutex::new(out),
        })
    }

    pub fn finish(self) -> io::Result<()> {
        let mut out = self.out.into_inner().expect("no recorder panicked");
        out.flush()
    }
}

impl<S: BlockSource + ?Sized> BlockSource for RecordingSource<'_, S> {
    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        self.inner.read_exact_at(offset, buffer)?;
        let mut out = self.out.lock().expect("no recorder panicked");
        out.write_all(&offset.to_le_bytes())?;
        out.write_all(&(buffer.len() as u64).to_le_bytes())?;
        out.write_all(buffer)
    }
}

/// One captured read.
#[derive(Clone, Copy)]
struct Piece {
    volume_offset: u64,
    bytes: u64,
    file_offset: u64,
}

/// Serves reads from a capture file.
pub struct ReplaySource {
    file: crate::source::ImageFile,
    pieces: Vec<Piece>,
    geometry: Geometry,
}

impl ReplaySource {
    /// True when the file starts like a capture.
    pub fn is_capture(path: &Path) -> io::Result<bool> {
        let mut magic = [0_u8; 8];
        match File::open(path)?.read_exact(&mut magic) {
            Ok(()) => Ok(&magic == MAGIC),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        let mut input = BufReader::new(File::open(path)?);
        let mut magic = [0_u8; 8];
        input.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a Driveleaf capture",
            ));
        }
        let mut fields = [0_u64; 6];
        for field in &mut fields {
            *field = read_u64(&mut input)?;
        }
        let geometry = Geometry {
            bytes_per_sector: u32::try_from(fields[0]).map_err(invalid)?,
            bytes_per_cluster: u32::try_from(fields[1]).map_err(invalid)?,
            bytes_per_record: u32::try_from(fields[2]).map_err(invalid)?,
            mft_lcn: fields[3],
            total_clusters: fields[4],
            free_clusters: (fields[5] != NO_FREE_COUNT).then_some(fields[5]),
        };
        let length = input.get_ref().metadata()?.len();
        let mut pieces = Vec::new();
        let mut position = input.stream_position()?;
        while position < length {
            let volume_offset = read_u64(&mut input)?;
            let bytes = read_u64(&mut input)?;
            let file_offset = position + 16;
            let next = file_offset
                .checked_add(bytes)
                .filter(|&next| next <= length)
                .ok_or_else(|| invalid("capture is truncated"))?;
            pieces.push(Piece {
                volume_offset,
                bytes,
                file_offset,
            });
            input.seek(SeekFrom::Start(next))?;
            position = next;
        }
        pieces.sort_unstable_by_key(|piece| piece.volume_offset);
        Ok(Self {
            file: crate::source::ImageFile::open(path)?,
            pieces,
            geometry,
        })
    }

    pub fn geometry(&self) -> Geometry {
        self.geometry
    }
}

impl BlockSource for ReplaySource {
    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        let mut done = 0;
        while done < buffer.len() {
            let position = offset + done as u64;
            let index = self
                .pieces
                .partition_point(|piece| piece.volume_offset <= position);
            let piece = index
                .checked_sub(1)
                .map(|index| self.pieces[index])
                .filter(|piece| position < piece.volume_offset + piece.bytes)
                .ok_or(io::ErrorKind::UnexpectedEof)?;
            let within = position - piece.volume_offset;
            let take = ((piece.bytes - within) as usize).min(buffer.len() - done);
            self.file
                .read_exact_at(piece.file_offset + within, &mut buffer[done..done + take])?;
            done += take;
        }
        Ok(())
    }
}

fn read_u64(input: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0_u8; 8];
    input.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{ScanControl, ScanOptions, scan};
    use crate::synth::{ImageLayout, TreeTotals, generate_tree, write_image};
    use std::io::Cursor;

    #[test]
    fn a_replayed_scan_equals_the_original() {
        let layout = ImageLayout {
            bytes_per_cluster: 4096,
            bytes_per_record: 1024,
            record_count: 20_000,
            fragments: 5,
        };
        let mut totals = TreeTotals::default();
        let mut image = Cursor::new(Vec::new());
        write_image(
            &mut image,
            &layout,
            generate_tree(layout.record_count, 4096, &mut totals),
        )
        .unwrap();
        let bytes = image.into_inner();
        let geometry = Geometry::from_boot_sector(&bytes[..512]).unwrap();
        let path = std::env::temp_dir().join(format!("driveleaf-{}.dlmft", std::process::id()));

        let options = ScanOptions {
            readers: 3,
            ..ScanOptions::default()
        };
        let recorder = RecordingSource::create(&bytes[..], &geometry, &path).unwrap();
        let original = scan(&recorder, &geometry, &options, &ScanControl::default()).unwrap();
        recorder.finish().unwrap();

        assert!(ReplaySource::is_capture(&path).unwrap());
        let replay = ReplaySource::open(&path).unwrap();
        assert_eq!(replay.geometry(), geometry);
        let again = scan(
            &replay,
            &replay.geometry(),
            &ScanOptions {
                block_bytes: 1 << 16,
                ..ScanOptions::default()
            },
            &ScanControl::default(),
        )
        .unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(again.stats.files, original.stats.files);
        assert_eq!(again.stats.allocated_bytes, original.stats.allocated_bytes);
        assert_eq!(again.stats.corrupt_records, 0);
    }

    #[test]
    fn other_files_are_not_captures() {
        let path = std::env::temp_dir().join(format!("driveleaf-{}.txt", std::process::id()));
        std::fs::write(&path, b"hello").unwrap();
        assert!(!ReplaySource::is_capture(&path).unwrap());
        assert!(ReplaySource::open(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
