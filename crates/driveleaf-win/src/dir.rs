//! Folder listing with sizes and file IDs, in one call per batch of entries
//! rather than one per file.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_NO_MORE_FILES, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdBothDirectoryInfo,
    FileIdBothDirectoryRestartInfo, GetFileInformationByHandleEx, OPEN_EXISTING,
};

use crate::{extended, wide};

pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
pub const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

const BUFFER_BYTES: usize = 64 * 1024;

// Field offsets of FILE_ID_BOTH_DIR_INFO. Decoding them from bytes keeps the
// parsing in safe code.
const OFFSET_LAST_WRITE: usize = 24;
const OFFSET_END_OF_FILE: usize = 40;
const OFFSET_ALLOCATION: usize = 48;
const OFFSET_ATTRIBUTES: usize = 56;
const OFFSET_NAME_BYTES: usize = 60;
const OFFSET_EA_SIZE: usize = 64;
const OFFSET_FILE_ID: usize = 96;
const OFFSET_NAME: usize = 104;

/// One entry of a folder.
#[derive(Clone, Debug)]
pub struct DirEntry {
    pub name: OsString,
    pub attributes: u32,
    /// The reparse tag, or zero when this is not a reparse point.
    pub reparse_tag: u32,
    pub logical: u64,
    pub allocated: u64,
    pub modified: u64,
    pub file_id: u64,
}

impl DirEntry {
    pub fn is_directory(&self) -> bool {
        self.attributes & FILE_ATTRIBUTE_DIRECTORY != 0
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("eight bytes"))
}

/// Splits one batch into entries. The system built it, but this is still bounds
/// checked by the slice accesses.
fn parse_batch(batch: &[u8], emit: &mut impl FnMut(DirEntry)) {
    let mut offset = 0;
    loop {
        let entry = &batch[offset..];
        let next = u32_at(entry, 0) as usize;
        let name_bytes = u32_at(entry, OFFSET_NAME_BYTES) as usize;
        let (pairs, _) = entry[OFFSET_NAME..OFFSET_NAME + name_bytes].as_chunks::<2>();
        let units: Vec<u16> = pairs.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
        let name = OsString::from_wide(&units);
        if name != "." && name != ".." {
            let attributes = u32_at(entry, OFFSET_ATTRIBUTES);
            emit(DirEntry {
                name,
                attributes,
                // The system reuses this field for the tag of a reparse point.
                reparse_tag: if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    u32_at(entry, OFFSET_EA_SIZE)
                } else {
                    0
                },
                logical: u64_at(entry, OFFSET_END_OF_FILE),
                allocated: u64_at(entry, OFFSET_ALLOCATION),
                modified: u64_at(entry, OFFSET_LAST_WRITE),
                file_id: u64_at(entry, OFFSET_FILE_ID),
            });
        }
        if next == 0 {
            return;
        }
        offset += next;
    }
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from `CreateFileW` and is closed only here.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Calls `emit` for everything in a folder. `path` must be absolute.
pub fn list_directory(path: &Path, mut emit: impl FnMut(DirEntry)) -> io::Result<()> {
    let name = wide(&extended(path.as_os_str()));
    // SAFETY: `name` is NUL-terminated and outlives the call.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let handle = Handle(handle);

    // Eight-byte items give the alignment the structure needs.
    let mut buffer = vec![0_u64; BUFFER_BYTES / 8];
    let mut class = FileIdBothDirectoryRestartInfo;
    loop {
        // SAFETY: the pointer and size describe `buffer`, and the call is
        // synchronous.
        let succeeded = unsafe {
            GetFileInformationByHandleEx(
                handle.0,
                class,
                buffer.as_mut_ptr().cast(),
                BUFFER_BYTES as u32,
            )
        };
        if succeeded == 0 {
            // SAFETY: reads this thread's last error; takes no arguments.
            let error = unsafe { GetLastError() };
            if error == ERROR_NO_MORE_FILES {
                return Ok(());
            }
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        class = FileIdBothDirectoryInfo;
        // SAFETY: any `u64` buffer is also valid as initialized bytes.
        let bytes = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast(), BUFFER_BYTES) };
        parse_batch(bytes, &mut emit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sandbox(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("driveleaf-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub")).unwrap();
        root
    }

    fn listing(root: &Path) -> Vec<DirEntry> {
        let mut entries = Vec::new();
        list_directory(root, |entry| entries.push(entry)).unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    #[test]
    fn lists_names_sizes_and_kinds() {
        let root = sandbox("list");
        fs::write(root.join("data.bin"), vec![7_u8; 10_000]).unwrap();
        fs::write(root.join("relat\u{F3}rio \u{1F33F}.txt"), b"hi").unwrap();

        let entries = listing(&root);
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(entries.len(), 3, "no . or .. entries");
        assert_eq!(entries[0].name, "data.bin");
        assert_eq!(entries[0].logical, 10_000);
        assert!(entries[0].allocated >= 10_000);
        assert!(!entries[0].is_directory());
        assert!(entries[0].modified > 0);
        assert_eq!(entries[1].name, "relat\u{F3}rio \u{1F33F}.txt");
        assert_eq!(entries[2].name, "sub");
        assert!(entries[2].is_directory());
        assert_eq!(entries[2].reparse_tag, 0);
    }

    #[test]
    fn long_listings_span_several_batches() {
        let root = sandbox("many");
        for index in 0..3000 {
            fs::write(
                root.join(format!("a-rather-long-file-name-{index:05}.txt")),
                b"x",
            )
            .unwrap();
        }
        let entries = listing(&root);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(entries.len(), 3001);
    }

    #[test]
    fn hard_links_share_a_file_id() {
        let root = sandbox("links");
        fs::write(root.join("first"), b"data").unwrap();
        fs::hard_link(root.join("first"), root.join("second")).unwrap();
        let entries = listing(&root);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(entries[0].file_id, entries[1].file_id);
        assert_ne!(entries[0].file_id, 0);
    }

    #[test]
    fn the_listing_and_the_file_agree_on_its_id() {
        let root = sandbox("id");
        fs::write(root.join("file"), b"data").unwrap();
        let listed = listing(&root)[0].file_id;
        let opened = crate::shell::file_id(root.join("file").as_os_str()).unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(listed, opened);
    }

    #[test]
    fn a_missing_folder_is_an_error() {
        let root = sandbox("missing");
        fs::remove_dir_all(&root).unwrap();
        assert!(list_directory(&root, |_| {}).is_err());
        assert!(crate::shell::file_id(root.as_os_str()).is_err());
    }
}
