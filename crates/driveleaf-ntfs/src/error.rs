use std::fmt;
use std::io;

/// Why a scan could not run at all. Single bad records never end up here; they
/// are counted in the scan statistics instead.
#[derive(Debug)]
pub enum Error {
    /// Reading the volume failed.
    Io(io::Error),
    /// The boot sector is not a usable NTFS boot sector.
    NotNtfs(&'static str),
    /// The MFT's own record could not be understood.
    BadMft(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(error) => write!(f, "volume read failed: {error}"),
            Error::NotNtfs(reason) => write!(f, "not an NTFS volume: {reason}"),
            Error::BadMft(reason) => write!(f, "unreadable MFT: {reason}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error::Io(error)
    }
}
