//! Administrator rights. The app starts without them and reopens itself on
//! request, so Windows shows its normal consent prompt.

use std::io;
use std::mem;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::wide;

/// Whether this process runs with administrator rights.
pub fn is_elevated() -> bool {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the pseudo handle is always valid and `token` is writable.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }
    // SAFETY: the structure is a plain integer, for which zero is valid.
    let mut elevation: TOKEN_ELEVATION = unsafe { mem::zeroed() };
    let mut returned = 0_u32;
    // SAFETY: the pointer and size describe `elevation`, and `token` is open.
    let succeeded = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&raw mut elevation).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    // SAFETY: `token` was opened above and is closed only here.
    unsafe {
        CloseHandle(token);
    }
    succeeded != 0 && elevation.TokenIsElevated != 0
}

/// Starts this program again through the consent prompt, with the given command
/// line. On success the caller should exit. Declining the prompt comes back as
/// an error.
pub fn relaunch_elevated(arguments: Option<&str>) -> io::Result<()> {
    let program = std::env::current_exe()?;
    let verb = wide("runas".as_ref());
    let file = wide(program.as_os_str());
    let parameters = arguments.map(|text| wide(text.as_ref()));
    // SAFETY: both strings are NUL-terminated and outlive the call, and the
    // other arguments may be null.
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            parameters
                .as_ref()
                .map_or(ptr::null(), |text| text.as_ptr()),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success, by the function's contract.
    if result as usize > 32 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
