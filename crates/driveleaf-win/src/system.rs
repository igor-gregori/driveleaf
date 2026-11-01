//! Facts about the running system.

use std::mem;

use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
use windows_sys::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories,
};
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

/// Makes every later DLL load by name come from System32 only. The app may run
/// elevated from a folder others can write to, such as Downloads, and a DLL
/// planted next to it must never load. Call this first thing in `main`.
pub fn restrict_dll_search() {
    // SAFETY: takes only a flag value and changes this process's search order.
    unsafe {
        SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32);
    }
}

/// Whether the Mica window backdrop is available.
pub fn supports_mica() -> bool {
    windows_build() >= WINDOWS_11_BUILD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_number_is_a_real_one() {
        assert!(windows_build() > 10_000);
    }
}
