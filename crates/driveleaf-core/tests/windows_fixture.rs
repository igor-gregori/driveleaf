//! Checks the scanner against a volume written by Windows itself, built by
//! `fixtures/build.ps1`. It holds what no other test can: WOF compression,
//! junctions, symbolic links and metafiles the way Windows lays them out.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use driveleaf_core::model::{FLAG_DIRECTORY, FLAG_HARD_LINK, FLAG_LINK, Model, NodeId, ROOT};
use driveleaf_core::scanner::scan_image;
use driveleaf_ntfs::ScanControl;
use flate2::read::GzDecoder;
use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

/// The tree, or `None` when the fixture has not been built yet.
fn load() -> Option<(Model, Value)> {
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
    let paths = index_paths(&model);
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
fn the_total_matches_what_windows_says_is_used() {
    let Some((model, manifest)) = load() else {
        return;
    };
    let used = manifest["used_bytes"].as_u64().unwrap() as f64;
    let total = model.allocated(ROOT) as f64;
    let gap = (total - used).abs() / used;
    assert!(gap < 0.01, "scanned {total}, Windows says {used}");
    assert_eq!(model.summary().problems, 0);
}

#[test]
fn hard_links_count_once() {
    let Some((model, _)) = load() else {
        return;
    };
    let paths = index_paths(&model);
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
    let paths = index_paths(&model);
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
    let carrier = index_paths(&model)["streams\\carrier.txt"];
    assert!(model.logical(carrier) < 100);
    assert!(model.allocated(carrier) >= 200_000);
}

#[test]
fn links_are_never_followed() {
    let Some((model, _)) = load() else {
        return;
    };
    let paths = index_paths(&model);
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
    let paths = index_paths(&model);
    assert!(paths.contains_key("$NTFS metadata\\$MFT"));
    assert!(paths.contains_key("$NTFS metadata\\$LogFile"));
    assert!(!paths.contains_key("$MFT"));
}

#[test]
fn freed_records_are_gone() {
    let Some((model, _)) = load() else {
        return;
    };
    let paths = index_paths(&model);
    assert!(paths.contains_key("deleted\\gone-1.bin"));
    assert!(!paths.contains_key("deleted\\gone-2.bin"));
}
