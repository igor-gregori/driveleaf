//! File name conversion.

/// Appends a raw UTF-16LE name to `pool` as WTF-8.
///
/// NTFS names are arbitrary 16-bit units, so a name can hold an unpaired
/// surrogate. WTF-8 keeps those intact where UTF-8 could not, and stays
/// byte-identical to UTF-8 for every valid name.
pub(crate) fn push_wtf8(pool: &mut Vec<u8>, raw: &[u8]) {
    pool.reserve(raw.len() / 2);
    let (pairs, _) = raw.as_chunks::<2>();
    let mut units = pairs
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .peekable();
    while let Some(unit) = units.next() {
        let code = match (unit, units.peek()) {
            (0xD800..=0xDBFF, Some(&low @ 0xDC00..=0xDFFF)) => {
                units.next();
                0x1_0000 + ((u32::from(unit) - 0xD800) << 10) + (u32::from(low) - 0xDC00)
            }
            _ => u32::from(unit),
        };
        match code {
            0..=0x7F => pool.push(code as u8),
            0x80..=0x7FF => {
                pool.push(0xC0 | (code >> 6) as u8);
                pool.push(0x80 | (code & 0x3F) as u8);
            }
            0x800..=0xFFFF => {
                pool.push(0xE0 | (code >> 12) as u8);
                pool.push(0x80 | ((code >> 6) & 0x3F) as u8);
                pool.push(0x80 | (code & 0x3F) as u8);
            }
            _ => {
                pool.push(0xF0 | (code >> 18) as u8);
                pool.push(0x80 | ((code >> 12) & 0x3F) as u8);
                pool.push(0x80 | ((code >> 6) & 0x3F) as u8);
                pool.push(0x80 | (code & 0x3F) as u8);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(units: &[u16]) -> Vec<u8> {
        let raw: Vec<u8> = units.iter().flat_map(|unit| unit.to_le_bytes()).collect();
        let mut pool = Vec::new();
        push_wtf8(&mut pool, &raw);
        pool
    }

    #[test]
    fn valid_names_match_utf8() {
        for name in [
            "report.txt",
            "relatório",
            "写真.jpg",
            "emoji 🌿 leaf",
            "$MFT",
        ] {
            let units: Vec<u16> = name.encode_utf16().collect();
            assert_eq!(convert(&units), name.as_bytes(), "{name}");
        }
    }

    #[test]
    fn unpaired_surrogates_survive() {
        assert_eq!(
            convert(&[0x61, 0xD800, 0x62]),
            [0x61, 0xED, 0xA0, 0x80, 0x62]
        );
        assert_eq!(convert(&[0xDC00]), [0xED, 0xB0, 0x80]);
    }
}
