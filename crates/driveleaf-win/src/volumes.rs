//! The drives on this computer.

use std::ffi::OsStr;
use std::ptr;

use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows_sys::Win32::System::WindowsProgramming::{
    DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
};

use crate::{from_wide, wide};

/// Where a drive lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Optical,
    Memory,
    Unknown,
}

/// One mounted drive.
#[derive(Clone, Debug)]
pub struct VolumeInfo {
    pub letter: char,
    pub label: String,
    pub file_system: String,
    pub kind: DriveKind,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

/// Lists the drives that are ready to use.
pub fn list_volumes() -> Vec<VolumeInfo> {
    // SAFETY: takes no arguments and only returns a bit mask.
    let mask = unsafe { GetLogicalDrives() };
    (0..26_u8)
        .filter(|bit| mask & (1 << bit) != 0)
        .filter_map(|bit| volume_info(char::from(b'A' + bit)))
        .collect()
}

fn volume_info(letter: char) -> Option<VolumeInfo> {
    let root_text = format!("{letter}:\\");
    let root = wide(OsStr::new(&root_text));

    // SAFETY: `root` is NUL-terminated and outlives the call.
    let kind = match unsafe { GetDriveTypeW(root.as_ptr()) } {
        DRIVE_FIXED => DriveKind::Fixed,
        DRIVE_REMOVABLE => DriveKind::Removable,
        DRIVE_REMOTE => DriveKind::Network,
        DRIVE_CDROM => DriveKind::Optical,
        DRIVE_RAMDISK => DriveKind::Memory,
        _ => DriveKind::Unknown,
    };

    let mut label = [0_u16; 261];
    let mut file_system = [0_u16; 261];
    // SAFETY: the pointers and lengths describe the two buffers, and the unused
    // outputs are allowed to be null.
    let succeeded = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            file_system.as_mut_ptr(),
            file_system.len() as u32,
        )
    };
    if succeeded == 0 {
        return None;
    }

    let (mut free_bytes, mut total_bytes) = (0_u64, 0_u64);
    // SAFETY: `root` is NUL-terminated, the outputs are valid, and the unused
    // one may be null.
    let succeeded = unsafe {
        GetDiskFreeSpaceExW(
            root.as_ptr(),
            &mut free_bytes,
            &mut total_bytes,
            ptr::null_mut(),
        )
    };
    if succeeded == 0 {
        return None;
    }

    Some(VolumeInfo {
        letter,
        label: from_wide(&label).to_string_lossy().into_owned(),
        file_system: from_wide(&file_system).to_string_lossy().into_owned(),
        kind,
        total_bytes,
        free_bytes,
    })
}
