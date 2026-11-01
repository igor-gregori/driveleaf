//! Command line interface for the Driveleaf scan engine.

#![forbid(unsafe_code)]

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use driveleaf_core::walk::walk;
use driveleaf_ntfs::synth::{ImageLayout, TreeTotals, for_each_generated, write_image};
use driveleaf_ntfs::{Geometry, ImageFile, ScanOptions, ScanOutput, peak_memory_bytes, scan};
use serde_json::{Value, json};

const USAGE: &str = "\
usage:
  driveleaf-cli mft <drive letter or image file>   scan an MFT and print JSON totals
  driveleaf-cli dump <drive letter or image file>  scan an MFT and print every record as TSV
  driveleaf-cli walk <folder>                      walk a folder and print JSON totals
  driveleaf-cli synth <image file> <records> [fragments]
                                                   write a synthetic NTFS volume image";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["mft", target] => mft(target),
        ["dump", target] => dump(target),
        ["walk", folder] => walk_folder(folder),
        ["synth", path, records] => synth(path, records, "1"),
        ["synth", path, records, fragments] => synth(path, records, fragments),
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

/// Reads `C`, `C:` or `C:\` as a drive letter.
fn drive_letter(target: &str) -> Option<char> {
    let mut chars = target.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    matches!(chars.as_str(), "" | ":" | ":\\").then_some(letter)
}

fn scan_target(target: &str) -> Result<(Geometry, ScanOutput), String> {
    let options = ScanOptions::default();
    match drive_letter(target) {
        Some(letter) => scan_drive(letter, &options),
        None => scan_image(Path::new(target), &options),
    }
}

fn mft(target: &str) -> Result<(), String> {
    let (geometry, output) = scan_target(target)?;
    print_json(&report(&geometry, &output));
    Ok(())
}

/// One `F` line per record and one `N` line per name, in tab-separated fields.
fn dump(target: &str) -> Result<(), String> {
    let (_, output) = scan_target(target)?;
    let mut out = BufWriter::with_capacity(1 << 20, std::io::stdout().lock());
    let mut write_all = || -> std::io::Result<()> {
        for chunk in &output.chunks {
            for file in &chunk.files {
                writeln!(
                    out,
                    "F\t{}\t{}\t{}\t{}\t{}\t{}",
                    file.record,
                    file.flags,
                    file.link_count,
                    file.logical_size,
                    file.stream_size,
                    file.allocated
                )?;
            }
            for name in &chunk.names {
                write!(out, "N\t{}\t{}\t", name.record, name.parent)?;
                out.write_all(chunk.name(name))?;
                out.write_all(b"\n")?;
            }
        }
        out.flush()
    };
    write_all().map_err(|error| error.to_string())
}

#[cfg(windows)]
fn scan_drive(letter: char, options: &ScanOptions) -> Result<(Geometry, ScanOutput), String> {
    use driveleaf_ntfs::win::Volume;
    let volume = Volume::open(letter).map_err(|error| {
        format!("could not open {letter}: ({error}); reading the MFT needs administrator rights")
    })?;
    let geometry = volume
        .geometry()
        .map_err(|error| format!("{letter}: is not an NTFS volume ({error})"))?;
    let output = scan(&volume, &geometry, options).map_err(|error| error.to_string())?;
    Ok((geometry, output))
}

#[cfg(not(windows))]
fn scan_drive(letter: char, _options: &ScanOptions) -> Result<(Geometry, ScanOutput), String> {
    Err(format!(
        "{letter}: scanning live volumes works on Windows only"
    ))
}

fn scan_image(path: &Path, options: &ScanOptions) -> Result<(Geometry, ScanOutput), String> {
    let image = ImageFile::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut boot = vec![0_u8; 4096];
    driveleaf_ntfs::BlockSource::read_exact_at(&image, 0, &mut boot)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let geometry = Geometry::from_boot_sector(&boot).map_err(|error| error.to_string())?;
    let output = scan(&image, &geometry, options).map_err(|error| error.to_string())?;
    Ok((geometry, output))
}

fn report(geometry: &Geometry, output: &ScanOutput) -> Value {
    let stats = &output.stats;
    let seconds = output.total_time.as_secs_f64();
    let entries = stats.names.max(1);
    let used_bytes = geometry
        .free_clusters
        .map(|free| (geometry.total_clusters - free) * u64::from(geometry.bytes_per_cluster));
    json!({
        "engine": "driveleaf",
        "elapsed_ms": seconds * 1000.0,
        "read_ms": output.read_time.as_secs_f64() * 1000.0,
        "records_per_second": stats.records_parsed as f64 / seconds,
        "read_jobs": output.read_jobs,
        "mft_extents": output.mft_extents,
        "bytes_per_record": geometry.bytes_per_record,
        "bytes_per_cluster": geometry.bytes_per_cluster,
        "records_total": stats.records_total,
        "records_parsed": stats.records_parsed,
        "records_unused": stats.records_unused,
        "base_records": stats.base_records,
        "extension_records": stats.extension_records,
        "files": stats.files,
        "directories": stats.directories,
        "names": stats.names,
        "name_bytes": stats.name_bytes,
        "logical_bytes": stats.logical_bytes,
        "stream_bytes": stats.stream_bytes,
        "allocated_bytes": stats.allocated_bytes,
        "used_bytes": used_bytes,
        "allocated_vs_used_percent": used_bytes
            .map(|used| 100.0 * (stats.allocated_bytes as f64 - used as f64) / used as f64),
        "corrupt_records": stats.corrupt_records,
        "malformed_records": stats.malformed_records,
        "unreadable_blocks": stats.unreadable_blocks,
        "bytes_read": stats.bytes_read,
        "engine_memory_bytes": output.memory_bytes(),
        "engine_bytes_per_entry": output.memory_bytes() as f64 / entries as f64,
        "peak_memory_bytes": peak_memory_bytes(),
    })
}

fn walk_folder(folder: &str) -> Result<(), String> {
    let started = Instant::now();
    let stats = walk(Path::new(folder));
    print_json(&json!({
        "engine": "walk",
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "files": stats.files,
        "directories": stats.directories,
        "logical_bytes": stats.logical_bytes,
        "errors": stats.errors,
        "peak_memory_bytes": peak_memory_bytes(),
    }));
    Ok(())
}

fn synth(path: &str, records: &str, fragments: &str) -> Result<(), String> {
    let record_count: u64 = records
        .parse()
        .map_err(|_| format!("bad record count: {records}"))?;
    let fragments: u64 = fragments
        .parse()
        .map_err(|_| format!("bad fragment count: {fragments}"))?;
    let layout = ImageLayout {
        bytes_per_cluster: 4096,
        bytes_per_record: 1024,
        record_count,
        fragments,
    };
    let file = File::create(path).map_err(|error| format!("{path}: {error}"))?;
    let mut out = BufWriter::with_capacity(1 << 20, file);
    let mut totals = TreeTotals::default();

    // Records come from a callback, so a channel lets `write_image` pull them
    // without holding the whole volume in memory.
    let (sender, receiver) = std::sync::mpsc::sync_channel(4096);
    std::thread::scope(|scope| {
        let generator = scope.spawn(move || {
            let mut totals = TreeTotals::default();
            for_each_generated(record_count, 4096, &mut totals, |number, spec| {
                let _ = sender.send((number, spec));
            });
            totals
        });
        let written = write_image(&mut out, &layout, receiver);
        totals = generator.join().expect("the generator did not panic");
        written.map_err(|error| format!("{path}: {error}"))
    })?;
    print_json(&json!({
        "files": totals.files,
        "directories": totals.directories,
        "names": totals.names,
        "logical_bytes": totals.logical_bytes,
        "stream_bytes": totals.stream_bytes,
        "allocated_bytes": totals.allocated_bytes,
    }));
    Ok(())
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("JSON values always serialize")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_letters_are_recognized() {
        assert_eq!(drive_letter("C"), Some('C'));
        assert_eq!(drive_letter("d:"), Some('d'));
        assert_eq!(drive_letter("E:\\"), Some('E'));
        assert_eq!(drive_letter("C:\\Windows"), None);
        assert_eq!(drive_letter("disk.img"), None);
        assert_eq!(drive_letter("1:"), None);
    }
}
