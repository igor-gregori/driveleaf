//! Windows file system and shell calls. This crate and the volume layer of
//! `driveleaf-ntfs` are the only places with `unsafe` code. On other platforms
//! it is empty.

#![cfg(windows)]
#![allow(unsafe_code)]

pub mod dir;
pub mod elevation;
pub mod shell;
pub mod system;
pub mod volumes;

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};

/// Encodes a string for a Windows call, with its terminating NUL.
pub(crate) fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

/// Reads a NUL-terminated buffer filled in by a Windows call.
pub(crate) fn from_wide(buffer: &[u16]) -> OsString {
    let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    OsString::from_wide(&buffer[..end])
}

/// Turns a WTF-8 path from the model into an `OsString`, keeping names that are
/// not valid Unicode.
pub fn path_from_wtf8(bytes: &[u8]) -> OsString {
    // SAFETY: the model's names come straight from `OsStr` encoded bytes or from
    // WTF-8 encoding of UTF-16, which are the same format on Windows.
    unsafe { OsString::from_encoded_bytes_unchecked(bytes.to_vec()) }
}

/// Adds the prefix that lifts the 260 character limit from a full path.
pub(crate) fn extended(path: &OsStr) -> OsString {
    let mut prefixed = OsString::from(r"\\?\");
    prefixed.push(path);
    prefixed
}
