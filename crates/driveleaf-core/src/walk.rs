//! Parallel directory walk, the fallback when the MFT cannot be read.
//!
//! Symlinks and junctions are never followed. Unreadable entries are counted
//! and skipped.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Totals gathered by [`walk`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WalkStats {
    pub files: u64,
    pub directories: u64,
    pub logical_bytes: u64,
    pub errors: u64,
}

#[derive(Default)]
struct Counters {
    files: AtomicU64,
    directories: AtomicU64,
    logical_bytes: AtomicU64,
    errors: AtomicU64,
}

/// Walks `root` in parallel and returns its totals.
pub fn walk(root: &Path) -> WalkStats {
    let counters = Counters::default();
    rayon::scope(|scope| visit_dir(root.to_path_buf(), &counters, scope));
    WalkStats {
        files: counters.files.into_inner(),
        directories: counters.directories.into_inner(),
        logical_bytes: counters.logical_bytes.into_inner(),
        errors: counters.errors.into_inner(),
    }
}

fn visit_dir<'s>(dir: std::path::PathBuf, counters: &'s Counters, scope: &rayon::Scope<'s>) {
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => {
            counters.errors.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    counters.directories.fetch_add(1, Ordering::Relaxed);
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                counters.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => {
                let path = entry.path();
                scope.spawn(move |scope| visit_dir(path, counters, scope));
            }
            Ok(kind) if kind.is_symlink() => {
                counters.files.fetch_add(1, Ordering::Relaxed);
            }
            Ok(_) => {
                counters.files.fetch_add(1, Ordering::Relaxed);
                match entry.metadata() {
                    Ok(metadata) => {
                        counters
                            .logical_bytes
                            .fetch_add(metadata.len(), Ordering::Relaxed);
                    }
                    Err(_) => {
                        counters.errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            Err(_) => {
                counters.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_files_directories_and_bytes() {
        let root = std::env::temp_dir().join(format!("driveleaf-walk-{}", std::process::id()));
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("one.bin"), [0_u8; 10]).unwrap();
        fs::write(root.join("a/two.bin"), [0_u8; 20]).unwrap();
        fs::write(root.join("a/b/three.bin"), [0_u8; 30]).unwrap();

        let stats = walk(&root);
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(stats.files, 3);
        assert_eq!(stats.directories, 3);
        assert_eq!(stats.logical_bytes, 60);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn a_missing_root_counts_as_an_error() {
        let stats = walk(Path::new("/definitely/not/here"));
        assert_eq!(stats.errors, 1);
        assert_eq!(stats.files, 0);
    }
}
