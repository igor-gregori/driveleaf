//! Decides what the app may send to the Recycle Bin.
//!
//! The app often runs as administrator, where Windows itself would allow almost
//! anything. So the rules live here, in portable code with tests, and they run
//! on node IDs from the scan, never on text from the interface.

use crate::model::{FLAG_DELETED, FLAG_METAFILE, FLAG_SYNTHETIC, Model, NodeId, ROOT};

/// Why a node cannot be removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The volume itself.
    VolumeRoot,
    /// A node the scanner made up, with no path on disk.
    NotOnDisk,
    /// NTFS's own bookkeeping.
    Metafile,
    /// Gone already.
    AlreadyRemoved,
    /// Part of Windows, or a folder Windows needs in place.
    Protected,
    /// A name that Windows paths cannot express, as a forged volume could hold.
    UnsafeName,
}

impl Refusal {
    /// Stable name for the frontend, which translates it.
    pub fn key(self) -> &'static str {
        match self {
            Refusal::VolumeRoot => "volumeRoot",
            Refusal::NotOnDisk => "notOnDisk",
            Refusal::Metafile => "metafile",
            Refusal::AlreadyRemoved => "alreadyRemoved",
            Refusal::Protected => "protected",
            Refusal::UnsafeName => "unsafeName",
        }
    }
}

/// Top-level folders with nothing in them that is safe to remove by hand.
const PROTECTED_TREES: [&[u8]; 8] = [
    b"windows",
    b"$recycle.bin",
    b"system volume information",
    b"recovery",
    b"boot",
    b"efi",
    b"$windows.~bt",
    b"$winreagent",
];

/// Top-level entries that must stay, although their contents belong to the user.
const PROTECTED_ENTRIES: [&[u8]; 9] = [
    b"program files",
    b"program files (x86)",
    b"programdata",
    b"users",
    b"documents and settings",
    b"pagefile.sys",
    b"hiberfil.sys",
    b"swapfile.sys",
    b"bootmgr",
];

/// True for a name Windows itself could produce as one path component.
///
/// The MFT can hold anything, so a forged volume might name a file `..\\..\\x`
/// to reach outside its folder, or use a quote to break out of a command line.
fn is_plain_name(name: &[u8]) -> bool {
    !name.is_empty()
        && name != b"."
        && name != b".."
        && !name
            .iter()
            .any(|&byte| byte < 0x20 || b"\\/:*?\"<>|".contains(&byte))
}

/// Checks that every name on the way to a node is safe to put in a path.
pub fn check_path(model: &Model, node: NodeId) -> Result<(), Refusal> {
    let path = model.ancestry(node);
    if path[1..]
        .iter()
        .all(|&step| is_plain_name(model.name_bytes(step)))
    {
        Ok(())
    } else {
        Err(Refusal::UnsafeName)
    }
}

fn lower(model: &Model, node: NodeId) -> Vec<u8> {
    model.name_bytes(node).to_ascii_lowercase()
}

/// Checks whether a node may go to the Recycle Bin.
pub fn check_removable(model: &Model, node: NodeId) -> Result<(), Refusal> {
    if node == ROOT {
        return Err(Refusal::VolumeRoot);
    }
    let flags = model.flags(node);
    if flags & FLAG_DELETED != 0 {
        return Err(Refusal::AlreadyRemoved);
    }
    if flags & FLAG_SYNTHETIC != 0 {
        return Err(Refusal::NotOnDisk);
    }
    if flags & FLAG_METAFILE != 0 {
        return Err(Refusal::Metafile);
    }
    check_path(model, node)?;
    let path = model.ancestry(node);
    let top = lower(model, path[1]);
    if PROTECTED_TREES.contains(&top.as_slice()) {
        return Err(Refusal::Protected);
    }
    if path.len() == 2 && PROTECTED_ENTRIES.contains(&top.as_slice()) {
        return Err(Refusal::Protected);
    }
    // A user's profile folder holds their whole account.
    if path.len() == 3 && top == b"users" {
        return Err(Refusal::Protected);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FLAG_DIRECTORY, NodeData, ScanMode, TreeBuilder, VolumeSummary};

    fn folder() -> NodeData {
        NodeData {
            flags: FLAG_DIRECTORY,
            ..NodeData::default()
        }
    }

    fn file() -> NodeData {
        NodeData {
            allocated: 100,
            ..NodeData::default()
        }
    }

    struct Tree {
        model: Model,
    }

    impl Tree {
        fn find(&self, path: &str) -> NodeId {
            (0..self.model.len() as NodeId)
                .find(|&node| self.model.display_path(node) == path)
                .unwrap_or_else(|| panic!("{path} is not in the tree"))
        }

        fn check(&self, path: &str) -> Result<(), Refusal> {
            check_removable(&self.model, self.find(path))
        }
    }

    fn tree() -> Tree {
        let mut tree = TreeBuilder::new("C:\\", 32, 256);
        let windows = tree.add(ROOT, b"WINDOWS", folder());
        let system = tree.add(windows, b"System32", folder());
        tree.add(system, b"kernel32.dll", file());
        tree.add(ROOT, b"pagefile.sys", file());
        tree.add(ROOT, b"Program Files", folder());
        let programs = tree.add(ROOT, b"Program Files (x86)", folder());
        let app = tree.add(programs, b"Some App", folder());
        tree.add(app, b"app.exe", file());
        let users = tree.add(ROOT, b"Users", folder());
        let profile = tree.add(users, b"maria", folder());
        let downloads = tree.add(profile, b"Downloads", folder());
        tree.add(downloads, b"setup.exe", file());
        let data = tree.add(ROOT, b"Dados", folder());
        tree.add(data, b"filme.mkv", file());
        // A folder named like a protected one, but somewhere else.
        tree.add(data, b"windows", folder());
        tree.add(
            ROOT,
            b"$MFT",
            NodeData {
                flags: FLAG_METAFILE,
                ..file()
            },
        );
        tree.add(
            ROOT,
            b"<Orphaned files>",
            NodeData {
                flags: FLAG_SYNTHETIC | FLAG_DIRECTORY,
                ..NodeData::default()
            },
        );
        Tree {
            model: tree.finish(
                "<orphans>",
                VolumeSummary {
                    mode: ScanMode::Mft,
                    used_bytes: None,
                    total_bytes: None,
                    problems: 0,
                },
            ),
        }
    }

    #[test]
    fn ordinary_files_and_folders_are_removable() {
        let tree = tree();
        assert_eq!(tree.check("C:\\Users\\maria\\Downloads\\setup.exe"), Ok(()));
        assert_eq!(tree.check("C:\\Users\\maria\\Downloads"), Ok(()));
        assert_eq!(tree.check("C:\\Dados"), Ok(()));
        assert_eq!(tree.check("C:\\Dados\\filme.mkv"), Ok(()));
        assert_eq!(tree.check("C:\\Program Files (x86)\\Some App"), Ok(()));
        assert_eq!(tree.check("C:\\Dados\\windows"), Ok(()));
    }

    #[test]
    fn the_volume_and_made_up_nodes_are_refused() {
        let tree = tree();
        assert_eq!(check_removable(&tree.model, ROOT), Err(Refusal::VolumeRoot));
        assert_eq!(tree.check("C:\\<Orphaned files>"), Err(Refusal::NotOnDisk));
        assert_eq!(tree.check("C:\\$MFT"), Err(Refusal::Metafile));
    }

    #[test]
    fn windows_is_protected_all_the_way_down_whatever_the_case() {
        let tree = tree();
        assert_eq!(tree.check("C:\\WINDOWS"), Err(Refusal::Protected));
        assert_eq!(tree.check("C:\\WINDOWS\\System32"), Err(Refusal::Protected));
        assert_eq!(
            tree.check("C:\\WINDOWS\\System32\\kernel32.dll"),
            Err(Refusal::Protected)
        );
    }

    #[test]
    fn system_entries_and_profile_folders_are_protected() {
        let tree = tree();
        assert_eq!(tree.check("C:\\pagefile.sys"), Err(Refusal::Protected));
        assert_eq!(tree.check("C:\\Program Files"), Err(Refusal::Protected));
        assert_eq!(
            tree.check("C:\\Program Files (x86)"),
            Err(Refusal::Protected)
        );
        assert_eq!(tree.check("C:\\Users"), Err(Refusal::Protected));
        assert_eq!(tree.check("C:\\Users\\maria"), Err(Refusal::Protected));
    }

    #[test]
    fn names_that_escape_their_folder_are_refused() {
        let mut builder = TreeBuilder::new("D:\\", 8, 64);
        let good = builder.add(ROOT, b"pasta", folder());
        let cases: [&[u8]; 8] = [
            b"..",
            b".",
            b"..\\..\\Windows",
            b"a/b",
            b"stream:hidden",
            b"quo\"te",
            b"line\nbreak",
            b"",
        ];
        let forged: Vec<NodeId> = cases
            .iter()
            .map(|name| builder.add(good, name, file()))
            .collect();
        let below = builder.add(forged[0], b"child.txt", file());
        let fine = builder.add(good, b"relat\xC3\xB3rio final (1).txt", file());
        let model = builder.finish(
            "<orphans>",
            VolumeSummary {
                mode: ScanMode::Mft,
                used_bytes: None,
                total_bytes: None,
                problems: 0,
            },
        );
        for node in forged {
            assert_eq!(check_path(&model, node), Err(Refusal::UnsafeName));
            assert_eq!(check_removable(&model, node), Err(Refusal::UnsafeName));
        }
        assert_eq!(check_path(&model, below), Err(Refusal::UnsafeName));
        assert_eq!(check_path(&model, fine), Ok(()));
        assert_eq!(check_path(&model, ROOT), Ok(()));
    }

    #[test]
    fn removed_nodes_cannot_be_removed_again() {
        let mut tree = tree();
        let node = tree.find("C:\\Dados\\filme.mkv");
        tree.model.mark_deleted(node);
        assert_eq!(
            check_removable(&tree.model, node),
            Err(Refusal::AlreadyRemoved)
        );
    }
}
