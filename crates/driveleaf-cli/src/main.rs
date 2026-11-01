//! Command line interface for the Driveleaf scan engine.

#![forbid(unsafe_code)]

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use driveleaf_core::model::{
    FLAG_DIRECTORY, FLAG_HARD_LINK, FLAG_LINK, FLAG_METAFILE, FLAG_SYNTHETIC, Model, NodeId, ROOT,
};
use driveleaf_core::query::SortKey;
use driveleaf_core::scanner::{ScanError, scan_folder, scan_image as scan_image_tree};
use driveleaf_ntfs::demo::{demo_record_count, for_each_demo_record};
use driveleaf_ntfs::record::{
    ATTR_REPARSE_POINT, AttributeBody, Attributes, RecordHeader, reparse_tag,
};
use driveleaf_ntfs::replay::ReplaySource;
use driveleaf_ntfs::synth::{ImageLayout, TreeTotals, for_each_generated, write_image};
use driveleaf_ntfs::{
    BlockSource, Geometry, ImageFile, ScanControl, ScanOptions, ScanOutput, peak_memory_bytes,
    read_record, scan,
};
use driveleaf_treemap::{LayoutOptions, layout_tree};
use serde_json::{Value, json};

const USAGE: &str = "\
usage:
  driveleaf-cli scan <target> [--format json|csv]  print every entry of a drive, folder,
                                                   image or capture; csv is the default
  driveleaf-cli mft <drive letter or image file>   scan an MFT and print JSON totals
  driveleaf-cli bench <target>                     time the queries behind the interface
  driveleaf-cli dump <drive letter or image file>  scan an MFT and print every record as TSV
  driveleaf-cli record <image file> <number>       print the attributes of one record
  driveleaf-cli capture <drive letter> <file>      save what a scan reads, to replay it later
  driveleaf-cli demo <image file>                  write a made-up disk for screenshots
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
        ["scan", target] => scan_tree(target, "csv"),
        ["scan", target, "--format", format] => scan_tree(target, format),
        ["bench", target] => bench(target),
        ["mft", target] => mft(target),
        ["dump", target] => dump(target),
        ["record", path, number] => show_record(path, number),
        ["capture", target, path] => capture(target, path),
        ["demo", path] => demo(path),
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

fn scan_target(target: &str) -> Result<(Geometry, ScanOutput, usize), String> {
    let mut options = ScanOptions::default();
    match drive_letter(target) {
        Some(letter) => scan_drive(letter, &mut options),
        None => scan_image(Path::new(target), &options),
    }
}

fn run_scan<S: BlockSource + ?Sized>(
    source: &S,
    geometry: &Geometry,
    options: &ScanOptions,
) -> Result<ScanOutput, String> {
    scan(source, geometry, options, &ScanControl::default()).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn capture(target: &str, path: &str) -> Result<(), String> {
    use driveleaf_ntfs::replay::RecordingSource;
    use driveleaf_ntfs::win::Volume;
    let letter = drive_letter(target).ok_or("capture needs a drive letter")?;
    let volume = open_volume(letter)?;
    let geometry = volume
        .geometry()
        .map_err(|error| format!("{letter}: is not an NTFS volume ({error})"))?;
    let recorder: RecordingSource<'_, Volume> =
        RecordingSource::create(&volume, &geometry, Path::new(path))
            .map_err(|error| format!("{path}: {error}"))?;
    let output = run_scan(&recorder, &geometry, &ScanOptions::default())?;
    recorder
        .finish()
        .map_err(|error| format!("{path}: {error}"))?;
    print_json(&report(&geometry, &output, 1));
    Ok(())
}

#[cfg(not(windows))]
fn capture(_target: &str, _path: &str) -> Result<(), String> {
    Err("capturing live volumes works on Windows only".to_owned())
}

fn mft(target: &str) -> Result<(), String> {
    let (geometry, output, readers) = scan_target(target)?;
    print_json(&report(&geometry, &output, readers));
    Ok(())
}

/// One `F` line per record and one `N` line per name, in tab-separated fields.
fn dump(target: &str) -> Result<(), String> {
    let (_, output, _) = scan_target(target)?;
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
fn open_volume(letter: char) -> Result<driveleaf_ntfs::win::Volume, String> {
    driveleaf_ntfs::win::Volume::open(letter).map_err(|error| {
        format!("could not open {letter}: ({error}); reading the MFT needs administrator rights")
    })
}

#[cfg(windows)]
fn scan_drive(
    letter: char,
    options: &mut ScanOptions,
) -> Result<(Geometry, ScanOutput, usize), String> {
    let volume = open_volume(letter)?;
    let geometry = volume
        .geometry()
        .map_err(|error| format!("{letter}: is not an NTFS volume ({error})"))?;
    options.readers = volume.recommended_readers();
    let output = run_scan(&volume, &geometry, options)?;
    Ok((geometry, output, options.readers))
}

#[cfg(not(windows))]
fn scan_drive(
    letter: char,
    _options: &mut ScanOptions,
) -> Result<(Geometry, ScanOutput, usize), String> {
    Err(format!(
        "{letter}: scanning live volumes works on Windows only"
    ))
}

fn scan_image(path: &Path, options: &ScanOptions) -> Result<(Geometry, ScanOutput, usize), String> {
    let describe = |error: std::io::Error| format!("{}: {error}", path.display());
    if ReplaySource::is_capture(path).map_err(describe)? {
        let replay = ReplaySource::open(path).map_err(describe)?;
        let geometry = replay.geometry();
        let output = run_scan(&replay, &geometry, options)?;
        return Ok((geometry, output, options.readers));
    }
    let image = ImageFile::open(path).map_err(describe)?;
    let mut boot = vec![0_u8; 4096];
    image.read_exact_at(0, &mut boot).map_err(describe)?;
    let geometry = Geometry::from_boot_sector(&boot).map_err(|error| error.to_string())?;
    let output = run_scan(&image, &geometry, options)?;
    Ok((geometry, output, options.readers))
}

fn report(geometry: &Geometry, output: &ScanOutput, readers: usize) -> Value {
    let stats = &output.stats;
    let seconds = output.total_time.as_secs_f64();
    let entries = stats.names.max(1);
    let used_bytes = geometry
        .free_clusters
        .map(|free| (geometry.total_clusters - free) * u64::from(geometry.bytes_per_cluster));
    json!({
        "engine": "driveleaf",
        "elapsed_ms": seconds * 1000.0,
        "layout_ms": output.layout_time.as_secs_f64() * 1000.0,
        "read_ms": output.read_time.as_secs_f64() * 1000.0,
        "wait_ms": output.wait_time.as_secs_f64() * 1000.0,
        "readers": readers,
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
        "bad_signature_records": stats.bad_signature_records,
        "torn_records": stats.torn_records,
        "bad_header_records": stats.bad_header_records,
        "malformed_records": stats.malformed_records,
        "unreadable_blocks": stats.unreadable_blocks,
        "bytes_read": stats.bytes_read,
        "engine_memory_bytes": output.memory_bytes(),
        "engine_bytes_per_entry": output.memory_bytes() as f64 / entries as f64,
        "peak_memory_bytes": peak_memory_bytes(),
    })
}

fn scan_any(target: &str) -> Result<Model, ScanError> {
    let control = ScanControl::default();
    if let Some(letter) = drive_letter(target) {
        return drive_tree(letter, &control);
    }
    let path = Path::new(target);
    if path.is_dir() {
        scan_folder(path, &control)
    } else {
        scan_image_tree(path, &control)
    }
}

#[cfg(windows)]
fn drive_tree(letter: char, control: &ScanControl) -> Result<Model, ScanError> {
    driveleaf_core::scanner::scan_drive(letter, control)
}

#[cfg(not(windows))]
fn drive_tree(letter: char, _control: &ScanControl) -> Result<Model, ScanError> {
    Err(ScanError::Failed(format!(
        "{letter}: scanning drives works on Windows only"
    )))
}

/// Times what the interface asks for, to check it against the navigation target.
fn bench(target: &str) -> Result<(), String> {
    fn timed<T>(work: impl FnOnce() -> T) -> (T, f64) {
        let started = Instant::now();
        let result = work();
        (result, started.elapsed().as_secs_f64() * 1000.0)
    }

    let (model, scan_ms) = timed(|| scan_any(target));
    let model = model.map_err(|error| error.to_string())?;
    let biggest_folder = model
        .raw_children(ROOT)
        .iter()
        .copied()
        .find(|&node| model.is_directory(node))
        .unwrap_or(ROOT);
    let options = LayoutOptions::default();
    let (tiles, layout_root_ms) = timed(|| layout_tree(&model, ROOT, 1600.0, 900.0, &options));
    let (_, layout_folder_ms) =
        timed(|| layout_tree(&model, biggest_folder, 1600.0, 900.0, &options));
    let (_, children_ms) = timed(|| model.list_children(ROOT, SortKey::Name, false, 0, 2000));
    let (ranked, rank_ms) = timed(|| model.rank_files(None));
    let (types, types_ms) = timed(|| model.file_types());
    let (found, search_ms) = timed(|| model.search("log", 100));
    print_json(&json!({
        "entries": model.len(),
        "files": model.files(ROOT),
        "memory_bytes": model.memory_bytes(),
        "bytes_per_entry": model.memory_bytes() as f64 / model.len() as f64,
        "scan_and_build_ms": scan_ms,
        "treemap_root_ms": layout_root_ms,
        "treemap_folder_ms": layout_folder_ms,
        "tiles": tiles.len(),
        "children_ms": children_ms,
        "rank_files_ms": rank_ms,
        "ranked_files": ranked.len(),
        "file_types_ms": types_ms,
        "file_types": types.len(),
        "search_ms": search_ms,
        "search_hits": found.total,
        "peak_memory_bytes": peak_memory_bytes(),
    }));
    Ok(())
}

fn kind(model: &Model, node: NodeId) -> &'static str {
    let flags = model.flags(node);
    if flags & FLAG_SYNTHETIC != 0 {
        "synthetic"
    } else if flags & FLAG_METAFILE != 0 {
        "metafile"
    } else if flags & FLAG_LINK != 0 {
        "link"
    } else if flags & FLAG_HARD_LINK != 0 {
        "hardlink"
    } else if flags & FLAG_DIRECTORY != 0 {
        "directory"
    } else {
        "file"
    }
}

/// Depth-first, so a folder comes right before what it holds.
fn each_node(
    model: &Model,
    mut visit: impl FnMut(NodeId) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut stack = vec![ROOT];
    while let Some(node) = stack.pop() {
        visit(node)?;
        stack.extend(
            model
                .raw_children(node)
                .iter()
                .rev()
                .copied()
                .filter(|&child| model.is_visible(child)),
        );
    }
    Ok(())
}

fn csv_field(text: &str) -> String {
    if text.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

/// Formats 100 ns ticks since 1601 as an ISO 8601 UTC time.
fn iso_time(ticks: u64) -> String {
    const TICKS_PER_SECOND: u64 = 10_000_000;
    const SECONDS_TO_UNIX_EPOCH: u64 = 11_644_473_600;
    if ticks == 0 {
        return String::new();
    }
    let seconds = (ticks / TICKS_PER_SECOND).saturating_sub(SECONDS_TO_UNIX_EPOCH);
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // Days to civil date, after Howard Hinnant's public domain algorithm.
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        (rest / 60) % 60,
        rest % 60
    )
}

fn scan_tree(target: &str, format: &str) -> Result<(), String> {
    if format != "csv" && format != "json" {
        return Err(format!("unknown format {format}; use json or csv"));
    }
    let model = scan_any(target).map_err(|error| error.to_string())?;
    let mut out = BufWriter::with_capacity(1 << 20, std::io::stdout().lock());
    let written = if format == "csv" {
        writeln!(out, "path,kind,logical,allocated,files,modified,id").and_then(|()| {
            each_node(&model, |node| {
                writeln!(
                    out,
                    "{},{},{},{},{},{},{}",
                    csv_field(&model.display_path(node)),
                    kind(&model, node),
                    model.logical(node),
                    model.allocated(node),
                    model.files(node),
                    iso_time(model.modified(node)),
                    model.file_id(node),
                )
            })
        })
    } else {
        let mut first = true;
        writeln!(out, "[")
            .and_then(|()| {
                each_node(&model, |node| {
                    let row = json!({
                        "path": model.display_path(node),
                        "kind": kind(&model, node),
                        "logical": model.logical(node),
                        "allocated": model.allocated(node),
                        "files": model.files(node),
                        "modified": iso_time(model.modified(node)),
                    });
                    let separator = if first { "" } else { ",\n" };
                    first = false;
                    write!(out, "{separator}{row}")
                })
            })
            .and_then(|()| writeln!(out, "\n]"))
    };
    written
        .and_then(|()| out.flush())
        .map_err(|error| error.to_string())
}

fn demo(path: &str) -> Result<(), String> {
    let layout = ImageLayout {
        bytes_per_cluster: 4096,
        bytes_per_record: 1024,
        record_count: demo_record_count(),
        fragments: 1,
    };
    let file = File::create(path).map_err(|error| format!("{path}: {error}"))?;
    let mut out = BufWriter::with_capacity(1 << 20, file);
    let mut records = Vec::new();
    for_each_demo_record(|number, spec| records.push((number, spec)));
    write_image(&mut out, &layout, records).map_err(|error| format!("{path}: {error}"))
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

/// Prints what one record holds, without any names. For looking into how a
/// particular file is stored.
fn show_record(path: &str, number: &str) -> Result<(), String> {
    let number: u32 = number
        .parse()
        .map_err(|_| format!("bad record number: {number}"))?;
    let path = Path::new(path);
    let describe = |error: std::io::Error| format!("{}: {error}", path.display());
    let record = if ReplaySource::is_capture(path).map_err(describe)? {
        let replay = ReplaySource::open(path).map_err(describe)?;
        read_record(&replay, &replay.geometry(), number)
    } else {
        let image = ImageFile::open(path).map_err(describe)?;
        let mut boot = vec![0_u8; 4096];
        image.read_exact_at(0, &mut boot).map_err(describe)?;
        let geometry = Geometry::from_boot_sector(&boot).map_err(|error| error.to_string())?;
        read_record(&image, &geometry, number)
    }
    .map_err(|error| error.to_string())?;

    let header = RecordHeader::parse(&record).map_err(|error| format!("{error:?}"))?;
    let attributes: Vec<Value> = Attributes::new(&record, &header)
        .map(|attribute| match attribute.body {
            AttributeBody::Resident(value) => json!({
                "type": format!("{:#x}", attribute.type_code),
                "named": !attribute.name.is_empty(),
                "resident_bytes": value.len(),
                "reparse_tag": (attribute.type_code == ATTR_REPARSE_POINT)
                    .then(|| reparse_tag(value).map(|tag| format!("{tag:#x}"))),
            }),
            AttributeBody::NonResident(extent) => json!({
                "type": format!("{:#x}", attribute.type_code),
                "named": !attribute.name.is_empty(),
                "lowest_vcn": extent.lowest_vcn,
                "real_size": extent.real_size,
                "allocated_size": extent.allocated_size,
                "total_allocated": extent.total_allocated,
            }),
        })
        .collect();
    print_json(&json!({
        "record": number,
        "flags": header.flags,
        "links": header.link_count,
        "base_record": header.base_record,
        "bytes_used": header.bytes_used,
        "attributes": attributes,
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

    #[test]
    fn csv_fields_are_quoted_only_when_needed() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn file_times_become_utc_dates() {
        assert_eq!(iso_time(0), "");
        // The Unix epoch, then the leap day of 2024 at noon.
        assert_eq!(iso_time(116_444_736_000_000_000), "1970-01-01T00:00:00Z");
        assert_eq!(iso_time(133_536_816_000_000_000), "2024-02-29T12:00:00Z");
    }
}
