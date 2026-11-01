//! Volume geometry, from the boot sector or from the Windows volume API.

use crate::bytes::{le_u16, le_u64};
use crate::error::Error;

/// The numbers needed to find and slice the MFT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub bytes_per_sector: u32,
    pub bytes_per_cluster: u32,
    pub bytes_per_record: u32,
    /// First cluster of the MFT.
    pub mft_lcn: u64,
    pub total_clusters: u64,
    /// Known on a live volume; a boot sector does not carry it.
    pub free_clusters: Option<u64>,
}

impl Geometry {
    /// Decodes the NTFS boot sector found at the start of a volume.
    pub fn from_boot_sector(boot: &[u8]) -> Result<Self, Error> {
        if boot.len() < 512 {
            return Err(Error::NotNtfs("boot sector too short"));
        }
        if &boot[3..11] != b"NTFS    " {
            return Err(Error::NotNtfs("missing NTFS signature"));
        }
        let bytes_per_sector =
            u32::from(le_u16(boot, 0x0B).ok_or(Error::NotNtfs("no sector size"))?);
        if !bytes_per_sector.is_power_of_two() || !(512..=4096).contains(&bytes_per_sector) {
            return Err(Error::NotNtfs("implausible sector size"));
        }
        let sectors_per_cluster = decode_sectors_per_cluster(boot[0x0D])?;
        let bytes_per_cluster = bytes_per_sector
            .checked_mul(sectors_per_cluster)
            .ok_or(Error::NotNtfs("implausible cluster size"))?;
        let total_sectors = le_u64(boot, 0x28).ok_or(Error::NotNtfs("no sector count"))?;
        let mft_lcn = le_u64(boot, 0x30).ok_or(Error::NotNtfs("no MFT location"))?;
        let bytes_per_record = decode_record_size(boot[0x40], bytes_per_cluster)?;
        Ok(Self {
            bytes_per_sector,
            bytes_per_cluster,
            bytes_per_record,
            mft_lcn,
            total_clusters: total_sectors / u64::from(sectors_per_cluster),
            free_clusters: None,
        })
    }

    /// Byte offset of the MFT from the start of the volume.
    pub fn mft_offset(&self) -> Option<u64> {
        self.mft_lcn.checked_mul(u64::from(self.bytes_per_cluster))
    }
}

/// Values above 0x80 mean a power of two: 0xF8 is 2^8 = 256 sectors.
fn decode_sectors_per_cluster(raw: u8) -> Result<u32, Error> {
    let sectors = if raw <= 0x80 {
        u32::from(raw)
    } else {
        1_u32
            .checked_shl(u32::from(raw.wrapping_neg()))
            .ok_or(Error::NotNtfs("implausible cluster size"))?
    };
    if sectors == 0 || !sectors.is_power_of_two() {
        return Err(Error::NotNtfs("implausible cluster size"));
    }
    Ok(sectors)
}

/// A positive value counts clusters; a negative one means `2^-value` bytes.
fn decode_record_size(raw: u8, bytes_per_cluster: u32) -> Result<u32, Error> {
    let signed = raw as i8;
    let size = if signed > 0 {
        bytes_per_cluster.checked_mul(u32::from(raw))
    } else {
        1_u32.checked_shl(u32::from(signed.unsigned_abs()))
    };
    match size {
        Some(size) if size.is_power_of_two() && (512..=65_536).contains(&size) => Ok(size),
        _ => Err(Error::NotNtfs("implausible record size")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boot_sector() -> Vec<u8> {
        let mut boot = vec![0_u8; 512];
        boot[3..11].copy_from_slice(b"NTFS    ");
        boot[0x0B..0x0D].copy_from_slice(&512_u16.to_le_bytes());
        boot[0x0D] = 8;
        boot[0x28..0x30].copy_from_slice(&1_000_000_u64.to_le_bytes());
        boot[0x30..0x38].copy_from_slice(&786_432_u64.to_le_bytes());
        boot[0x40] = 0xF6; // -10: 1024 byte records
        boot
    }

    #[test]
    fn decodes_a_typical_volume() {
        let geometry = Geometry::from_boot_sector(&boot_sector()).unwrap();
        assert_eq!(geometry.bytes_per_sector, 512);
        assert_eq!(geometry.bytes_per_cluster, 4096);
        assert_eq!(geometry.bytes_per_record, 1024);
        assert_eq!(geometry.mft_lcn, 786_432);
        assert_eq!(geometry.total_clusters, 125_000);
        assert_eq!(geometry.mft_offset(), Some(786_432 * 4096));
    }

    #[test]
    fn decodes_large_clusters_written_as_a_power_of_two() {
        let mut boot = boot_sector();
        boot[0x0D] = 0xF8; // -8: 256 sectors, 128 KiB clusters
        let geometry = Geometry::from_boot_sector(&boot).unwrap();
        assert_eq!(geometry.bytes_per_cluster, 128 * 1024);
    }

    #[test]
    fn rejects_garbage() {
        assert!(Geometry::from_boot_sector(&[0_u8; 16]).is_err());
        assert!(Geometry::from_boot_sector(&[0_u8; 512]).is_err());
        let mut boot = boot_sector();
        boot[0x0B..0x0D].copy_from_slice(&513_u16.to_le_bytes());
        assert!(Geometry::from_boot_sector(&boot).is_err());
        let mut boot = boot_sector();
        boot[0x40] = 0;
        assert!(Geometry::from_boot_sector(&boot).is_err());
    }
}
