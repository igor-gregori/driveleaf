//! Checks the scanner against a volume written by Windows itself, built by
//! `fixtures/build.ps1`. It holds what no other test can: WOF compression,
//! junctions, symbolic links and metafiles the way Windows lays them out.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use driveleaf_core::model::{FLAG_DIRECTORY, FLAG_HARD_LINK, FLAG_LINK, Model, NodeId, ROOT};
use driveleaf_core::scanner::scan_image;
use driveleaf_ntfs::record::{ATTR_DATA, AttributeBody, Attributes, RecordHeader};
use driveleaf_ntfs::{Geometry, Runs, ScanControl, read_record};
use flate2::read::GzDecoder;
use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

/// The tree, or `None` when the fixture has not been built yet.
fn load() -> Option<&'static (Model, Value)> {
    static LOADED: OnceLock<Option<(Model, Value)>> = OnceLock::new();
    LOADED.get_or_init(unpack_and_scan).as_ref()
}

fn unpack_and_scan() -> Option<(Model, Value)> {
    let image = fixture("windows-ntfs.img.gz");
    let manifest = fixture("windows-ntfs.manifest.json");
    if !image.exists() || !manifest.exists() {
        eprintln!("skipped: run fixtures/build.ps1 as administrator to build the fixture");
        return None;
    }
    let raw = std::env::temp_dir().join(format!("driveleaf-fixture-{}.img", std::process::id()));
    let mut input = GzDecoder::new(File::open(image).unwrap());
    let mut output = File::create(&raw).unwrap();
    io::copy(&mut input, &mut output).unwrap();
    output.flush().unwrap();
    drop(output);

    let model = scan_image(&raw, &ScanControl::default()).unwrap();
    std::fs::remove_file(&raw).unwrap();

    let mut text = String::new();
    File::open(manifest)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    Some((model, serde_json::from_str(&text).unwrap()))
}

/// Bytes the volume's own `$Bitmap` marks as used.
fn bytes_marked_used() -> u64 {
    const BITMAP_RECORD: u32 = 6;

    let mut image = Vec::new();
    GzDecoder::new(File::open(fixture("windows-ntfs.img.gz")).unwrap())
        .read_to_end(&mut image)
        .unwrap();
    let geometry = Geometry::from_boot_sector(&image[..512]).unwrap();
    let record = read_record(&image[..], &geometry, BITMAP_RECORD).unwrap();
    let header = RecordHeader::parse(&record).unwrap();
    let data = Attributes::new(&record, &header)
        .find(|attribute| attribute.type_code == ATTR_DATA)
        .unwrap();
    let AttributeBody::NonResident(extent) = data.body else {
        panic!("the bitmap of this volume does not fit in its record");
    };
    let cluster = geometry.bytes_per_cluster as usize;
    let mut bits = Vec::new();
    for run in Runs::new(extent.runs) {
        let start = run.lcn.unwrap() as usize * cluster;
        bits.extend_from_slice(&image[start..start + run.clusters as usize * cluster]);
    }
    let marked = (0..geometry.total_clusters as usize)
        .filter(|&index| bits[index / 8] >> (index % 8) & 1 != 0)
        .count();
    (marked * cluster) as u64
}

fn index_paths(model: &Model) -> HashMap<String, NodeId> {
    (0..model.len() as NodeId)
        .map(|node| {
            let path = model.display_path(node);
            (path.trim_start_matches('\\').to_owned(), node)
        })
        .collect()
}

#[test]
fn everything_windows_lists_is_in_the_tree_with_its_size() {
    let Some((model, manifest)) = load() else {
        return;
    };
    let paths = index_paths(model);
    let entries = manifest["entries"].as_array().unwrap();
    assert!(entries.len() > 500);
    for entry in entries {
        let path = entry["path"].as_str().unwrap();
        let node = *paths
            .get(path)
            .unwrap_or_else(|| panic!("{path} is missing from the tree"));
        let is_directory = entry["directory"].as_bool().unwrap();
        assert_eq!(
            model.flags(node) & FLAG_DIRECTORY != 0,
            is_directory,
            "{path}"
        );
        if !is_directory {
            assert_eq!(
                model.logical(node),
                entry["size"].as_u64().unwrap(),
                "{path}"
            );
        }
    }
}

#[test]
fn the_total_is_what_the_volume_marks_as_used() {
    let Some((model, _)) = load() else {
        return;
    };
    assert_eq!(model.allocated(ROOT), bytes_marked_used());
    assert_eq!(model.summary().problems, 0);
}

#[test]
fn hard_links_count_once() {
    let Some((model, _)) = load() else {
        return;
    };
    let paths = index_paths(model);
    let names = [
        "links\\original.bin",
        "links\\second.bin",
        "docs\\third.bin",
    ];
    let nodes: Vec<NodeId> = names.iter().map(|name| paths[*name]).collect();
    let counted = nodes
        .iter()
        .filter(|&&node| model.flags(node) & FLAG_HARD_LINK == 0)
        .count();
    assert_eq!(counted, 1);
    for &node in &nodes {
        assert_eq!(model.logical(node), 123_456);
    }
}

#[test]
fn compression_and_sparse_files_report_real_clusters() {
    let Some((model, _)) = load() else {
        return;
    };
    let paths = index_paths(model);
    for name in ["compressed\\ntfs.txt", "compressed\\wof.txt"] {
        let node = paths[name];
        assert_eq!(model.logical(node), 960_000, "{name}");
        assert!(model.allocated(node) < 200_000, "{name} is not compressed");
        assert!(model.allocated(node) > 0, "{name} lost its data");
    }
    let hole = paths["sparse\\hole.bin"];
    assert_eq!(model.logical(hole), 20 << 20);
    assert!(model.allocated(hole) < 1 << 20);
}

#[test]
fn alternate_streams_count_on_disk() {
    let Some((model, _)) = load() else {
        return;
    };
    let carrier = index_paths(model)["streams\\carrier.txt"];
    assert!(model.logical(carrier) < 100);
    assert!(model.allocated(carrier) >= 200_000);
}

#[test]
fn links_are_never_followed() {
    let Some((model, _)) = load() else {
        return;
    };
    let paths = index_paths(model);
    for name in ["junction", "symlink"] {
        let node = paths[name];
        assert_ne!(model.flags(node) & FLAG_LINK, 0, "{name}");
        assert_eq!(model.allocated(node), 0, "{name}");
    }
    assert_eq!(
        model.allocated(paths["target\\inside.bin"]),
        70_000_u64.next_multiple_of(4096)
    );
}

#[test]
fn metafiles_stay_under_their_own_node() {
    let Some((model, _)) = load() else {
        return;
    };
    let paths = index_paths(model);
    assert!(paths.contains_key("$NTFS metadata\\$MFT"));
    assert!(paths.contains_key("$NTFS metadata\\$LogFile"));
    assert!(!paths.contains_key("$MFT"));
}

#[test]
fn freed_records_are_gone() {
    let Some((model, _)) = load() else {
        return;
    };
    let paths = index_paths(model);
    assert!(paths.contains_key("deleted\\gone-1.bin"));
    assert!(!paths.contains_key("deleted\\gone-2.bin"));
}
