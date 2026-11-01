//! Facts about the running system.

use std::mem;

use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;

/// The first Windows 11 build, which is also the first with Mica.
const WINDOWS_11_BUILD: u32 = 22_000;

/// The Windows build number. Unlike the older version calls, this one does not
/// depend on what the program's manifest declares.
pub fn windows_build() -> u32 {
    // SAFETY: the structure is plain data, for which all zeroes is valid.
    let mut info: OSVERSIONINFOW = unsafe { mem::zeroed() };
    info.dwOSVersionInfoSize = size_of::<OSVERSIONINFOW>() as u32;
    // SAFETY: `info` is writable and its size field is set, as the call requires.
    if unsafe { RtlGetVersion(&mut info) } == 0 {
        info.dwBuildNumber
    } else {
        0
    }
}

/// Whether the Mica window backdrop is available.
pub fn supports_mica() -> bool {
    windows_build() >= WINDOWS_11_BUILD
}
