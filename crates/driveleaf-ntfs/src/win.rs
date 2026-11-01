//! Live volume access on Windows. The only `unsafe` in this crate lives here.

#![allow(unsafe_code)]

use std::io;
use std::mem;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_NO_BUFFERING, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING, ReadFile,
};
use windows_sys::Win32::System::IO::{DeviceIoControl, OVERLAPPED};
use windows_sys::Win32::System::Ioctl::{FSCTL_GET_NTFS_VOLUME_DATA, NTFS_VOLUME_DATA_BUFFER};
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::geometry::Geometry;
use crate::source::BlockSource;

/// An open `\\.\X:` volume. Reads bypass the file cache, so they always measure
/// and return what is on disk. Opening it requires administrator rights.
pub struct Volume {
    handle: HANDLE,
}

// SAFETY: the handle is only passed to kernel calls that are safe to issue from
// any thread, and it stays valid until `Drop`.
unsafe impl Send for Volume {}
unsafe impl Sync for Volume {}

impl Volume {
    /// Opens the volume with the given drive letter.
    pub fn open(letter: char) -> io::Result<Self> {
        if !letter.is_ascii_alphabetic() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let path: Vec<u16> = format!(r"\\.\{letter}:")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: `path` is NUL-terminated and outlives the call.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_NO_BUFFERING,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { handle })
    }

    /// Asks NTFS for the volume geometry. Fails on any other file system.
    pub fn geometry(&self) -> io::Result<Geometry> {
        // SAFETY: the buffer is plain integers, for which all zeroes is valid.
        let mut data: NTFS_VOLUME_DATA_BUFFER = unsafe { mem::zeroed() };
        let mut returned = 0_u32;
        // SAFETY: the output pointer and its size describe `data`, which
        // outlives the synchronous call.
        let succeeded = unsafe {
            DeviceIoControl(
                self.handle,
                FSCTL_GET_NTFS_VOLUME_DATA,
                ptr::null(),
                0,
                (&raw mut data).cast(),
                size_of::<NTFS_VOLUME_DATA_BUFFER>() as u32,
                &mut returned,
                ptr::null_mut(),
            )
        };
        if succeeded == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Geometry {
            bytes_per_sector: data.BytesPerSector,
            bytes_per_cluster: data.BytesPerCluster,
            bytes_per_record: data.BytesPerFileRecordSegment,
            mft_lcn: data.MftStartLcn as u64,
            total_clusters: data.TotalClusters as u64,
            free_clusters: Some(data.FreeClusters as u64),
        })
    }
}

impl BlockSource for Volume {
    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        let mut done = 0;
        while done < buffer.len() {
            let position = offset + done as u64;
            // SAFETY: OVERLAPPED is plain data, for which all zeroes is valid.
            let mut overlapped: OVERLAPPED = unsafe { mem::zeroed() };
            overlapped.Anonymous.Anonymous.Offset = position as u32;
            overlapped.Anonymous.Anonymous.OffsetHigh = (position >> 32) as u32;
            let wanted = u32::try_from(buffer.len() - done).unwrap_or(u32::MAX);
            let mut read = 0_u32;
            // SAFETY: the pointer and length stay inside `buffer`. On a
            // synchronous handle the call completes before returning, so
            // `overlapped` and `read` outlive it.
            let succeeded = unsafe {
                ReadFile(
                    self.handle,
                    buffer[done..].as_mut_ptr(),
                    wanted,
                    &mut read,
                    &mut overlapped,
                )
            };
            if succeeded == 0 {
                return Err(io::Error::last_os_error());
            }
            if read == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            done += read as usize;
        }
        Ok(())
    }
}

/// Peak working set of this process, in bytes.
pub fn peak_memory_bytes() -> Option<u64> {
    // SAFETY: the counters are plain integers, for which all zeroes is valid.
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { mem::zeroed() };
    let size = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: the pseudo handle is always valid, and `counters` is writable for
    // the `size` bytes passed.
    let succeeded = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) };
    (succeeded != 0).then_some(counters.PeakWorkingSetSize as u64)
}

impl Drop for Volume {
    fn drop(&mut self) {
        // SAFETY: the handle came from `CreateFileW` and is closed only here.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}
