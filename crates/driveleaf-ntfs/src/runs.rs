//! Data run ("mapping pairs") decoding.

/// One stretch of an attribute's data. `lcn` is `None` for a sparse hole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub lcn: Option<u64>,
    pub clusters: u64,
}

/// Iterates over encoded data runs, stopping at the terminator or at the first
/// malformed entry. Check [`Runs::malformed`] once the iterator is exhausted.
pub struct Runs<'a> {
    bytes: &'a [u8],
    offset: usize,
    lcn: i64,
    malformed: bool,
}

impl<'a> Runs<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            offset: 0,
            lcn: 0,
            malformed: false,
        }
    }

    pub fn malformed(&self) -> bool {
        self.malformed
    }

    fn next_run(&mut self) -> Option<Run> {
        let header = *self.bytes.get(self.offset)?;
        if header == 0 {
            return None;
        }
        let length_size = usize::from(header & 0x0F);
        let offset_size = usize::from(header >> 4);
        let start = self.offset + 1;
        let length_end = start + length_size;
        let end = length_end + offset_size;
        let (Some(length_field), Some(offset_field)) = (
            self.bytes.get(start..length_end),
            self.bytes.get(length_end..end),
        ) else {
            self.malformed = true;
            return None;
        };
        if length_size == 0 || length_size > 8 || offset_size > 8 {
            self.malformed = true;
            return None;
        }
        let clusters = read_unsigned(length_field);
        if clusters == 0 || clusters > i64::MAX as u64 {
            self.malformed = true;
            return None;
        }
        self.offset = end;
        if offset_size == 0 {
            return Some(Run {
                lcn: None,
                clusters,
            });
        }
        match self.lcn.checked_add(read_signed(offset_field)) {
            Some(lcn) if lcn >= 0 => {
                self.lcn = lcn;
                Some(Run {
                    lcn: Some(lcn as u64),
                    clusters,
                })
            }
            _ => {
                self.malformed = true;
                None
            }
        }
    }
}

impl Iterator for Runs<'_> {
    type Item = Run;

    fn next(&mut self) -> Option<Run> {
        self.next_run()
    }
}

fn read_unsigned(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .rev()
        .fold(0_u64, |value, &byte| (value << 8) | u64::from(byte))
}

fn read_signed(bytes: &[u8]) -> i64 {
    let unsigned = read_unsigned(bytes);
    let unused_bits = 64 - (bytes.len() as u32) * 8;
    if unused_bits >= 64 {
        return 0;
    }
    ((unsigned << unused_bits) as i64) >> unused_bits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> (Vec<Run>, bool) {
        let mut runs = Runs::new(bytes);
        let list: Vec<Run> = runs.by_ref().collect();
        (list, runs.malformed())
    }

    fn at(lcn: u64, clusters: u64) -> Run {
        Run {
            lcn: Some(lcn),
            clusters,
        }
    }

    #[test]
    fn decodes_a_single_run() {
        // 0x18 clusters at LCN 0x5634.
        let (runs, malformed) = decode(&[0x21, 0x18, 0x34, 0x56, 0x00]);
        assert_eq!(runs, vec![at(0x5634, 0x18)]);
        assert!(!malformed);
    }

    #[test]
    fn offsets_are_relative_and_signed() {
        // Forward to 0x1000, then back by 0x100.
        let (runs, _) = decode(&[0x21, 0x10, 0x00, 0x10, 0x21, 0x10, 0x00, 0xFF, 0x00]);
        assert_eq!(runs, vec![at(0x1000, 0x10), at(0x0F00, 0x10)]);
    }

    #[test]
    fn a_zero_offset_size_is_a_sparse_hole() {
        let (runs, _) = decode(&[0x11, 0x08, 0x20, 0x01, 0x40, 0x11, 0x08, 0x10, 0x00]);
        assert_eq!(
            runs,
            vec![
                at(0x20, 0x08),
                Run {
                    lcn: None,
                    clusters: 0x40
                },
                at(0x30, 0x08),
            ]
        );
    }

    #[test]
    fn truncation_is_reported_and_keeps_the_good_prefix() {
        let (runs, malformed) = decode(&[0x11, 0x08, 0x20, 0x21, 0x08]);
        assert_eq!(runs, vec![at(0x20, 0x08)]);
        assert!(malformed);
    }

    #[test]
    fn an_unterminated_list_ends_without_a_malformed_flag() {
        let (runs, malformed) = decode(&[0x11, 0x08, 0x20]);
        assert_eq!(runs, vec![at(0x20, 0x08)]);
        assert!(!malformed);
    }

    #[test]
    fn negative_absolute_lcn_is_rejected() {
        let (runs, malformed) = decode(&[0x11, 0x08, 0xF0, 0x00]);
        assert!(runs.is_empty());
        assert!(malformed);
    }

    #[test]
    fn oversized_fields_and_zero_lengths_are_rejected() {
        assert!(decode(&[0x09, 0, 0, 0, 0, 0, 0, 0, 0, 0]).1);
        assert!(decode(&[0x01, 0x00, 0x00]).1);
        assert!(decode(&[0x10, 0x05, 0x00]).1);
    }
}
