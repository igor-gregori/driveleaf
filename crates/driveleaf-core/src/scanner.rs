//! Picks the way to scan a target and returns its tree.

use std::fmt;
use std::path::Path;

use driveleaf_ntfs::replay::ReplaySource;
use driveleaf_ntfs::{BlockSource, Error, Geometry, ImageFile, ScanControl, ScanOptions, scan};

use crate::mft_tree::build_model;
use crate::model::Model;
use crate::walk::walk;

/// Why a scan produced no tree.
#[derive(Debug)]
pub enum ScanError {
    Cancelled,
    Failed(String),
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScanError::Cancelled => write!(f, "scan cancelled"),
            ScanError::Failed(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for ScanError {}

impl From<Error> for ScanError {
    fn from(error: Error) -> Self {
        match error {
            Error::Cancelled => ScanError::Cancelled,
            other => ScanError::Failed(other.to_string()),
        }
    }
}

fn describe(path: &Path, error: std::io::Error) -> ScanError {
    ScanError::Failed(format!("{}: {error}", path.display()))
}

/// Scans a volume image or a capture file. Paths in the tree start at `\`.
pub fn scan_image(path: &Path, control: &ScanControl) -> Result<Model, ScanError> {
    const ROOT_NAME: &str = "\\";
    let options = ScanOptions::default();
    if ReplaySource::is_capture(path).map_err(|error| describe(path, error))? {
        let replay = ReplaySource::open(path).map_err(|error| describe(path, error))?;
        let geometry = replay.geometry();
        let output = scan(&replay, &geometry, &options, control)?;
        return Ok(build_model(&output, &geometry, ROOT_NAME));
    }
    let image = ImageFile::open(path).map_err(|error| describe(path, error))?;
    let mut boot = vec![0_u8; 4096];
    image
        .read_exact_at(0, &mut boot)
        .map_err(|error| describe(path, error))?;
    let geometry = Geometry::from_boot_sector(&boot)?;
    let output = scan(&image, &geometry, &options, control)?;
    Ok(build_model(&output, &geometry, ROOT_NAME))
}

/// Walks a folder.
pub fn scan_folder(path: &Path, control: &ScanControl) -> Result<Model, ScanError> {
    let name = path.display().to_string();
    walk(path, &name, cfg!(unix), None, None, control).ok_or(ScanError::Cancelled)
}

/// Scans a drive in the fastest way this process is allowed to: the MFT when the
/// volume is NTFS and can be opened, and a folder walk otherwise.
#[cfg(windows)]
pub fn scan_drive(letter: char, control: &ScanControl) -> Result<Model, ScanError> {
    use driveleaf_ntfs::win::Volume;

    let root_name = format!("{}:\\", letter.to_ascii_uppercase());
    let volumes = driveleaf_win::volumes::list_volumes();
    let info = volumes
        .iter()
        .find(|volume| volume.letter.eq_ignore_ascii_case(&letter));
    let is_ntfs = info.is_some_and(|volume| volume.file_system.eq_ignore_ascii_case("NTFS"));

    if is_ntfs {
        let opened = Volume::open(letter).and_then(|volume| {
            let geometry = volume.geometry()?;
            Ok((volume, geometry))
        });
        if let Ok((volume, geometry)) = opened {
            let options = ScanOptions {
                readers: volume.recommended_readers(),
                ..ScanOptions::default()
            };
            match scan(&volume, &geometry, &options, control) {
                Ok(output) => return Ok(build_model(&output, &geometry, &root_name)),
                Err(Error::Cancelled) => return Err(ScanError::Cancelled),
                // A volume that cannot be read this way can still be walked.
                Err(_) => {}
            }
        }
    }

    let (used, total) = info.map_or((None, None), |volume| {
        (
            Some(volume.total_bytes - volume.free_bytes),
            Some(volume.total_bytes),
        )
    });
    walk(
        Path::new(&root_name),
        &root_name,
        is_ntfs,
        used,
        total,
        control,
    )
    .ok_or(ScanError::Cancelled)
}
