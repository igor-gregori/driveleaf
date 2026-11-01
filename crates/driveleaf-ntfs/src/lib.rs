//! Fast NTFS Master File Table scanner.
//!
//! The parser works on a [`BlockSource`], never on a Windows handle, so it runs
//! and gets tested on any platform. It contains no `unsafe` and treats every
//! byte it reads as untrusted. The Windows volume layer lives apart in [`win`].

#![deny(unsafe_code)]

mod bytes;
mod error;
mod geometry;
mod mft;
mod names;
pub mod record;
mod runs;
mod scan;
mod source;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
#[cfg(windows)]
pub mod win;

pub use error::Error;
pub use geometry::Geometry;
pub use runs::{Run, Runs};
pub use scan::{
    Chunk, RAW_DIRECTORY, RAW_EXTENSION, RawFile, RawName, ScanOptions, ScanOutput, ScanStats, scan,
};
pub use source::{BlockSource, ImageFile};
