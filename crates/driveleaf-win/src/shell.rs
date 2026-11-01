//! Acting on files: showing them in Explorer, opening their properties and
//! sending them to the Recycle Bin. Paths must be absolute.
//!
//! The app often runs elevated, so everything here takes full paths built from
//! the scan, never text from the web view, and nothing deletes permanently.

use std::ffi::{OsStr, OsString};
use std::io;
use std::mem;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::ptr;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_WANTNUKEWARNING, FOFX_ADDUNDORECORD,
    FOFX_RECYCLEONDELETE, FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName,
};
use windows::core::{HSTRING, Result as WinResult};
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING,
};
use windows_sys::Win32::UI::Shell::{SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;

use crate::{extended, wide};

/// Opens Explorer with the item selected.
pub fn reveal_in_explorer(path: &OsStr) -> io::Result<()> {
    let mut argument = OsString::from("/select,\"");
    argument.push(path);
    argument.push("\"");
    Command::new("explorer.exe").raw_arg(argument).spawn()?;
    Ok(())
}

/// Opens the standard properties dialog.
pub fn show_properties(path: &OsStr) -> io::Result<()> {
    let verb = wide("properties".as_ref());
    let file = wide(path);
    // SAFETY: the structure is plain data, for which all zeroes is valid.
    let mut info: SHELLEXECUTEINFOW = unsafe { mem::zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_INVOKEIDLIST;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.nShow = SW_SHOW;
    // SAFETY: `info` is fully initialized, and its strings outlive the call.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The file ID at a path, without following links. It confirms that a path
/// still names the file that was scanned before anything acts on it.
pub fn file_id(path: &OsStr) -> io::Result<u64> {
    let name = wide(&extended(path));
    // SAFETY: `name` is NUL-terminated and outlives the call.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_READ_ATTRIBUTES,
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
    // SAFETY: the structure is plain data, for which all zeroes is valid.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { mem::zeroed() };
    // SAFETY: `handle` is open and `info` is writable.
    let succeeded = unsafe { GetFileInformationByHandle(handle, &mut info) };
    let error = io::Error::last_os_error();
    // SAFETY: `handle` was opened above and is closed only here.
    unsafe {
        CloseHandle(handle);
    }
    if succeeded == 0 {
        return Err(error);
    }
    Ok((u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow))
}

/// Keeps COM initialized on this thread for as long as it lives.
struct ComGuard;

impl ComGuard {
    fn new() -> WinResult<Self> {
        // SAFETY: the reserved argument may be null. The guard pairs this with
        // one `CoUninitialize` on the same thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.ok()?;
        Ok(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: pairs with the successful `CoInitializeEx` in `new`.
        unsafe {
            CoUninitialize();
        }
    }
}

fn queue_delete(operation: &IFileOperation, path: &OsStr) -> WinResult<()> {
    let name = HSTRING::from(path);
    // SAFETY: `name` outlives the call, and the bind context is optional.
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(&name, None) }?;
    // SAFETY: `item` is a valid shell item and the progress sink is optional.
    unsafe { operation.DeleteItem(&item, None) }
}

/// Sends items to the Recycle Bin. Windows asks first if an item cannot go there
/// and would be lost, instead of quietly deleting it.
///
/// Runs on a thread of its own, because COM needs a single-threaded apartment.
/// `owner` is the window handle Windows should attach any dialog to. Returns
/// false when the user or the system aborted part of the operation.
pub fn recycle(paths: Vec<OsString>, owner: isize) -> io::Result<bool> {
    std::thread::spawn(move || -> WinResult<bool> {
        let _com = ComGuard::new()?;
        // SAFETY: COM is initialized on this thread, and the class comes from
        // the system.
        let operation: IFileOperation =
            unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL) }?;
        let flags = FOF_ALLOWUNDO.0
            | FOF_NOCONFIRMATION.0
            | FOF_WANTNUKEWARNING.0
            | FOFX_ADDUNDORECORD.0
            | FOFX_RECYCLEONDELETE.0;
        // SAFETY: `operation` is a valid object, and the flags are documented
        // constants.
        unsafe {
            operation.SetOperationFlags(windows::Win32::UI::Shell::FILEOPERATION_FLAGS(flags))?;
            operation.SetOwnerWindow(HWND(owner as *mut _))?;
        }
        for path in &paths {
            queue_delete(&operation, path)?;
        }
        // SAFETY: `operation` is a valid object with its work queued.
        unsafe {
            operation.PerformOperations()?;
            Ok(!operation.GetAnyOperationsAborted()?.as_bool())
        }
    })
    .join()
    .map_err(|_| io::Error::other("the recycle thread panicked"))?
    .map_err(io::Error::other)
}
