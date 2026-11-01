//! Parallel directory walk, the fallback when the MFT cannot be read: on other
//! file systems, on network drives, or without administrator rights.
//!
//! Symbolic links and junctions are never followed. Unreadable folders are
//! counted and skipped.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use driveleaf_ntfs::ScanControl;

#[cfg(windows)]
use crate::mft_tree::is_link_tag;
use crate::mft_tree::{ORPHANS_NAME, node_flags};
use crate::model::{
    FLAG_DIRECTORY, FLAG_HARD_LINK, FLAG_LINK, Model, NodeData, NodeId, ROOT, ScanMode,
    TreeBuilder, VolumeSummary,
};

/// One folder entry, on any platform.
struct Entry {
    name: OsString,
    is_directory: bool,
    is_link: bool,
    attributes: u32,
    logical: u64,
    allocated: u64,
    modified: u64,
    /// Identifies the file across hard links, or zero when unknown.
    identity: u64,
    /// Whether another name may point at the same file.
    may_be_linked: bool,
}

#[cfg(windows)]
fn list(path: &Path, track_links: bool, out: &mut Vec<Entry>) -> io::Result<()> {
    driveleaf_win::dir::list_directory(path, |entry| {
        out.push(Entry {
            is_directory: entry.is_directory(),
            is_link: is_link_tag(entry.reparse_tag),
            attributes: entry.attributes,
            logical: entry.logical,
            allocated: entry.allocated,
            modified: entry.modified,
            identity: entry.file_id,
            may_be_linked: track_links && !entry.is_directory(),
            name: entry.name,
        });
    })
}

#[cfg(not(windows))]
fn list(path: &Path, track_links: bool, out: &mut Vec<Entry>) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;

    /// Seconds between 1601 and 1970, to store times like NTFS does.
    const UNIX_EPOCH_IN_1601: u64 = 11_644_473_600;

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        let kind = metadata.file_type();
        let seconds = u64::try_from(metadata.mtime()).unwrap_or(0) + UNIX_EPOCH_IN_1601;
        out.push(Entry {
            name: entry.file_name(),
            is_directory: kind.is_dir(),
            is_link: kind.is_symlink(),
            attributes: 0,
            logical: if kind.is_dir() { 0 } else { metadata.len() },
            allocated: if kind.is_dir() {
                0
            } else {
                metadata.blocks() * 512
            },
            modified: seconds * 10_000_000,
            identity: metadata.ino(),
            may_be_linked: track_links && metadata.nlink() > 1,
        });
    }
    Ok(())
}

struct State {
    tree: TreeBuilder,
    /// Files seen so far, to count hard links once.
    seen: HashSet<u64>,
}

struct Shared<'a> {
    state: Mutex<State>,
    problems: AtomicU64,
    control: &'a ScanControl,
    track_links: bool,
}

fn visit_folder<'s>(path: PathBuf, node: NodeId, shared: &'s Shared<'_>, scope: &rayon::Scope<'s>) {
    if shared.control.is_cancelled() {
        return;
    }
    let mut entries = Vec::new();
    if list(&path, shared.track_links, &mut entries).is_err() {
        shared.problems.fetch_add(1, Ordering::Relaxed);
    }
    shared.control.advance(entries.len() as u64);

    let mut folders = Vec::new();
    {
        let mut state = shared.state.lock().expect("no walker panicked");
        for entry in &entries {
            let mut flags = node_flags(entry.attributes, 0);
            if entry.is_directory {
                flags |= FLAG_DIRECTORY;
            }
            if entry.is_link {
                flags |= FLAG_LINK;
            }
            if entry.may_be_linked && !state.seen.insert(entry.identity) {
                flags |= FLAG_HARD_LINK;
            }
            let child = state.tree.add(
                node,
                entry.name.as_encoded_bytes(),
                NodeData {
                    flags,
                    file_id: entry.identity as u32,
                    logical: entry.logical,
                    allocated: if entry.is_directory {
                        0
                    } else {
                        entry.allocated
                    },
                    modified: entry.modified,
                },
            );
            if entry.is_directory && !entry.is_link {
                folders.push((path.join(&entry.name), child));
            }
        }
    }
    for (folder, child) in folders {
        scope.spawn(move |scope| visit_folder(folder, child, shared, scope));
    }
}

/// Walks `root` into a [`Model`]. Set `track_links` only where file IDs identify
/// a file for sure, as they do on NTFS. Returns `None` when cancelled.
pub fn walk(
    root: &Path,
    root_name: &str,
    track_links: bool,
    used_bytes: Option<u64>,
    total_bytes: Option<u64>,
    control: &ScanControl,
) -> Option<Model> {
    let shared = Shared {
        state: Mutex::new(State {
            tree: TreeBuilder::new(root_name, 1 << 16, 1 << 20),
            seen: HashSet::new(),
        }),
        problems: AtomicU64::new(0),
        control,
        track_links,
    };
    rayon::scope(|scope| visit_folder(root.to_path_buf(), ROOT, &shared, scope));
    if control.is_cancelled() {
        return None;
    }
    let summary = VolumeSummary {
        mode: ScanMode::Walk,
        used_bytes,
        total_bytes,
        problems: shared.problems.into_inner(),
    };
    let state = shared.state.into_inner().expect("no walker panicked");
    Some(state.tree.finish(ORPHANS_NAME, summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sandbox(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("driveleaf-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("one.bin"), [0_u8; 10]).unwrap();
        fs::write(root.join("a/two.bin"), [0_u8; 20]).unwrap();
        fs::write(root.join("a/b/three.bin"), [0_u8; 30]).unwrap();
        root
    }

    fn run(root: &Path, control: &ScanControl) -> Option<Model> {
        walk(root, "root", true, None, None, control)
    }

    fn find(model: &Model, name: &str) -> NodeId {
        (0..model.len() as NodeId)
            .find(|&node| model.name(node) == name)
            .unwrap_or_else(|| panic!("{name} is not in the tree"))
    }

    #[test]
    fn builds_the_same_kind_of_tree_as_a_scan() {
        let root = sandbox("tree");
        let model = run(&root, &ScanControl::default()).unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(model.files(ROOT), 3);
        assert_eq!(model.logical(ROOT), 60);
        assert_eq!(model.summary().mode, ScanMode::Walk);
        assert_eq!(model.summary().problems, 0);
        assert_eq!(model.logical(find(&model, "a")), 50);
        assert_eq!(
            model.display_path(find(&model, "three.bin")),
            "root\\a\\b\\three.bin"
        );
    }

    #[cfg(unix)]
    #[test]
    fn links_are_listed_but_not_followed() {
        let root = sandbox("links");
        std::os::unix::fs::symlink(root.join("a"), root.join("shortcut")).unwrap();
        let model = run(&root, &ScanControl::default()).unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            model.files(ROOT),
            4,
            "the link itself, not what it leads to"
        );
        assert_ne!(model.flags(find(&model, "shortcut")) & FLAG_LINK, 0);
    }

    #[test]
    fn hard_links_count_once() {
        let root = sandbox("hardlinks");
        fs::hard_link(root.join("a/two.bin"), root.join("twin.bin")).unwrap();
        let model = run(&root, &ScanControl::default()).unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(model.files(ROOT), 3);
        assert_eq!(model.logical(ROOT), 60);
    }

    #[test]
    fn a_missing_root_counts_as_a_problem() {
        let model = run(Path::new("/definitely/not/here"), &ScanControl::default()).unwrap();
        assert_eq!(model.summary().problems, 1);
        assert_eq!(model.files(ROOT), 0);
    }

    #[test]
    fn a_cancelled_walk_returns_nothing() {
        let root = sandbox("cancel");
        let control = ScanControl::default();
        control.cancel();
        assert!(run(&root, &control).is_none());
        fs::remove_dir_all(&root).unwrap();
    }
}
