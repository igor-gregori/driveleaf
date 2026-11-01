//! The scan itself. One thread reads the MFT in large blocks, the rayon pool
//! parses them, and every block yields one compact chunk of results.
//!
//! Extension records need no `$ATTRIBUTE_LIST` lookup here: since every record
//! gets visited anyway, each one simply credits what it holds to its base record.

use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::error::Error;
use crate::geometry::Geometry;
use crate::mft::{MftLayout, ReadJob};
use crate::names::push_wtf8;
use crate::record::{
    ATTR_DATA, ATTR_FILE_NAME, ATTR_STANDARD_INFORMATION, AttributeBody, Attributes, FileName,
    NAMESPACE_DOS, NonResident, RecordHeader, apply_fixups, standard_attributes,
};
use crate::runs::Runs;
use crate::source::{AlignedBuffer, BlockSource};

/// `$BadClus`, whose `$Bad` stream claims the size of the whole volume.
const BAD_CLUSTERS_RECORD: u64 = 8;

/// The record is a directory.
pub const RAW_DIRECTORY: u8 = 0x01;
/// The record continues another one instead of being a file of its own.
pub const RAW_EXTENSION: u8 = 0x02;

/// Tuning knobs for [`scan`].
#[derive(Clone, Copy, Debug)]
pub struct ScanOptions {
    /// Largest single read.
    pub block_bytes: usize,
    /// Blocks in flight between the reader and the parsers.
    pub buffers: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            block_bytes: 4 << 20,
            buffers: (rayon::current_num_threads() * 2).max(4),
        }
    }
}

/// What one in-use record says about its file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawFile {
    /// The base record, which differs from the slot for extension records.
    pub record: u32,
    pub sequence: u16,
    pub link_count: u16,
    /// `$STANDARD_INFORMATION` file attribute flags.
    pub attributes: u32,
    /// [`RAW_DIRECTORY`] and [`RAW_EXTENSION`].
    pub flags: u8,
    /// Size of the unnamed data stream.
    pub logical_size: u64,
    /// Size of the named data streams.
    pub stream_size: u64,
    /// Bytes on disk, over every attribute stored in clusters.
    pub allocated: u64,
}

/// One name of a file. A hard-linked file has several.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawName {
    pub record: u32,
    pub parent: u32,
    pub parent_sequence: u16,
    /// Length in bytes of the WTF-8 name in the chunk's pool.
    pub len: u16,
    pub start: u32,
}

/// Results of one block.
#[derive(Debug, Default)]
pub struct Chunk {
    pub files: Vec<RawFile>,
    pub names: Vec<RawName>,
    /// WTF-8 names, addressed by [`RawName`].
    pub pool: Vec<u8>,
}

impl Chunk {
    pub fn name(&self, name: &RawName) -> &[u8] {
        let start = name.start as usize;
        &self.pool[start..start + usize::from(name.len)]
    }

    fn memory_bytes(&self) -> usize {
        self.files.capacity() * size_of::<RawFile>()
            + self.names.capacity() * size_of::<RawName>()
            + self.pool.capacity()
    }
}

/// Counters for a whole scan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanStats {
    /// Slots the MFT has room for.
    pub records_total: u64,
    /// Slots read and parsed.
    pub records_parsed: u64,
    /// Valid records with the in-use flag off, seen inside read blocks.
    pub records_unused: u64,
    pub base_records: u64,
    pub extension_records: u64,
    pub files: u64,
    pub directories: u64,
    pub names: u64,
    pub name_bytes: u64,
    pub logical_bytes: u64,
    pub stream_bytes: u64,
    pub allocated_bytes: u64,
    /// Records the bitmap marks as used but that failed to decode.
    pub corrupt_records: u64,
    /// Records that decoded only in part.
    pub malformed_records: u64,
    pub unreadable_blocks: u64,
    pub bytes_read: u64,
}

impl ScanStats {
    fn absorb(&mut self, other: &ScanStats) {
        self.records_parsed += other.records_parsed;
        self.records_unused += other.records_unused;
        self.base_records += other.base_records;
        self.extension_records += other.extension_records;
        self.files += other.files;
        self.directories += other.directories;
        self.names += other.names;
        self.name_bytes += other.name_bytes;
        self.logical_bytes = self.logical_bytes.saturating_add(other.logical_bytes);
        self.stream_bytes = self.stream_bytes.saturating_add(other.stream_bytes);
        self.allocated_bytes = self.allocated_bytes.saturating_add(other.allocated_bytes);
        self.corrupt_records += other.corrupt_records;
        self.malformed_records += other.malformed_records;
    }
}

/// Everything a scan produced.
#[derive(Debug)]
pub struct ScanOutput {
    pub chunks: Vec<Chunk>,
    pub stats: ScanStats,
    pub mft_extents: usize,
    pub read_jobs: usize,
    /// Time the reader spent inside read calls.
    pub read_time: Duration,
    pub total_time: Duration,
}

impl ScanOutput {
    /// Heap bytes held by the results.
    pub fn memory_bytes(&self) -> usize {
        self.chunks.iter().map(Chunk::memory_bytes).sum::<usize>()
            + self.chunks.capacity() * size_of::<Chunk>()
    }
}

/// Reads and parses the whole MFT of a volume.
pub fn scan<S: BlockSource + ?Sized>(
    source: &S,
    geometry: &Geometry,
    options: &ScanOptions,
) -> Result<ScanOutput, Error> {
    let started = Instant::now();
    let layout = MftLayout::load(source, geometry)?;
    let record_bytes = geometry.bytes_per_record as usize;
    let block_records = (options.block_bytes / record_bytes).max(1);
    let block_bytes = block_records * record_bytes;
    let jobs = layout.plan_reads(block_records as u64);

    let (free_sender, free_buffers) = mpsc::channel();
    for _ in 0..options.buffers.max(1) {
        free_sender
            .send(AlignedBuffer::new(block_bytes))
            .expect("the receiver is alive");
    }

    let finished = Mutex::new(Vec::with_capacity(jobs.len()));
    let mut read_time = Duration::ZERO;
    let mut unreadable_blocks = 0;
    let mut bytes_read = 0;

    rayon::in_place_scope(|scope| {
        for &job in &jobs {
            let mut buffer: AlignedBuffer = free_buffers.recv().expect("a sender is alive");
            let before = Instant::now();
            let result = layout.read_job(source, job, buffer.as_mut_slice());
            read_time += before.elapsed();
            if result.is_err() {
                unreadable_blocks += 1;
                free_sender.send(buffer).expect("the receiver is alive");
                continue;
            }
            bytes_read += u64::from(job.records) * record_bytes as u64;
            let free_sender = free_sender.clone();
            let (layout, finished) = (&layout, &finished);
            scope.spawn(move |_| {
                let mut parser = BlockParser::new(u64::from(geometry.bytes_per_cluster), job);
                parser.parse(buffer.as_mut_slice(), layout, record_bytes);
                finished
                    .lock()
                    .expect("no parser panicked")
                    .push((parser.chunk, parser.stats));
                let _ = free_sender.send(buffer);
            });
        }
    });

    let mut chunks = Vec::with_capacity(jobs.len());
    let mut stats = ScanStats {
        records_total: layout.record_count(),
        unreadable_blocks,
        bytes_read,
        ..ScanStats::default()
    };
    for (chunk, block_stats) in finished.into_inner().expect("no parser panicked") {
        stats.absorb(&block_stats);
        chunks.push(chunk);
    }
    Ok(ScanOutput {
        chunks,
        stats,
        mft_extents: layout.extent_count(),
        read_jobs: jobs.len(),
        read_time,
        total_time: started.elapsed(),
    })
}

struct BlockParser {
    cluster_bytes: u64,
    first_record: u64,
    chunk: Chunk,
    stats: ScanStats,
}

impl BlockParser {
    fn new(cluster_bytes: u64, job: ReadJob) -> Self {
        let records = job.records as usize;
        Self {
            cluster_bytes,
            first_record: job.first_record,
            chunk: Chunk {
                files: Vec::with_capacity(records),
                names: Vec::with_capacity(records),
                pool: Vec::with_capacity(records * 24),
            },
            stats: ScanStats::default(),
        }
    }

    fn parse(&mut self, block: &mut [u8], layout: &MftLayout, record_bytes: usize) {
        for (index, slot) in block.chunks_exact_mut(record_bytes).enumerate() {
            let number = self.first_record + index as u64;
            if !layout.is_free(number) {
                self.stats.records_parsed += 1;
                self.parse_record(slot, number);
            }
        }
    }

    fn parse_record(&mut self, slot: &mut [u8], number: u64) {
        if apply_fixups(slot).is_err() {
            self.stats.corrupt_records += 1;
            return;
        }
        let Ok(header) = RecordHeader::parse(slot) else {
            self.stats.corrupt_records += 1;
            return;
        };
        if !header.in_use() {
            self.stats.records_unused += 1;
            return;
        }
        let is_extension = header.base_record != 0;
        let owner = if is_extension {
            header.base_record
        } else {
            number
        };
        let Ok(record) = u32::try_from(owner) else {
            self.stats.corrupt_records += 1;
            return;
        };

        let mut file = RawFile {
            record,
            sequence: header.sequence,
            link_count: header.link_count,
            attributes: 0,
            flags: 0,
            logical_size: 0,
            stream_size: 0,
            allocated: 0,
        };
        let mut damaged = false;
        let mut attributes = Attributes::new(slot, &header);
        for attribute in attributes.by_ref() {
            let unnamed = attribute.name.is_empty();
            match attribute.body {
                AttributeBody::Resident(value) => match attribute.type_code {
                    ATTR_FILE_NAME => damaged |= !self.add_name(record, value),
                    ATTR_STANDARD_INFORMATION => {
                        file.attributes = standard_attributes(value).unwrap_or(0);
                    }
                    ATTR_DATA => add_stream(&mut file, unnamed, value.len() as u64),
                    _ => {}
                },
                AttributeBody::NonResident(extent) if extent.lowest_vcn == 0 => {
                    let on_disk = if number == BAD_CLUSTERS_RECORD {
                        self.clusters_in_use(&extent)
                    } else {
                        extent.on_disk_size()
                    };
                    file.allocated = file.allocated.saturating_add(on_disk);
                    if attribute.type_code == ATTR_DATA {
                        add_stream(&mut file, unnamed, extent.real_size);
                    }
                }
                AttributeBody::NonResident(_) => {}
            }
        }
        if damaged || attributes.malformed() {
            self.stats.malformed_records += 1;
        }

        if is_extension {
            file.flags |= RAW_EXTENSION;
            self.stats.extension_records += 1;
        } else {
            self.stats.base_records += 1;
            if header.is_directory() {
                file.flags |= RAW_DIRECTORY;
                self.stats.directories += 1;
            } else {
                self.stats.files += 1;
            }
        }
        self.stats.logical_bytes = self.stats.logical_bytes.saturating_add(file.logical_size);
        self.stats.stream_bytes = self.stats.stream_bytes.saturating_add(file.stream_size);
        self.stats.allocated_bytes = self.stats.allocated_bytes.saturating_add(file.allocated);
        self.chunk.files.push(file);
    }

    /// Records a name. Returns false when the attribute is unusable.
    fn add_name(&mut self, record: u32, value: &[u8]) -> bool {
        let Some(name) = FileName::parse(value) else {
            return false;
        };
        if name.namespace == NAMESPACE_DOS {
            return true;
        }
        let Ok(parent) = u32::try_from(name.parent_record) else {
            return false;
        };
        let start = self.chunk.pool.len();
        push_wtf8(&mut self.chunk.pool, name.name);
        let len = self.chunk.pool.len() - start;
        // A name has at most 255 UTF-16 units, so it always fits in u16.
        self.chunk.names.push(RawName {
            record,
            parent,
            parent_sequence: name.parent_sequence,
            len: len as u16,
            start: start as u32,
        });
        self.stats.names += 1;
        self.stats.name_bytes += len as u64;
        true
    }

    /// Sums the clusters a run list really occupies, ignoring holes.
    fn clusters_in_use(&self, extent: &NonResident<'_>) -> u64 {
        let clusters: u64 = Runs::new(extent.runs)
            .filter(|run| run.lcn.is_some())
            .map(|run| run.clusters)
            .fold(0, u64::saturating_add);
        clusters.saturating_mul(self.cluster_bytes)
    }
}

fn add_stream(file: &mut RawFile, unnamed: bool, bytes: u64) {
    if unnamed {
        file.logical_size = file.logical_size.saturating_add(bytes);
    } else {
        file.stream_size = file.stream_size.saturating_add(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::ATTR_DATA;
    use crate::synth::{
        AttrSpec, ImageLayout, NonResidentSpec, ROOT_RECORD, RecordSpec, TreeTotals, encode_runs,
        generate_tree, write_image,
    };
    use std::io::Cursor;

    const CLUSTER: u64 = 4096;

    fn layout(record_count: u64, fragments: u64) -> ImageLayout {
        ImageLayout {
            bytes_per_cluster: CLUSTER as u32,
            bytes_per_record: 1024,
            record_count,
            fragments,
        }
    }

    fn image(layout: &ImageLayout, records: Vec<(u64, RecordSpec)>) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        write_image(&mut out, layout, records).unwrap();
        out.into_inner()
    }

    fn run(bytes: &[u8]) -> ScanOutput {
        let geometry = Geometry::from_boot_sector(&bytes[..512]).unwrap();
        scan(bytes, &geometry, &ScanOptions::default()).unwrap()
    }

    /// The MFT's own record, which every image contains.
    fn mft_stats(layout: &ImageLayout) -> u64 {
        let mft = (layout.record_count * 1024).next_multiple_of(CLUSTER);
        mft + layout.record_count.div_ceil(8).next_multiple_of(CLUSTER)
    }

    fn names_of(output: &ScanOutput, record: u32) -> Vec<String> {
        let mut names = Vec::new();
        for chunk in &output.chunks {
            for name in chunk.names.iter().filter(|name| name.record == record) {
                names.push(String::from_utf8_lossy(chunk.name(name)).into_owned());
            }
        }
        names.sort();
        names
    }

    #[test]
    fn a_generated_tree_scans_back_to_its_totals() {
        for fragments in [1, 7, 64] {
            let layout = layout(20_000, fragments);
            let mut totals = TreeTotals::default();
            let records = generate_tree(layout.record_count, CLUSTER, &mut totals);
            let output = run(&image(&layout, records));
            let stats = output.stats;
            assert_eq!(stats.files, totals.files + 1, "the $MFT file is extra");
            assert_eq!(stats.directories, totals.directories);
            assert_eq!(stats.names, totals.names + 1);
            assert_eq!(stats.logical_bytes, totals.logical_bytes + 20_000 * 1024);
            assert_eq!(stats.stream_bytes, totals.stream_bytes);
            assert_eq!(
                stats.allocated_bytes,
                totals.allocated_bytes + mft_stats(&layout)
            );
            assert_eq!(stats.corrupt_records, 0);
            assert_eq!(stats.malformed_records, 0);
            assert_eq!(output.mft_extents as u64, fragments);
        }
    }

    #[test]
    fn dos_twins_are_dropped_and_long_names_kept() {
        let spec = RecordSpec::file(1)
            .attr(AttrSpec::name(ROOT_RECORD, 5, 2, "RELATO~1.TXT"))
            .attr(AttrSpec::name(ROOT_RECORD, 5, 1, "relatório final.txt"));
        let output = run(&image(&layout(64, 1), vec![(30, spec)]));
        assert_eq!(names_of(&output, 30), vec!["relatório final.txt"]);
    }

    #[test]
    fn hard_links_keep_every_name_but_count_their_size_once() {
        let spec = RecordSpec::file(1)
            .links(2)
            .attr(AttrSpec::name(ROOT_RECORD, 5, 3, "one"))
            .attr(AttrSpec::name(40, 1, 3, "two"))
            .attr(AttrSpec::non_resident_data(b"", 10_000, 12_288));
        let layout = layout(64, 1);
        let output = run(&image(&layout, vec![(30, spec)]));
        assert_eq!(names_of(&output, 30), vec!["one", "two"]);
        assert_eq!(output.stats.files, 2);
        assert_eq!(output.stats.allocated_bytes, 12_288 + mft_stats(&layout));
    }

    #[test]
    fn extension_records_credit_their_base() {
        let base = RecordSpec::file(1)
            .attr(AttrSpec::name(ROOT_RECORD, 5, 3, "big"))
            .attr(AttrSpec::non_resident_data(b"", 8192, 8192));
        let extension = RecordSpec::extension_of(30)
            .attr(AttrSpec::name(ROOT_RECORD, 5, 3, "other link"))
            .attr(AttrSpec::non_resident_data(b"S\0", 4096, 4096));
        let layout = layout(64, 1);
        let output = run(&image(&layout, vec![(30, base), (31, extension)]));
        let stats = output.stats;
        assert_eq!(stats.files, 2);
        assert_eq!(stats.extension_records, 1);
        assert_eq!(names_of(&output, 30), vec!["big", "other link"]);
        assert_eq!(stats.stream_bytes, 4096);
        assert_eq!(stats.allocated_bytes, 12_288 + mft_stats(&layout));
        let flagged = output.chunks[0]
            .files
            .iter()
            .find(|file| file.flags & RAW_EXTENSION != 0)
            .unwrap();
        assert_eq!(flagged.record, 30);
    }

    #[test]
    fn later_extents_of_a_stream_do_not_count_twice() {
        let extent = NonResidentSpec {
            type_code: ATTR_DATA,
            name: Vec::new(),
            flags: 0,
            lowest_vcn: 100,
            highest_vcn: 199,
            allocated: 0,
            real: 0,
            total_allocated: None,
            runs: encode_runs(&[(0x2000, 100)]),
        };
        let spec = RecordSpec::extension_of(30).attr(AttrSpec::NonResident(extent));
        let base = RecordSpec::file(1).attr(AttrSpec::non_resident_data(b"", 800_000, 819_200));
        let layout = layout(64, 1);
        let output = run(&image(&layout, vec![(30, base), (31, spec)]));
        assert_eq!(output.stats.allocated_bytes, 819_200 + mft_stats(&layout));
        assert_eq!(output.stats.logical_bytes, 800_000 + 64 * 1024);
    }

    #[test]
    fn the_bad_clusters_stream_counts_only_real_clusters() {
        // $Bad spans the whole volume as one hole plus two bad clusters.
        let runs = vec![0x03, 0x00, 0x00, 0x10, 0x11, 0x02, 0x30, 0x00];
        let bad = NonResidentSpec {
            type_code: ATTR_DATA,
            name: "$Bad".encode_utf16().flat_map(u16::to_le_bytes).collect(),
            flags: 0,
            lowest_vcn: 0,
            highest_vcn: 0x1001,
            allocated: 1 << 40,
            real: 1 << 40,
            total_allocated: None,
            runs,
        };
        let layout = layout(64, 1);
        let spec = RecordSpec::file(1).attr(AttrSpec::NonResident(bad));
        let output = run(&image(&layout, vec![(8, spec)]));
        assert_eq!(
            output.stats.allocated_bytes,
            2 * CLUSTER + mft_stats(&layout)
        );
    }

    #[test]
    fn damage_is_counted_and_never_fatal() {
        let layout = layout(256, 2);
        let records: Vec<(u64, RecordSpec)> = (30..60)
            .map(|number| {
                let name = format!("f{number}");
                let spec = RecordSpec::file(1).attr(AttrSpec::name(ROOT_RECORD, 5, 3, &name));
                (number, spec)
            })
            .collect();
        let mut bytes = image(&layout, records);
        let torn = layout.record_offset(33) as usize;
        bytes[torn + 510] ^= 0xFF;
        let scrambled = layout.record_offset(44) as usize;
        bytes[scrambled..scrambled + 4].copy_from_slice(b"BAAD");
        // Length of the first attribute, which sits right after the fixup array.
        let shredded = layout.record_offset(50) as usize;
        bytes[shredded + 0x3C..shredded + 0x40].fill(0xFF);

        let stats = run(&bytes).stats;
        assert_eq!(stats.corrupt_records, 2);
        assert_eq!(stats.malformed_records, 1);
        assert_eq!(stats.files, 29);
    }

    #[test]
    fn records_marked_free_are_ignored_even_if_they_look_valid() {
        let layout = layout(64, 1);
        let mut bytes = image(&layout, Vec::new());
        // A leftover record in a slot the bitmap says is free.
        let stale = RecordSpec::file(1)
            .attr(AttrSpec::name(ROOT_RECORD, 5, 3, "ghost"))
            .encode(1024);
        let offset = layout.record_offset(40) as usize;
        bytes[offset..offset + 1024].copy_from_slice(&stale);
        let stats = run(&bytes).stats;
        assert_eq!(stats.files, 1);
        assert_eq!(stats.records_parsed, 1);
    }

    #[test]
    fn deleted_records_are_not_files() {
        let spec = RecordSpec::file(1).deleted();
        let stats = run(&image(&layout(64, 1), vec![(30, spec)])).stats;
        assert_eq!(stats.records_unused, 1);
        assert_eq!(stats.files, 1);
    }

    #[test]
    fn a_small_block_size_changes_nothing() {
        let layout = layout(8_192, 3);
        let mut totals = TreeTotals::default();
        let bytes = image(
            &layout,
            generate_tree(layout.record_count, CLUSTER, &mut totals),
        );
        let geometry = Geometry::from_boot_sector(&bytes[..512]).unwrap();
        let options = ScanOptions {
            block_bytes: 1024,
            buffers: 2,
        };
        let small = scan(&bytes[..], &geometry, &options).unwrap().stats;
        let large = run(&bytes).stats;
        // Smaller blocks trim free records more tightly, so they read less.
        assert!(small.bytes_read <= large.bytes_read);
        assert_eq!(
            ScanStats {
                bytes_read: 0,
                ..small
            },
            ScanStats {
                bytes_read: 0,
                ..large
            }
        );
    }

    #[test]
    fn an_unreadable_block_is_counted_and_the_rest_survives() {
        struct Failing<'a>(&'a [u8], u64);
        impl BlockSource for Failing<'_> {
            fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> std::io::Result<()> {
                if offset >= self.1 {
                    return Err(std::io::ErrorKind::Other.into());
                }
                self.0.read_exact_at(offset, buffer)
            }
        }
        let layout = layout(8_192, 1);
        let mut totals = TreeTotals::default();
        let bytes = image(
            &layout,
            generate_tree(layout.record_count, CLUSTER, &mut totals),
        );
        let geometry = Geometry::from_boot_sector(&bytes[..512]).unwrap();
        let options = ScanOptions {
            block_bytes: 1 << 20,
            buffers: 2,
        };
        let cutoff = layout.record_offset(4096);
        let output = scan(&Failing(&bytes, cutoff), &geometry, &options).unwrap();
        assert!(output.stats.unreadable_blocks > 0);
        assert!(output.stats.files > 0 && output.stats.files < totals.files);
    }
}
