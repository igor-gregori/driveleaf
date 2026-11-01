//! Locates the MFT on a volume: where its pieces live and which records are used.

use std::io;

use crate::error::Error;
use crate::geometry::Geometry;
use crate::record::{
    ATTR_ATTRIBUTE_LIST, ATTR_BITMAP, ATTR_DATA, AttributeBody, Attributes, RecordHeader,
    apply_fixups, attribute_list,
};
use crate::runs::{Run, Runs};
use crate::source::{AlignedBuffer, BlockSource};

/// More records than this do not fit the model's 32-bit indices.
const MAX_RECORDS: u64 = u32::MAX as u64;
/// Far above any real `$MFT` attribute list; keeps a hostile size from allocating.
const MAX_LIST_BYTES: u64 = 1 << 24;
/// Far above what any real `$MFT` spills into; keeps a hostile list from dragging on.
const MAX_EXTENSION_RECORDS: usize = 4096;

/// A contiguous piece of the MFT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Extent {
    /// Offset inside the MFT.
    mft_offset: u64,
    bytes: u64,
    /// Offset on the volume, `None` for a hole.
    volume_offset: Option<u64>,
}

/// A range of consecutive records to read as one block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReadJob {
    pub(crate) first_record: u64,
    pub(crate) records: u32,
}

/// Where the MFT is and which of its records are in use.
pub(crate) struct MftLayout {
    record_bytes: u64,
    record_count: u64,
    extents: Vec<Extent>,
    /// One bit per record, `None` when `$MFT:$BITMAP` could not be read.
    bitmap: Option<Vec<u8>>,
}

/// Run lists of one attribute, keyed by their first VCN.
#[derive(Default)]
struct RunLists {
    pieces: Vec<(u64, Vec<Run>)>,
}

impl RunLists {
    fn add(&mut self, lowest_vcn: u64, encoded: &[u8]) -> Result<(), Error> {
        let mut decoder = Runs::new(encoded);
        let runs: Vec<Run> = decoder.by_ref().collect();
        if decoder.malformed() {
            return Err(Error::BadMft("malformed data runs"));
        }
        if !self.pieces.iter().any(|(vcn, _)| *vcn == lowest_vcn) {
            self.pieces.push((lowest_vcn, runs));
        }
        Ok(())
    }

    fn ordered(&self) -> impl Iterator<Item = &Run> {
        let mut order: Vec<&(u64, Vec<Run>)> = self.pieces.iter().collect();
        order.sort_by_key(|(vcn, _)| *vcn);
        order.into_iter().flat_map(|(_, runs)| runs.iter())
    }
}

/// What the `$MFT` records say about the MFT itself.
#[derive(Default)]
struct MftAttributes {
    data: RunLists,
    data_real_size: Option<u64>,
    bitmap: RunLists,
    bitmap_real_size: Option<u64>,
    resident_bitmap: Option<Vec<u8>>,
    list_value: Option<Vec<u8>>,
    list_runs: RunLists,
    list_real_size: u64,
}

impl MftAttributes {
    fn absorb(&mut self, record: &[u8]) -> Result<(), Error> {
        let header = RecordHeader::parse(record).map_err(|_| Error::BadMft("bad record header"))?;
        let mut attributes = Attributes::new(record, &header);
        for attribute in attributes.by_ref() {
            if !attribute.name.is_empty() {
                continue;
            }
            match (attribute.type_code, attribute.body) {
                (ATTR_DATA, AttributeBody::NonResident(extent)) => {
                    if extent.lowest_vcn == 0 {
                        self.data_real_size = Some(extent.real_size);
                    }
                    self.data.add(extent.lowest_vcn, extent.runs)?;
                }
                (ATTR_BITMAP, AttributeBody::NonResident(extent)) => {
                    if extent.lowest_vcn == 0 {
                        self.bitmap_real_size = Some(extent.real_size);
                    }
                    self.bitmap.add(extent.lowest_vcn, extent.runs)?;
                }
                (ATTR_BITMAP, AttributeBody::Resident(value)) => {
                    self.resident_bitmap = Some(value.to_vec());
                }
                (ATTR_ATTRIBUTE_LIST, AttributeBody::Resident(value)) => {
                    self.list_value = Some(value.to_vec());
                }
                (ATTR_ATTRIBUTE_LIST, AttributeBody::NonResident(extent)) => {
                    if extent.lowest_vcn == 0 {
                        self.list_real_size = extent.real_size;
                    }
                    self.list_runs.add(extent.lowest_vcn, extent.runs)?;
                }
                _ => {}
            }
        }
        if attributes.malformed() {
            return Err(Error::BadMft("malformed attribute in the MFT's own record"));
        }
        Ok(())
    }
}

impl MftLayout {
    pub(crate) fn load<S: BlockSource + ?Sized>(
        source: &S,
        geometry: &Geometry,
    ) -> Result<Self, Error> {
        let record_bytes = u64::from(geometry.bytes_per_record);
        let cluster = u64::from(geometry.bytes_per_cluster);
        let first_offset = geometry
            .mft_offset()
            .ok_or(Error::BadMft("MFT location overflows"))?;
        let mut buffer = AlignedBuffer::new(record_bytes as usize);

        source.read_exact_at(first_offset, buffer.as_mut_slice())?;
        apply_fixups(buffer.as_mut_slice())
            .map_err(|_| Error::BadMft("the MFT's first record is damaged"))?;
        let mut attributes = MftAttributes::default();
        attributes.absorb(buffer.as_mut_slice())?;

        // A very fragmented MFT spills its own run list into extension records,
        // named by $ATTRIBUTE_LIST. They usually live in the part already known.
        let list = read_attribute_list(source, &attributes, cluster)?;
        let mut visited = vec![0_u64];
        for entry in attribute_list(&list) {
            let wanted = matches!(entry.type_code, ATTR_DATA | ATTR_BITMAP);
            if !wanted || !entry.unnamed || visited.contains(&entry.record) {
                continue;
            }
            if visited.len() == MAX_EXTENSION_RECORDS {
                return Err(Error::BadMft("too many extension records"));
            }
            visited.push(entry.record);
            let known = extents_from(&attributes.data, cluster)?;
            let offset = entry
                .record
                .checked_mul(record_bytes)
                .ok_or(Error::BadMft("attribute list points too far"))?;
            read_mft_bytes(source, &known, offset, buffer.as_mut_slice())?;
            apply_fixups(buffer.as_mut_slice())
                .map_err(|_| Error::BadMft("an MFT extension record is damaged"))?;
            attributes.absorb(buffer.as_mut_slice())?;
        }

        let extents = extents_from(&attributes.data, cluster)?;
        let mapped: u64 = extents.iter().map(|extent| extent.bytes).sum();
        let real_size = attributes
            .data_real_size
            .ok_or(Error::BadMft("the MFT has no main stream"))?;
        let record_count = real_size.min(mapped) / record_bytes;
        if record_count == 0 || record_count > MAX_RECORDS {
            return Err(Error::BadMft("implausible record count"));
        }
        let bitmap = read_bitmap(source, &attributes, cluster, record_count);
        Ok(Self {
            record_bytes,
            record_count,
            extents,
            bitmap,
        })
    }

    pub(crate) fn record_count(&self) -> u64 {
        self.record_count
    }

    /// Records the bitmap marks as used, which is what a scan parses.
    pub(crate) fn used_record_count(&self) -> u64 {
        match &self.bitmap {
            Some(_) => (0..self.record_count)
                .filter(|&record| !self.is_free(record))
                .count() as u64,
            None => self.record_count,
        }
    }

    pub(crate) fn extent_count(&self) -> usize {
        self.extents.len()
    }

    /// True when the bitmap marks the record free. Without a bitmap nothing is.
    #[inline]
    pub(crate) fn is_free(&self, record: u64) -> bool {
        let Some(bitmap) = &self.bitmap else {
            return false;
        };
        let byte = (record / 8) as usize;
        match bitmap.get(byte) {
            Some(bits) => bits & (1 << (record % 8)) == 0,
            None => true,
        }
    }

    /// Splits the MFT into blocks, dropping free records at both ends of each
    /// block and whole blocks that hold nothing.
    pub(crate) fn plan_reads(&self, block_records: u64) -> Vec<ReadJob> {
        let mut jobs = Vec::new();
        let mut start = 0;
        while start < self.record_count {
            let end = (start + block_records).min(self.record_count);
            let first = (start..end).find(|&record| !self.is_free(record));
            if let Some(first) = first {
                let last = (first..end)
                    .rev()
                    .find(|&record| !self.is_free(record))
                    .unwrap_or(first);
                jobs.push(ReadJob {
                    first_record: first,
                    records: (last - first + 1) as u32,
                });
            }
            start = end;
        }
        jobs
    }

    pub(crate) fn read_job<S: BlockSource + ?Sized>(
        &self,
        source: &S,
        job: ReadJob,
        buffer: &mut [u8],
    ) -> io::Result<()> {
        let offset = job.first_record * self.record_bytes;
        let bytes = u64::from(job.records) * self.record_bytes;
        read_mft_bytes(source, &self.extents, offset, &mut buffer[..bytes as usize])
    }
}

fn read_attribute_list<S: BlockSource + ?Sized>(
    source: &S,
    attributes: &MftAttributes,
    cluster: u64,
) -> Result<Vec<u8>, Error> {
    if let Some(value) = &attributes.list_value {
        return Ok(value.clone());
    }
    if attributes.list_real_size == 0 {
        return Ok(Vec::new());
    }
    let mut value = read_runs(
        source,
        attributes.list_runs.ordered(),
        cluster,
        MAX_LIST_BYTES,
    )?;
    value.truncate(attributes.list_real_size as usize);
    Ok(value)
}

/// A missing or unreadable bitmap only makes the scan slower, so it is not fatal.
fn read_bitmap<S: BlockSource + ?Sized>(
    source: &S,
    attributes: &MftAttributes,
    cluster: u64,
    record_count: u64,
) -> Option<Vec<u8>> {
    let needed = record_count.div_ceil(8);
    if let Some(value) = &attributes.resident_bitmap {
        return Some(value.clone());
    }
    let real_size = attributes.bitmap_real_size?;
    let limit = needed.next_multiple_of(cluster).max(cluster);
    let mut bitmap = read_runs(source, attributes.bitmap.ordered(), cluster, limit).ok()?;
    bitmap.truncate(real_size.min(needed) as usize);
    Some(bitmap)
}

/// Reads up to `limit` bytes described by a run list.
fn read_runs<'r, S: BlockSource + ?Sized>(
    source: &S,
    runs: impl Iterator<Item = &'r Run>,
    cluster: u64,
    limit: u64,
) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    for run in runs {
        let left = limit.saturating_sub(out.len() as u64);
        if left == 0 {
            break;
        }
        let bytes = run
            .clusters
            .checked_mul(cluster)
            .ok_or(Error::BadMft("run length overflows"))?
            .min(left.next_multiple_of(cluster));
        let mut piece = AlignedBuffer::new(bytes as usize);
        let slice = piece.as_mut_slice();
        if let Some(lcn) = run.lcn {
            let offset = lcn
                .checked_mul(cluster)
                .ok_or(Error::BadMft("run location overflows"))?;
            source.read_exact_at(offset, slice)?;
        }
        out.extend_from_slice(slice);
    }
    Ok(out)
}

fn extents_from(lists: &RunLists, cluster: u64) -> Result<Vec<Extent>, Error> {
    let mut extents = Vec::new();
    let mut mft_offset = 0_u64;
    for run in lists.ordered() {
        let bytes = run
            .clusters
            .checked_mul(cluster)
            .ok_or(Error::BadMft("run length overflows"))?;
        let volume_offset = match run.lcn {
            Some(lcn) => Some(
                lcn.checked_mul(cluster)
                    .ok_or(Error::BadMft("run location overflows"))?,
            ),
            None => None,
        };
        extents.push(Extent {
            mft_offset,
            bytes,
            volume_offset,
        });
        mft_offset = mft_offset
            .checked_add(bytes)
            .ok_or(Error::BadMft("MFT size overflows"))?;
    }
    Ok(extents)
}

/// Fills `buffer` from an offset inside the MFT, crossing extents as needed.
fn read_mft_bytes<S: BlockSource + ?Sized>(
    source: &S,
    extents: &[Extent],
    offset: u64,
    buffer: &mut [u8],
) -> io::Result<()> {
    let mut index = extents.partition_point(|extent| extent.mft_offset + extent.bytes <= offset);
    let mut position = offset;
    let mut done = 0;
    while done < buffer.len() {
        let extent = extents.get(index).ok_or(io::ErrorKind::UnexpectedEof)?;
        let within = position - extent.mft_offset;
        let take = ((extent.bytes - within) as usize).min(buffer.len() - done);
        let slice = &mut buffer[done..done + take];
        match extent.volume_offset {
            Some(volume) => {
                let start = volume
                    .checked_add(within)
                    .ok_or(io::ErrorKind::UnexpectedEof)?;
                source.read_exact_at(start, slice)?;
            }
            None => slice.fill(0),
        }
        done += take;
        position += take as u64;
        index += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{ImageLayout, RecordSpec, write_image};
    use std::io::Cursor;

    fn image(layout: &ImageLayout, records: Vec<(u64, RecordSpec)>) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        write_image(&mut out, layout, records).unwrap();
        out.into_inner()
    }

    const LAYOUT: ImageLayout = ImageLayout {
        bytes_per_cluster: 4096,
        bytes_per_record: 1024,
        record_count: 4096,
        fragments: 4,
    };

    fn load(bytes: &[u8]) -> MftLayout {
        let geometry = Geometry::from_boot_sector(&bytes[..512]).unwrap();
        MftLayout::load(bytes, &geometry).unwrap()
    }

    #[test]
    fn finds_every_fragment_and_the_bitmap() {
        let bytes = image(&LAYOUT, vec![(5, RecordSpec::directory(5))]);
        let layout = load(&bytes);
        assert_eq!(layout.record_count(), 4096);
        assert_eq!(layout.extent_count(), 4);
        assert!(!layout.is_free(0));
        assert!(!layout.is_free(5));
        assert!(layout.is_free(6));
        assert!(layout.is_free(4095));
        assert!(layout.is_free(99_999));
    }

    #[test]
    fn reads_cross_fragment_boundaries() {
        let records = (0..3)
            .map(|index| (1022 + index, RecordSpec::file(index as u16 + 9)))
            .collect();
        let bytes = image(&LAYOUT, records);
        let layout = load(&bytes);
        let mut buffer = vec![0_u8; 3 * 1024];
        let job = ReadJob {
            first_record: 1022,
            records: 3,
        };
        layout.read_job(&bytes[..], job, &mut buffer).unwrap();
        for (index, slot) in buffer.as_chunks_mut::<1024>().0.iter_mut().enumerate() {
            apply_fixups(slot).unwrap();
            let header = RecordHeader::parse(slot).unwrap();
            assert_eq!(usize::from(header.sequence), index + 9);
        }
    }

    #[test]
    fn plans_skip_free_space() {
        let records = vec![
            (10, RecordSpec::file(1)),
            (12, RecordSpec::file(1)),
            (4000, RecordSpec::file(1)),
        ];
        let bytes = image(&LAYOUT, records);
        let jobs = load(&bytes).plan_reads(1024);
        assert_eq!(
            jobs,
            vec![
                ReadJob {
                    first_record: 0,
                    records: 13
                },
                ReadJob {
                    first_record: 4000,
                    records: 1
                },
            ]
        );
    }

    #[test]
    fn a_damaged_first_record_is_an_error_not_a_panic() {
        let mut bytes = image(&LAYOUT, Vec::new());
        let offset = LAYOUT.record_offset(0) as usize;
        bytes[offset + 510] ^= 0xFF;
        let geometry = Geometry::from_boot_sector(&bytes[..512]).unwrap();
        assert!(matches!(
            MftLayout::load(&bytes[..], &geometry),
            Err(Error::BadMft(_))
        ));
    }
}
