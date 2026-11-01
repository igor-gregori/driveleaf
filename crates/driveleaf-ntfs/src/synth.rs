//! Hand-encoded NTFS structures for tests and benchmarks.
//!
//! This is an encoder for just enough of the on-disk format to exercise the
//! parser without a real volume. It holds no file contents: a non-resident
//! stream only declares its sizes.

use std::io::{self, Seek, SeekFrom, Write};

use crate::record::{ATTR_BITMAP, ATTR_DATA, ATTR_FILE_NAME, ATTR_STANDARD_INFORMATION};

const FIXUP_STRIDE: usize = 512;
const HEADER_BYTES: usize = 0x30;
const FIXUP_GUARD: u16 = 0xA55A;
const ATTR_END: u32 = 0xFFFF_FFFF;
const FLAG_ATTR_SPARSE: u16 = 0x8000;
const FLAG_RECORD_IN_USE: u16 = 0x0001;
const FLAG_RECORD_DIRECTORY: u16 = 0x0002;

/// Record number of the root directory.
pub const ROOT_RECORD: u64 = 5;
const FIRST_USER_RECORD: u64 = 24;

/// One attribute to encode.
#[derive(Clone, Debug)]
pub enum AttrSpec {
    Resident {
        type_code: u32,
        name: Vec<u8>,
        value: Vec<u8>,
    },
    NonResident(NonResidentSpec),
}

/// A non-resident attribute extent.
#[derive(Clone, Debug)]
pub struct NonResidentSpec {
    pub type_code: u32,
    pub name: Vec<u8>,
    pub flags: u16,
    pub lowest_vcn: u64,
    pub highest_vcn: u64,
    pub allocated: u64,
    pub real: u64,
    pub total_allocated: Option<u64>,
    pub runs: Vec<u8>,
}

impl AttrSpec {
    /// `$STANDARD_INFORMATION` carrying only the file attribute flags.
    pub fn standard(attributes: u32) -> Self {
        let mut value = vec![0_u8; 72];
        value[0x20..0x24].copy_from_slice(&attributes.to_le_bytes());
        AttrSpec::Resident {
            type_code: ATTR_STANDARD_INFORMATION,
            name: Vec::new(),
            value,
        }
    }

    /// `$FILE_NAME` in the given namespace.
    pub fn name(parent: u64, parent_sequence: u16, namespace: u8, name: &str) -> Self {
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut value = vec![0_u8; 0x42];
        let reference = parent | (u64::from(parent_sequence) << 48);
        value[0..8].copy_from_slice(&reference.to_le_bytes());
        value[0x40] = units.len() as u8;
        value[0x41] = namespace;
        for unit in units {
            value.extend_from_slice(&unit.to_le_bytes());
        }
        AttrSpec::Resident {
            type_code: ATTR_FILE_NAME,
            name: Vec::new(),
            value,
        }
    }

    /// `$DATA` stored inside the record. `name` is raw UTF-16LE, empty for the
    /// main stream.
    pub fn resident_data(name: &[u8], value: &[u8]) -> Self {
        AttrSpec::Resident {
            type_code: ATTR_DATA,
            name: name.to_vec(),
            value: value.to_vec(),
        }
    }

    /// `$DATA` stored in clusters.
    pub fn non_resident_data(name: &[u8], real: u64, allocated: u64) -> Self {
        AttrSpec::NonResident(NonResidentSpec {
            type_code: ATTR_DATA,
            name: name.to_vec(),
            flags: 0,
            lowest_vcn: 0,
            highest_vcn: 0,
            allocated,
            real,
            total_allocated: None,
            runs: encode_runs(&[(0x1000, 1)]),
        })
    }

    /// Main `$DATA` of a sparse file.
    pub fn sparse_data(real: u64, allocated: u64, total_allocated: u64) -> Self {
        AttrSpec::NonResident(NonResidentSpec {
            type_code: ATTR_DATA,
            name: Vec::new(),
            flags: FLAG_ATTR_SPARSE,
            lowest_vcn: 0,
            highest_vcn: 0,
            allocated,
            real,
            total_allocated: Some(total_allocated),
            runs: encode_runs(&[(0x1000, 1)]),
        })
    }

    fn encoded_len(&self) -> usize {
        match self {
            AttrSpec::Resident { name, value, .. } => {
                align8(align8(0x18 + name.len()) + value.len())
            }
            AttrSpec::NonResident(spec) => {
                let header = if spec.total_allocated.is_some() {
                    0x48
                } else {
                    0x40
                };
                align8(header + spec.name.len() + spec.runs.len() + 1)
            }
        }
    }

    fn encode(&self, id: u16, out: &mut [u8]) {
        let length = self.encoded_len();
        out[4..8].copy_from_slice(&(length as u32).to_le_bytes());
        out[0x0E..0x10].copy_from_slice(&id.to_le_bytes());
        match self {
            AttrSpec::Resident {
                type_code,
                name,
                value,
            } => {
                out[..4].copy_from_slice(&type_code.to_le_bytes());
                out[9] = (name.len() / 2) as u8;
                out[0x0A..0x0C].copy_from_slice(&0x18_u16.to_le_bytes());
                let value_offset = align8(0x18 + name.len());
                out[0x10..0x14].copy_from_slice(&(value.len() as u32).to_le_bytes());
                out[0x14..0x16].copy_from_slice(&(value_offset as u16).to_le_bytes());
                out[0x18..0x18 + name.len()].copy_from_slice(name);
                out[value_offset..value_offset + value.len()].copy_from_slice(value);
            }
            AttrSpec::NonResident(spec) => {
                out[..4].copy_from_slice(&spec.type_code.to_le_bytes());
                out[8] = 1;
                out[9] = (spec.name.len() / 2) as u8;
                let name_offset = if spec.total_allocated.is_some() {
                    0x48
                } else {
                    0x40
                };
                let runs_offset = name_offset + spec.name.len();
                out[0x0A..0x0C].copy_from_slice(&(name_offset as u16).to_le_bytes());
                out[0x0C..0x0E].copy_from_slice(&spec.flags.to_le_bytes());
                out[0x10..0x18].copy_from_slice(&spec.lowest_vcn.to_le_bytes());
                out[0x18..0x20].copy_from_slice(&spec.highest_vcn.to_le_bytes());
                out[0x20..0x22].copy_from_slice(&(runs_offset as u16).to_le_bytes());
                out[0x28..0x30].copy_from_slice(&spec.allocated.to_le_bytes());
                out[0x30..0x38].copy_from_slice(&spec.real.to_le_bytes());
                out[0x38..0x40].copy_from_slice(&spec.real.to_le_bytes());
                if let Some(total) = spec.total_allocated {
                    out[0x40..0x48].copy_from_slice(&total.to_le_bytes());
                }
                out[name_offset..runs_offset].copy_from_slice(&spec.name);
                out[runs_offset..runs_offset + spec.runs.len()].copy_from_slice(&spec.runs);
            }
        }
    }
}

/// One FILE record to encode.
#[derive(Clone, Debug)]
pub struct RecordSpec {
    pub sequence: u16,
    pub flags: u16,
    pub link_count: u16,
    pub base_record: u64,
    pub attrs: Vec<AttrSpec>,
}

impl RecordSpec {
    pub fn file(sequence: u16) -> Self {
        Self {
            sequence,
            flags: FLAG_RECORD_IN_USE,
            link_count: 1,
            base_record: 0,
            attrs: Vec::new(),
        }
    }

    pub fn directory(sequence: u16) -> Self {
        Self {
            flags: FLAG_RECORD_IN_USE | FLAG_RECORD_DIRECTORY,
            ..Self::file(sequence)
        }
    }

    /// An extension record of `base`, which only continues its attributes.
    pub fn extension_of(base: u64) -> Self {
        Self {
            link_count: 0,
            base_record: base,
            ..Self::file(1)
        }
    }

    pub fn attr(mut self, attr: AttrSpec) -> Self {
        self.attrs.push(attr);
        self
    }

    pub fn links(mut self, count: u16) -> Self {
        self.link_count = count;
        self
    }

    /// A record slot whose in-use flag is off.
    pub fn deleted(mut self) -> Self {
        self.flags &= !FLAG_RECORD_IN_USE;
        self
    }

    /// Encodes the record with its update sequence applied, as stored on disk.
    pub fn encode(&self, record_bytes: usize) -> Vec<u8> {
        let mut record = vec![0_u8; record_bytes];
        let strides = record_bytes / FIXUP_STRIDE;
        let array_offset = HEADER_BYTES;
        let first_attribute = align8(array_offset + (strides + 1) * 2);

        let mut offset = first_attribute;
        for (id, attr) in self.attrs.iter().enumerate() {
            let length = attr.encoded_len();
            attr.encode(id as u16, &mut record[offset..offset + length]);
            offset += length;
        }
        record[offset..offset + 4].copy_from_slice(&ATTR_END.to_le_bytes());
        let bytes_used = offset + 8;

        record[..4].copy_from_slice(b"FILE");
        record[4..6].copy_from_slice(&(array_offset as u16).to_le_bytes());
        record[6..8].copy_from_slice(&((strides + 1) as u16).to_le_bytes());
        record[0x10..0x12].copy_from_slice(&self.sequence.to_le_bytes());
        record[0x12..0x14].copy_from_slice(&self.link_count.to_le_bytes());
        record[0x14..0x16].copy_from_slice(&(first_attribute as u16).to_le_bytes());
        record[0x16..0x18].copy_from_slice(&self.flags.to_le_bytes());
        record[0x18..0x1C].copy_from_slice(&(bytes_used as u32).to_le_bytes());
        record[0x1C..0x20].copy_from_slice(&(record_bytes as u32).to_le_bytes());
        record[0x20..0x28].copy_from_slice(&self.base_record.to_le_bytes());

        record[array_offset..array_offset + 2].copy_from_slice(&FIXUP_GUARD.to_le_bytes());
        for stride in 0..strides {
            let tail = (stride + 1) * FIXUP_STRIDE - 2;
            let saved = array_offset + 2 + stride * 2;
            record.copy_within(tail..tail + 2, saved);
            record[tail..tail + 2].copy_from_slice(&FIXUP_GUARD.to_le_bytes());
        }
        record
    }
}

/// Encodes `(absolute_lcn, clusters)` pairs as data runs.
pub fn encode_runs(runs: &[(u64, u64)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut previous = 0_i64;
    for &(lcn, clusters) in runs {
        let delta = lcn as i64 - previous;
        previous = lcn as i64;
        let length_bytes = unsigned_field(clusters);
        let offset_bytes = signed_field(delta);
        out.push(((offset_bytes.len() as u8) << 4) | length_bytes.len() as u8);
        out.extend_from_slice(&length_bytes);
        out.extend_from_slice(&offset_bytes);
    }
    out.push(0);
    out
}

fn unsigned_field(value: u64) -> Vec<u8> {
    let bytes = value.to_le_bytes();
    let used = 8 - (value.leading_zeros() as usize / 8);
    bytes[..used.max(1)].to_vec()
}

fn signed_field(value: i64) -> Vec<u8> {
    let bytes = value.to_le_bytes();
    let mut used = 8;
    while used > 1 {
        let top = bytes[used - 1];
        let next_sign = bytes[used - 2] & 0x80;
        if (top == 0 && next_sign == 0) || (top == 0xFF && next_sign != 0) {
            used -= 1;
        } else {
            break;
        }
    }
    bytes[..used].to_vec()
}

fn align8(value: usize) -> usize {
    value.next_multiple_of(8)
}

/// Shape of a synthetic volume image.
#[derive(Clone, Copy, Debug)]
pub struct ImageLayout {
    pub bytes_per_cluster: u32,
    pub bytes_per_record: u32,
    pub record_count: u64,
    /// Number of pieces the MFT is split into on disk.
    pub fragments: u64,
}

impl ImageLayout {
    const FIRST_MFT_LCN: u64 = 64;
    const GAP_CLUSTERS: u64 = 3;

    fn mft_clusters(&self) -> u64 {
        let bytes = self.record_count * u64::from(self.bytes_per_record);
        bytes.div_ceil(u64::from(self.bytes_per_cluster))
    }

    /// `(lcn, clusters)` of each MFT fragment.
    fn extents(&self) -> Vec<(u64, u64)> {
        let total = self.mft_clusters();
        let fragments = self.fragments.clamp(1, total.max(1));
        let each = total / fragments;
        let mut extents = Vec::with_capacity(fragments as usize);
        let mut lcn = Self::FIRST_MFT_LCN;
        let mut remaining = total;
        for index in 0..fragments {
            let clusters = if index + 1 == fragments {
                remaining
            } else {
                each
            };
            extents.push((lcn, clusters));
            lcn += clusters + Self::GAP_CLUSTERS;
            remaining -= clusters;
        }
        extents
    }

    fn bitmap_lcn(&self) -> u64 {
        let last = self.extents().last().copied().unwrap_or((0, 0));
        last.0 + last.1 + Self::GAP_CLUSTERS
    }

    fn bitmap_bytes(&self) -> u64 {
        self.record_count.div_ceil(8)
    }

    fn bitmap_clusters(&self) -> u64 {
        self.bitmap_bytes()
            .div_ceil(u64::from(self.bytes_per_cluster))
    }

    /// Byte offset of a record inside the image.
    pub fn record_offset(&self, number: u64) -> u64 {
        let target = number * u64::from(self.bytes_per_record);
        let cluster = u64::from(self.bytes_per_cluster);
        let mut skipped = 0;
        for (lcn, clusters) in self.extents() {
            let bytes = clusters * cluster;
            if target < skipped + bytes {
                return lcn * cluster + (target - skipped);
            }
            skipped += bytes;
        }
        panic!("record {number} is outside the image");
    }

    fn boot_sector(&self) -> Vec<u8> {
        let mut boot = vec![0_u8; 512];
        boot[3..11].copy_from_slice(b"NTFS    ");
        boot[0x0B..0x0D].copy_from_slice(&512_u16.to_le_bytes());
        boot[0x0D] = (self.bytes_per_cluster / 512) as u8;
        let clusters = self.bitmap_lcn() + self.bitmap_clusters() + 1;
        let sectors = clusters * u64::from(self.bytes_per_cluster / 512);
        boot[0x28..0x30].copy_from_slice(&sectors.to_le_bytes());
        boot[0x30..0x38].copy_from_slice(&Self::FIRST_MFT_LCN.to_le_bytes());
        boot[0x40] = (-(self.bytes_per_record.trailing_zeros() as i8)) as u8;
        boot
    }

    fn mft_record(&self) -> RecordSpec {
        let cluster = u64::from(self.bytes_per_cluster);
        let mft_bytes = self.mft_clusters() * cluster;
        let bitmap_clusters = self.bitmap_clusters();
        RecordSpec::file(1)
            .attr(AttrSpec::standard(0x06))
            .attr(AttrSpec::name(ROOT_RECORD, 5, 3, "$MFT"))
            .attr(AttrSpec::NonResident(NonResidentSpec {
                type_code: ATTR_DATA,
                name: Vec::new(),
                flags: 0,
                lowest_vcn: 0,
                highest_vcn: self.mft_clusters().saturating_sub(1),
                allocated: mft_bytes,
                real: self.record_count * u64::from(self.bytes_per_record),
                total_allocated: None,
                runs: encode_runs(&self.extents()),
            }))
            .attr(AttrSpec::NonResident(NonResidentSpec {
                type_code: ATTR_BITMAP,
                name: Vec::new(),
                flags: 0,
                lowest_vcn: 0,
                highest_vcn: bitmap_clusters.saturating_sub(1),
                allocated: bitmap_clusters * cluster,
                real: self.bitmap_bytes(),
                total_allocated: None,
                runs: encode_runs(&[(self.bitmap_lcn(), bitmap_clusters)]),
            }))
    }
}

/// Writes a volume image. Record 0 is generated here. `records` must arrive in
/// increasing record order.
pub fn write_image<W: Write + Seek>(
    out: &mut W,
    layout: &ImageLayout,
    records: impl IntoIterator<Item = (u64, RecordSpec)>,
) -> io::Result<()> {
    let record_bytes = layout.bytes_per_record as usize;
    let cluster = u64::from(layout.bytes_per_cluster);
    let mut bitmap = vec![0_u8; layout.bitmap_bytes() as usize];

    out.seek(SeekFrom::Start(0))?;
    out.write_all(&layout.boot_sector())?;

    let mut position = None;
    let mft_record = (0, layout.mft_record());
    for (number, spec) in std::iter::once(mft_record).chain(records) {
        bitmap[(number / 8) as usize] |= 1 << (number % 8);
        let offset = layout.record_offset(number);
        if position != Some(offset) {
            out.seek(SeekFrom::Start(offset))?;
        }
        out.write_all(&spec.encode(record_bytes))?;
        position = Some(offset + u64::from(layout.bytes_per_record));
    }

    out.seek(SeekFrom::Start(layout.bitmap_lcn() * cluster))?;
    out.write_all(&bitmap)?;
    let end = (layout.bitmap_lcn() + layout.bitmap_clusters() + 1) * cluster;
    out.seek(SeekFrom::Start(end - 1))?;
    out.write_all(&[0])?;
    out.flush()
}

/// Totals a generated tree must scan back to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TreeTotals {
    pub files: u64,
    pub directories: u64,
    pub names: u64,
    pub logical_bytes: u64,
    pub stream_bytes: u64,
    pub allocated_bytes: u64,
}

/// Deterministic disk-like records, for tests and benchmarks.
///
/// Mixes directories, resident and non-resident files, DOS twins, hard links,
/// alternate streams and extension records in plausible proportions. Yields up
/// to `record_count` record numbers, plus the totals of everything it produced
/// through `totals`.
pub fn generate_tree(
    record_count: u64,
    cluster_bytes: u64,
    totals: &mut TreeTotals,
) -> Vec<(u64, RecordSpec)> {
    let mut records = Vec::new();
    for_each_generated(record_count, cluster_bytes, totals, |number, spec| {
        records.push((number, spec));
    });
    records
}

/// Streaming form of [`generate_tree`], for images too large to hold in memory.
pub fn for_each_generated(
    record_count: u64,
    cluster_bytes: u64,
    totals: &mut TreeTotals,
    mut emit: impl FnMut(u64, RecordSpec),
) {
    let root = RecordSpec::directory(5)
        .attr(AttrSpec::standard(0x06))
        .attr(AttrSpec::name(ROOT_RECORD, 5, 3, "."));
    emit(ROOT_RECORD, root);
    totals.directories += 1;
    totals.names += 1;

    let mut parent = ROOT_RECORD;
    let mut number = FIRST_USER_RECORD;
    while number < record_count {
        // Weyl sequence: cheap, deterministic and well spread.
        let mix = number.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32;
        let pick = mix % 100;
        if number.is_multiple_of(13) {
            let spec = RecordSpec::directory(1)
                .attr(AttrSpec::standard(0x10))
                .attr(AttrSpec::name(parent, 1, 3, &format!("dir{number:x}")));
            emit(number, spec);
            totals.directories += 1;
            totals.names += 1;
            parent = number;
            number += 1;
            continue;
        }

        let name = format!("document-{number:x}.dat");
        let mut spec = RecordSpec::file(1).attr(AttrSpec::standard(0x20));
        if pick < 30 {
            spec = spec
                .attr(AttrSpec::name(parent, 1, 2, "DOCUME~1.DAT"))
                .attr(AttrSpec::name(parent, 1, 1, &name));
            totals.names += 1;
        } else {
            spec = spec.attr(AttrSpec::name(parent, 1, 3, &name));
            totals.names += 1;
        }
        if pick < 2 {
            spec = spec
                .attr(AttrSpec::name(
                    ROOT_RECORD,
                    5,
                    3,
                    &format!("link-{number:x}"),
                ))
                .links(2);
            totals.names += 1;
        }

        let real = 1 + (mix >> 8) % 4_000_000;
        if pick < 6 {
            let size = (mix % 300) as usize;
            spec = spec.attr(AttrSpec::resident_data(b"", &vec![0x42; size]));
            totals.logical_bytes += size as u64;
        } else {
            let allocated = real.next_multiple_of(cluster_bytes);
            if pick == 7 {
                let used = allocated / 2;
                spec = spec.attr(AttrSpec::sparse_data(real, allocated, used));
                totals.allocated_bytes += used;
            } else {
                spec = spec.attr(AttrSpec::non_resident_data(b"", real, allocated));
                totals.allocated_bytes += allocated;
            }
            totals.logical_bytes += real;
        }
        if pick == 8 {
            spec = spec.attr(AttrSpec::non_resident_data(
                b"Z\0o\0n\0e\0",
                5000,
                5000_u64.next_multiple_of(cluster_bytes),
            ));
            totals.stream_bytes += 5000;
            totals.allocated_bytes += 5000_u64.next_multiple_of(cluster_bytes);
        }
        emit(number, spec);
        totals.files += 1;

        if pick == 9 && number + 1 < record_count {
            let extension = RecordSpec::extension_of(number).attr(AttrSpec::non_resident_data(
                b"e\0x\0t\0",
                9000,
                9000_u64.next_multiple_of(cluster_bytes),
            ));
            number += 1;
            emit(number, extension);
            totals.stream_bytes += 9000;
            totals.allocated_bytes += 9000_u64.next_multiple_of(cluster_bytes);
        }
        number += 1;
    }
}
