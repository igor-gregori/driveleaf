//! M0 spike: runs the candidate crates over the same MFT as `driveleaf-cli mft`.
//!
//! The workload is the same for all of them: decode every in-use record, count
//! files and folders, read the name and sum the main stream sizes.

#![forbid(unsafe_code)]

use std::process::ExitCode;
use std::time::{Duration, Instant};

use serde_json::json;

const USAGE: &str = "\
usage:
  engine-compare mft <dumped $MFT file>
  engine-compare ntfs-reader <\\\\.\\C: or volume image>       (Windows)
  engine-compare ntfs-reader-scan <\\\\.\\C: or volume image>  (Windows)";

#[derive(Default)]
struct Totals {
    files: u64,
    directories: u64,
    names: u64,
    name_bytes: u64,
    logical_bytes: u64,
    errors: u64,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["mft", dump] => mft_crate(dump),
        ["ntfs-reader", volume] => ntfs_reader::whole(volume),
        ["ntfs-reader-scan", volume] => ntfs_reader::chunked(volume),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn report(engine: &str, elapsed: Duration, totals: &Totals, memory_bytes: Option<usize>) {
    let summary = json!({
        "engine": engine,
        "elapsed_ms": elapsed.as_secs_f64() * 1000.0,
        "files": totals.files,
        "directories": totals.directories,
        "names": totals.names,
        "name_bytes": totals.name_bytes,
        "logical_bytes": totals.logical_bytes,
        "errors": totals.errors,
        "engine_memory_bytes": memory_bytes,
        "peak_memory_bytes": driveleaf_ntfs::peak_memory_bytes(),
    });
    println!("{summary:#}");
}

fn mft_crate(path: &str) -> Result<(), String> {
    use mft::MftParser;
    use mft::attribute::MftAttributeContent;
    use mft::attribute::header::ResidentialHeader;

    let started = Instant::now();
    let mut parser = MftParser::from_path(path).map_err(|error| error.to_string())?;
    let mut totals = Totals::default();
    for entry in parser.iter_entries() {
        let Ok(entry) = entry else {
            totals.errors += 1;
            continue;
        };
        if !entry.is_allocated() {
            continue;
        }
        if entry.is_dir() {
            totals.directories += 1;
        } else {
            totals.files += 1;
        }
        for attribute in entry.iter_attributes() {
            let Ok(attribute) = attribute else {
                totals.errors += 1;
                continue;
            };
            let unnamed = attribute.header.name_size == 0;
            match &attribute.data {
                MftAttributeContent::AttrX30(name) => {
                    totals.names += 1;
                    totals.name_bytes += name.name.to_string().len() as u64;
                }
                MftAttributeContent::AttrX80(data) if unnamed => {
                    totals.logical_bytes += data.data().len() as u64;
                }
                _ => {}
            }
            if let ResidentialHeader::NonResident(header) = &attribute.header.residential_header {
                let is_data = attribute.header.type_code == mft::attribute::MftAttributeType::DATA;
                if is_data && unnamed && header.vnc_first == 0 {
                    totals.logical_bytes += header.file_size;
                }
            }
        }
    }
    report("mft-crate", started.elapsed(), &totals, None);
    Ok(())
}

#[cfg(windows)]
mod ntfs_reader {
    use std::time::Instant;

    use ntfs_reader::{DefaultPathCache, FileInfo, Mft, MftScan, NtfsFile, Volume};

    use super::{Totals, report};

    fn add<'a>(
        files: impl Iterator<Item = NtfsFile<'a>>,
        cache: &mut DefaultPathCache,
        totals: &mut Totals,
    ) {
        for file in files {
            let info = FileInfo::with_cache(&file, cache);
            if info.is_directory {
                totals.directories += 1;
            } else {
                totals.files += 1;
                totals.logical_bytes += info.size;
            }
            totals.names += 1;
            totals.name_bytes += info.name.len() as u64;
        }
    }

    pub(super) fn whole(volume: &str) -> Result<(), String> {
        let started = Instant::now();
        let volume = Volume::new(volume).map_err(|error| error.to_string())?;
        let mft = Mft::new(volume).map_err(|error| error.to_string())?;
        let mut totals = Totals::default();
        let mut cache = DefaultPathCache::new();
        add(mft.files(), &mut cache, &mut totals);
        report(
            "ntfs-reader",
            started.elapsed(),
            &totals,
            Some(mft.size_in_memory()),
        );
        Ok(())
    }

    pub(super) fn chunked(volume: &str) -> Result<(), String> {
        let started = Instant::now();
        let volume = Volume::new(volume).map_err(|error| error.to_string())?;
        let mut scan = MftScan::new(volume).map_err(|error| error.to_string())?;
        let mut totals = Totals::default();
        let mut cache = DefaultPathCache::new();
        while let Some(chunk) = scan.next_chunk().map_err(|error| error.to_string())? {
            add(chunk.files(), &mut cache, &mut totals);
        }
        report(
            "ntfs-reader-scan",
            started.elapsed(),
            &totals,
            Some(scan.size_in_memory()),
        );
        Ok(())
    }
}

#[cfg(not(windows))]
mod ntfs_reader {
    pub(super) fn whole(_volume: &str) -> Result<(), String> {
        Err("ntfs-reader runs on Windows only".to_owned())
    }

    pub(super) fn chunked(_volume: &str) -> Result<(), String> {
        Err("ntfs-reader runs on Windows only".to_owned())
    }
}
