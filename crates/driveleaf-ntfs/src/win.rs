//! Live volume access on Windows. The only `unsafe` in this crate lives here.

#![allow(unsafe_code)]

use std::io;
use std::mem;
use std::ptr;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, GENERIC_READ, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile,
};
use windows_sys::Win32::System::IO::{DeviceIoControl, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Ioctl::{
    DEVICE_SEEK_PENALTY_DESCRIPTOR, FSCTL_GET_NTFS_VOLUME_DATA, IOCTL_STORAGE_QUERY_PROPERTY,
    NTFS_VOLUME_DATA_BUFFER, PropertyStandardQuery, STORAGE_PROPERTY_QUERY,
    StorageDeviceSeekPenaltyProperty,
};
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::{CreateEventW, GetCurrentProcess};

use crate::geometry::Geometry;
use crate::source::BlockSource;

/// Reads in flight on flash storage. Beyond this the gain is small.
const FLASH_READERS: usize = 8;

/// An open `\\.\X:` volume. Reads bypass the file cache, so they always measure
/// and return what is on disk. Opening it requires administrator rights.
pub struct Volume {
    handle: HANDLE,
}

// SAFETY: the handle is opened for overlapped I/O, which is safe to issue from
// any number of threads at once, and it stays valid until `Drop`.
unsafe impl Send for Volume {}
unsafe impl Sync for Volume {}

/// One overlapped call with its own completion event.
struct Completion {
    overlapped: OVERLAPPED,
}

impl Completion {
    fn at(offset: u64) -> io::Result<Self> {
        // SAFETY: all-null arguments ask for an unnamed event.
        let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: OVERLAPPED is plain data, for which all zeroes is valid.
        let mut overlapped: OVERLAPPED = unsafe { mem::zeroed() };
        overlapped.Anonymous.Anonymous.Offset = offset as u32;
        overlapped.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;
        overlapped.hEvent = event;
        Ok(Self { overlapped })
    }

    /// Waits for the call that just returned `succeeded` and yields the bytes
    /// it transferred. The call's buffers must stay valid until this returns.
    fn finish(&mut self, file: HANDLE, succeeded: i32) -> io::Result<u32> {
        if succeeded == 0 {
            // SAFETY: reads this thread's last error; takes no arguments.
            let error = unsafe { GetLastError() };
            if error != ERROR_IO_PENDING {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
        }
        let mut transferred = 0;
        // SAFETY: `overlapped` is the structure given to the pending call, and
        // the final argument makes this block until that call completes.
        let completed = unsafe { GetOverlappedResult(file, &self.overlapped, &mut transferred, 1) };
        if completed == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(transferred)
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        // SAFETY: the event came from `CreateEventW` and is closed only here,
        // after `finish` has seen the call complete.
        unsafe {
            CloseHandle(self.overlapped.hEvent);
        }
    }
}

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
                FILE_FLAG_NO_BUFFERING | FILE_FLAG_OVERLAPPED,
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
        let mut completion = Completion::at(0)?;
        // SAFETY: the output pointer and size describe `data`, and `finish`
        // waits for the call while `data` and `completion` are alive.
        let succeeded = unsafe {
            DeviceIoControl(
                self.handle,
                FSCTL_GET_NTFS_VOLUME_DATA,
                ptr::null(),
                0,
                (&raw mut data).cast(),
                size_of::<NTFS_VOLUME_DATA_BUFFER>() as u32,
                ptr::null_mut(),
                &mut completion.overlapped,
            )
        };
        completion.finish(self.handle, succeeded)?;
        Ok(Geometry {
            bytes_per_sector: data.BytesPerSector,
            bytes_per_cluster: data.BytesPerCluster,
            bytes_per_record: data.BytesPerFileRecordSegment,
            mft_lcn: data.MftStartLcn as u64,
            total_clusters: data.TotalClusters as u64,
            free_clusters: Some(data.FreeClusters as u64),
        })
    }

    /// How many reads to issue at once: several on flash, where they overlap
    /// well, and one on a spinning disk, where they would only add seeks.
    pub fn recommended_readers(&self) -> usize {
        match self.incurs_seek_penalty() {
            Ok(false) => FLASH_READERS,
            _ => 1,
        }
    }

    fn incurs_seek_penalty(&self) -> io::Result<bool> {
        // SAFETY: both structures are plain data, for which all zeroes is valid.
        let mut query: STORAGE_PROPERTY_QUERY = unsafe { mem::zeroed() };
        query.PropertyId = StorageDeviceSeekPenaltyProperty;
        query.QueryType = PropertyStandardQuery;
        let mut answer: DEVICE_SEEK_PENALTY_DESCRIPTOR = unsafe { mem::zeroed() };
        let mut completion = Completion::at(0)?;
        // SAFETY: the pointers and sizes describe `query` and `answer`, and
        // `finish` waits for the call while all three are alive.
        let succeeded = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_STORAGE_QUERY_PROPERTY,
                (&raw const query).cast(),
                size_of::<STORAGE_PROPERTY_QUERY>() as u32,
                (&raw mut answer).cast(),
                size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>() as u32,
                ptr::null_mut(),
                &mut completion.overlapped,
            )
        };
        completion.finish(self.handle, succeeded)?;
        Ok(answer.IncursSeekPenalty)
    }
}

impl BlockSource for Volume {
    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        let mut done = 0;
        while done < buffer.len() {
            let mut completion = Completion::at(offset + done as u64)?;
            let wanted = u32::try_from(buffer.len() - done).unwrap_or(u32::MAX);
            // SAFETY: the pointer and length stay inside `buffer`, and `finish`
            // waits for the call while `buffer` and `completion` are alive.
            let succeeded = unsafe {
                ReadFile(
                    self.handle,
                    buffer[done..].as_mut_ptr(),
                    wanted,
                    ptr::null_mut(),
                    &mut completion.overlapped,
                )
            };
            match completion.finish(self.handle, succeeded)? {
                0 => return Err(io::ErrorKind::UnexpectedEof.into()),
                read => done += read as usize,
            }
        }
        Ok(())
    }
}

impl Drop for Volume {
    fn drop(&mut self) {
        // SAFETY: the handle came from `CreateFileW` and is closed only here.
        unsafe {
            CloseHandle(self.handle);
        }
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
