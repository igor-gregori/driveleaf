//! FILE record decoding: update sequence fixups, header and attribute walking.
//!
//! Written from the public NTFS on-disk format documentation. Nothing here
//! trusts the input: every offset is checked, and a malformed record only ever
//! yields an error or a shorter attribute list.

use crate::bytes::{le_u16, le_u32, le_u64};

pub const ATTR_STANDARD_INFORMATION: u32 = 0x10;
pub const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
pub const ATTR_FILE_NAME: u32 = 0x30;
pub const ATTR_DATA: u32 = 0x80;
pub const ATTR_BITMAP: u32 = 0xB0;
pub const ATTR_REPARSE_POINT: u32 = 0xC0;
const ATTR_END: u32 = 0xFFFF_FFFF;

const FLAG_ATTR_COMPRESSED: u16 = 0x0001;
const FLAG_ATTR_SPARSE: u16 = 0x8000;

const FLAG_RECORD_IN_USE: u16 = 0x0001;
const FLAG_RECORD_DIRECTORY: u16 = 0x0002;

/// The update sequence protects the last two bytes of every 512-byte stride,
/// whatever the physical sector size is.
const FIXUP_STRIDE: usize = 512;

/// Size of the fixed record header up to the fields this parser reads.
const RECORD_HEADER_BYTES: usize = 0x30;

/// Low 48 bits of a file reference are the record number; the high 16 are its
/// sequence number.
pub const RECORD_NUMBER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Why a record slot could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordError {
    /// The slot does not start with `FILE`.
    BadSignature,
    /// The update sequence array is out of bounds.
    BadFixupArray,
    /// A protected sector tail does not match: a torn or stale write.
    TornWrite,
    /// The header fields are inconsistent.
    BadHeader,
}

/// Checks the signature and restores the bytes hidden by the update sequence.
pub fn apply_fixups(record: &mut [u8]) -> Result<(), RecordError> {
    if record.get(..4) != Some(b"FILE") {
        return Err(RecordError::BadSignature);
    }
    let array_offset = usize::from(le_u16(record, 4).ok_or(RecordError::BadFixupArray)?);
    let array_len = usize::from(le_u16(record, 6).ok_or(RecordError::BadFixupArray)?);
    if array_len == 0 || array_offset < 8 {
        return Err(RecordError::BadFixupArray);
    }
    let strides = array_len - 1;
    let array_end = array_offset + array_len * 2;
    if array_end > record.len() || strides * FIXUP_STRIDE > record.len() {
        return Err(RecordError::BadFixupArray);
    }
    let guard = [record[array_offset], record[array_offset + 1]];
    for stride in 0..strides {
        let tail = (stride + 1) * FIXUP_STRIDE - 2;
        if record[tail..tail + 2] != guard {
            return Err(RecordError::TornWrite);
        }
        let saved = array_offset + 2 + stride * 2;
        record[tail] = record[saved];
        record[tail + 1] = record[saved + 1];
    }
    Ok(())
}

/// Decoded fixed part of a FILE record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordHeader {
    pub sequence: u16,
    pub link_count: u16,
    pub first_attribute: u16,
    pub flags: u16,
    pub bytes_used: u32,
    /// Record number of the base record, zero when this is a base record.
    pub base_record: u64,
}

impl RecordHeader {
    pub fn parse(record: &[u8]) -> Result<Self, RecordError> {
        if record.len() < RECORD_HEADER_BYTES {
            return Err(RecordError::BadHeader);
        }
        let read = || -> Option<Self> {
            Some(Self {
                sequence: le_u16(record, 0x10)?,
                link_count: le_u16(record, 0x12)?,
                first_attribute: le_u16(record, 0x14)?,
                flags: le_u16(record, 0x16)?,
                bytes_used: le_u32(record, 0x18)?,
                base_record: le_u64(record, 0x20)? & RECORD_NUMBER_MASK,
            })
        };
        let header = read().ok_or(RecordError::BadHeader)?;
        if usize::from(header.first_attribute) < RECORD_HEADER_BYTES
            || header.bytes_used as usize > record.len()
        {
            return Err(RecordError::BadHeader);
        }
        Ok(header)
    }

    pub fn in_use(&self) -> bool {
        self.flags & FLAG_RECORD_IN_USE != 0
    }

    pub fn is_directory(&self) -> bool {
        self.flags & FLAG_RECORD_DIRECTORY != 0
    }
}

/// One attribute of a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attribute<'a> {
    pub type_code: u32,
    /// Attribute name as raw UTF-16LE; empty for the unnamed stream.
    pub name: &'a [u8],
    pub body: AttributeBody<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributeBody<'a> {
    Resident(&'a [u8]),
    NonResident(NonResident<'a>),
}

/// Header of a non-resident attribute extent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NonResident<'a> {
    pub lowest_vcn: u64,
    pub runs: &'a [u8],
    /// Clusters reserved for the stream, as bytes.
    pub allocated_size: u64,
    pub real_size: u64,
    /// Clusters actually used, as bytes. Present only on the first extent of
    /// compressed or sparse streams, where it is less than `allocated_size`.
    pub total_allocated: Option<u64>,
}

impl NonResident<'_> {
    /// Bytes this stream really occupies on disk.
    pub fn on_disk_size(&self) -> u64 {
        self.total_allocated.unwrap_or(self.allocated_size)
    }
}

/// Walks the attributes of a record. Stops at the end marker or at the first
/// malformed attribute; check [`Attributes::malformed`] when done.
pub struct Attributes<'a> {
    record: &'a [u8],
    offset: usize,
    limit: usize,
    malformed: bool,
}

impl<'a> Attributes<'a> {
    pub fn new(record: &'a [u8], header: &RecordHeader) -> Self {
        Self {
            record,
            offset: usize::from(header.first_attribute),
            limit: (header.bytes_used as usize).min(record.len()),
            malformed: false,
        }
    }

    pub fn malformed(&self) -> bool {
        self.malformed
    }

    fn fail(&mut self) -> Option<Attribute<'a>> {
        self.malformed = true;
        None
    }
}

impl<'a> Iterator for Attributes<'a> {
    type Item = Attribute<'a>;

    fn next(&mut self) -> Option<Attribute<'a>> {
        let scope = self.record.get(..self.limit)?;
        let Some(type_code) = le_u32(scope, self.offset) else {
            return self.fail();
        };
        if type_code == ATTR_END {
            return None;
        }
        let Some(length) = le_u32(scope, self.offset + 4) else {
            return self.fail();
        };
        let length = length as usize;
        let Some(end) = self.offset.checked_add(length) else {
            return self.fail();
        };
        if length < 0x18 || end > scope.len() {
            return self.fail();
        }
        let raw = &scope[self.offset..end];
        self.offset = end;
        match decode_attribute(type_code, raw) {
            Some(attribute) => Some(attribute),
            None => self.fail(),
        }
    }
}

fn decode_attribute(type_code: u32, raw: &[u8]) -> Option<Attribute<'_>> {
    let non_resident = *raw.get(8)?;
    let name_chars = usize::from(*raw.get(9)?);
    let name_offset = usize::from(le_u16(raw, 0x0A)?);
    let flags = le_u16(raw, 0x0C)?;
    let name = raw.get(name_offset..name_offset + name_chars * 2)?;
    let body = match non_resident {
        0 => {
            let value_len = le_u32(raw, 0x10)? as usize;
            let value_offset = usize::from(le_u16(raw, 0x14)?);
            AttributeBody::Resident(raw.get(value_offset..value_offset.checked_add(value_len)?)?)
        }
        1 => {
            let lowest_vcn = le_u64(raw, 0x10)?;
            let runs_offset = usize::from(le_u16(raw, 0x20)?);
            let is_packed = flags & (FLAG_ATTR_COMPRESSED | FLAG_ATTR_SPARSE) != 0;
            let total_allocated = if is_packed && lowest_vcn == 0 {
                Some(le_u64(raw, 0x40)?)
            } else {
                None
            };
            AttributeBody::NonResident(NonResident {
                lowest_vcn,
                runs: raw.get(runs_offset..)?,
                allocated_size: le_u64(raw, 0x28)?,
                real_size: le_u64(raw, 0x30)?,
                total_allocated,
            })
        }
        _ => return None,
    };
    Some(Attribute {
        type_code,
        name,
        body,
    })
}

/// Decoded `$FILE_NAME` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileName<'a> {
    pub parent_record: u64,
    pub parent_sequence: u16,
    pub namespace: u8,
    /// Name as raw UTF-16LE.
    pub name: &'a [u8],
}

/// The DOS 8.3 twin of a long name, which never needs its own tree entry.
pub const NAMESPACE_DOS: u8 = 2;

impl<'a> FileName<'a> {
    pub fn parse(value: &'a [u8]) -> Option<Self> {
        let parent = le_u64(value, 0)?;
        let name_chars = usize::from(*value.get(0x40)?);
        let namespace = *value.get(0x41)?;
        let name = value.get(0x42..0x42 + name_chars * 2)?;
        Some(Self {
            parent_record: parent & RECORD_NUMBER_MASK,
            parent_sequence: (parent >> 48) as u16,
            namespace,
            name,
        })
    }
}

/// Decoded `$STANDARD_INFORMATION` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StandardInfo {
    /// Last modification, in 100 ns ticks since 1601.
    pub modified: u64,
    pub attributes: u32,
}

impl StandardInfo {
    pub fn parse(value: &[u8]) -> Option<Self> {
        Some(Self {
            modified: le_u64(value, 0x08)?,
            attributes: le_u32(value, 0x20)?,
        })
    }
}

/// One entry of an `$ATTRIBUTE_LIST`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListEntry {
    pub type_code: u32,
    pub lowest_vcn: u64,
    pub record: u64,
    pub unnamed: bool,
}

/// Walks an `$ATTRIBUTE_LIST` value, stopping at the first malformed entry.
pub fn attribute_list(value: &[u8]) -> impl Iterator<Item = ListEntry> + '_ {
    let mut offset = 0_usize;
    std::iter::from_fn(move || {
        let type_code = le_u32(value, offset)?;
        let length = usize::from(le_u16(value, offset + 4)?);
        if length < 0x1A {
            return None;
        }
        let name_chars = *value.get(offset + 6)?;
        let lowest_vcn = le_u64(value, offset + 8)?;
        let record = le_u64(value, offset + 0x10)? & RECORD_NUMBER_MASK;
        offset = offset.checked_add(length)?;
        Some(ListEntry {
            type_code,
            lowest_vcn,
            record,
            unnamed: name_chars == 0,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{AttrSpec, RecordSpec};

    const RECORD_BYTES: usize = 1024;

    fn encoded(spec: &RecordSpec) -> Vec<u8> {
        spec.encode(RECORD_BYTES)
    }

    #[test]
    fn fixups_restore_the_protected_bytes() {
        let spec = RecordSpec::file(40).attr(AttrSpec::resident_data(b"", &[0xAB; 600]));
        let mut record = encoded(&spec);
        // The encoder stamps the guard over the data at the end of the stride.
        assert_ne!(&record[510..512], &[0xAB, 0xAB]);
        apply_fixups(&mut record).unwrap();
        assert_eq!(&record[510..512], &[0xAB, 0xAB]);
    }

    #[test]
    fn a_torn_write_is_detected() {
        let mut record = encoded(&RecordSpec::file(40));
        record[1022] ^= 0xFF;
        assert_eq!(apply_fixups(&mut record), Err(RecordError::TornWrite));
    }

    #[test]
    fn bad_signature_and_fixup_arrays_are_rejected() {
        let mut record = encoded(&RecordSpec::file(40));
        record[0] = b'B';
        assert_eq!(apply_fixups(&mut record), Err(RecordError::BadSignature));

        let mut record = encoded(&RecordSpec::file(40));
        record[4..6].copy_from_slice(&1020_u16.to_le_bytes());
        assert_eq!(apply_fixups(&mut record), Err(RecordError::BadFixupArray));

        let mut record = encoded(&RecordSpec::file(40));
        record[6..8].copy_from_slice(&0_u16.to_le_bytes());
        assert_eq!(apply_fixups(&mut record), Err(RecordError::BadFixupArray));
    }

    #[test]
    fn walks_resident_and_non_resident_attributes() {
        let spec = RecordSpec::file(40)
            .attr(AttrSpec::name(5, 1, 1, "report.txt"))
            .attr(AttrSpec::resident_data(b"", &[1, 2, 3]))
            .attr(AttrSpec::non_resident_data(b"S\0", 10_000, 12_288));
        let mut record = encoded(&spec);
        apply_fixups(&mut record).unwrap();
        let header = RecordHeader::parse(&record).unwrap();
        assert!(header.in_use());
        assert!(!header.is_directory());

        let mut attributes = Attributes::new(&record, &header);
        let name = attributes.next().unwrap();
        assert_eq!(name.type_code, ATTR_FILE_NAME);
        let AttributeBody::Resident(value) = name.body else {
            panic!("expected a resident name");
        };
        let parsed = FileName::parse(value).unwrap();
        assert_eq!(parsed.parent_record, 5);
        assert_eq!(parsed.parent_sequence, 1);
        assert_eq!(parsed.namespace, 1);
        assert_eq!(parsed.name.len(), 20);

        let data = attributes.next().unwrap();
        assert_eq!(data.body, AttributeBody::Resident(&[1, 2, 3]));

        let stream = attributes.next().unwrap();
        assert_eq!(stream.name, b"S\0");
        let AttributeBody::NonResident(stream) = stream.body else {
            panic!("expected a non-resident stream");
        };
        assert_eq!(stream.real_size, 10_000);
        assert_eq!(stream.on_disk_size(), 12_288);

        assert!(attributes.next().is_none());
        assert!(!attributes.malformed());
    }

    #[test]
    fn sparse_streams_report_what_they_really_use() {
        let spec = RecordSpec::file(40).attr(AttrSpec::sparse_data(1 << 40, 1 << 40, 8192));
        let mut record = encoded(&spec);
        apply_fixups(&mut record).unwrap();
        let header = RecordHeader::parse(&record).unwrap();
        let attribute = Attributes::new(&record, &header).next().unwrap();
        let AttributeBody::NonResident(stream) = attribute.body else {
            panic!("expected a non-resident stream");
        };
        assert_eq!(stream.on_disk_size(), 8192);
    }

    #[test]
    fn a_broken_attribute_ends_the_walk_without_panicking() {
        let spec = RecordSpec::file(40).attr(AttrSpec::resident_data(b"", &[1, 2, 3]));
        let mut record = encoded(&spec);
        apply_fixups(&mut record).unwrap();
        let header = RecordHeader::parse(&record).unwrap();
        let first = usize::from(header.first_attribute);
        record[first + 4..first + 8].copy_from_slice(&u32::MAX.to_le_bytes());
        let mut attributes = Attributes::new(&record, &header);
        assert!(attributes.next().is_none());
        assert!(attributes.malformed());
    }

    #[test]
    fn inconsistent_headers_are_rejected() {
        let mut record = encoded(&RecordSpec::file(40));
        apply_fixups(&mut record).unwrap();
        record[0x18..0x1C].copy_from_slice(&5000_u32.to_le_bytes());
        assert_eq!(RecordHeader::parse(&record), Err(RecordError::BadHeader));
    }

    #[test]
    fn attribute_lists_decode_and_stop_on_garbage() {
        let mut value = Vec::new();
        for (type_code, record) in [(ATTR_FILE_NAME, 7_u64), (ATTR_DATA, 90)] {
            value.extend_from_slice(&type_code.to_le_bytes());
            value.extend_from_slice(&32_u16.to_le_bytes());
            value.push(0);
            value.push(0x1A);
            value.extend_from_slice(&0_u64.to_le_bytes());
            value.extend_from_slice(&(record | (3 << 48)).to_le_bytes());
            value.extend_from_slice(&[0; 8]);
        }
        let entries: Vec<ListEntry> = attribute_list(&value).collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].type_code, ATTR_DATA);
        assert_eq!(entries[1].record, 90);
        assert!(entries[1].unnamed);

        value[4..6].copy_from_slice(&2_u16.to_le_bytes());
        assert_eq!(attribute_list(&value).count(), 0);
    }
}
