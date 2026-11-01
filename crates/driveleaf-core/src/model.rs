//! The in-memory disk usage model.
//!
//! Everything lives in contiguous vectors indexed by `u32`, with all names in a
//! single pool and no allocation per entry, so millions of entries stay cheap to
//! hold and instant to drop. The tree is immutable after aggregation. Deleting a
//! node only flags it and adjusts the totals of its ancestors.

use rayon::prelude::*;

use crate::categories::{Category, categorize};

/// Index of a node. Handed to the frontend instead of any path.
pub type NodeId = u32;

/// The volume or folder that was scanned.
pub const ROOT: NodeId = 0;

pub const FLAG_DIRECTORY: u16 = 1 << 0;
/// Made up by the scanner: it has no path on disk.
pub const FLAG_SYNTHETIC: u16 = 1 << 1;
/// NTFS's own bookkeeping, which is never a target for actions.
pub const FLAG_METAFILE: u16 = 1 << 2;
/// A further name of a file already counted at its first name. It keeps the
/// file's sizes for display but adds nothing to any total.
pub const FLAG_HARD_LINK: u16 = 1 << 3;
/// A symbolic link or junction, which is never followed.
pub const FLAG_LINK: u16 = 1 << 4;
/// A cloud placeholder whose content is not on this disk.
pub const FLAG_CLOUD: u16 = 1 << 5;
pub const FLAG_COMPRESSED: u16 = 1 << 6;
pub const FLAG_SPARSE: u16 = 1 << 7;
/// Has alternate data streams, included in its allocated size.
pub const FLAG_STREAMS: u16 = 1 << 8;
pub const FLAG_HIDDEN: u16 = 1 << 9;
pub const FLAG_SYSTEM: u16 = 1 << 10;
/// Removed by the user after the scan.
pub const FLAG_DELETED: u16 = 1 << 15;

/// How the tree was obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanMode {
    /// Read from the Master File Table: complete and exact.
    Mft,
    /// Walked folder by folder: slower, and blind to what access rights hide.
    Walk,
}

/// What is known about the scanned volume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeSummary {
    pub mode: ScanMode,
    /// Bytes in use on the volume, when known.
    pub used_bytes: Option<u64>,
    /// Bytes on the whole volume, when known.
    pub total_bytes: Option<u64>,
    /// Entries that could not be read or decoded.
    pub problems: u64,
}

/// Values for one new node.
#[derive(Clone, Copy, Debug, Default)]
pub struct NodeData {
    pub flags: u16,
    /// MFT record or file ID, used to confirm identity before acting on a file.
    pub file_id: u32,
    pub logical: u64,
    pub allocated: u64,
    /// Last modification, in 100 ns ticks since 1601.
    pub modified: u64,
}

/// Collects nodes, then aggregates them into a [`Model`].
pub struct TreeBuilder {
    parent: Vec<u32>,
    name_start: Vec<u32>,
    name_len: Vec<u16>,
    flags: Vec<u16>,
    file_id: Vec<u32>,
    logical: Vec<u64>,
    allocated: Vec<u64>,
    modified: Vec<u64>,
    pool: Vec<u8>,
}

impl TreeBuilder {
    /// Starts a tree whose root displays as `root_name`.
    pub fn new(root_name: &str, nodes: usize, name_bytes: usize) -> Self {
        let mut builder = Self {
            parent: Vec::with_capacity(nodes),
            name_start: Vec::with_capacity(nodes),
            name_len: Vec::with_capacity(nodes),
            flags: Vec::with_capacity(nodes),
            file_id: Vec::with_capacity(nodes),
            logical: Vec::with_capacity(nodes),
            allocated: Vec::with_capacity(nodes),
            modified: Vec::with_capacity(nodes),
            pool: Vec::with_capacity(name_bytes + root_name.len()),
        };
        builder.push(
            ROOT,
            root_name.as_bytes(),
            NodeData {
                flags: FLAG_DIRECTORY,
                ..NodeData::default()
            },
        );
        builder
    }

    pub fn len(&self) -> usize {
        self.parent.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parent.is_empty()
    }

    /// Adds a node under `parent`, which may be added later.
    pub fn add(&mut self, parent: NodeId, name: &[u8], data: NodeData) -> NodeId {
        self.push(parent, name, data)
    }

    fn push(&mut self, parent: NodeId, name: &[u8], data: NodeData) -> NodeId {
        let id = self.parent.len() as NodeId;
        let len = name.len().min(u16::MAX as usize);
        self.parent.push(parent);
        self.name_start.push(self.pool.len() as u32);
        self.name_len.push(len as u16);
        self.pool.extend_from_slice(&name[..len]);
        self.flags.push(data.flags);
        self.file_id.push(data.file_id);
        self.logical.push(data.logical);
        self.allocated.push(data.allocated);
        self.modified.push(data.modified);
        id
    }

    /// Adds sizes to an existing node, such as a directory's own overhead.
    pub fn add_sizes(&mut self, node: NodeId, logical: u64, allocated: u64) {
        let index = node as usize;
        self.logical[index] = self.logical[index].saturating_add(logical);
        self.allocated[index] = self.allocated[index].saturating_add(allocated);
    }

    /// Replaces the sizes and details of an existing node.
    pub fn update(&mut self, node: NodeId, data: NodeData) {
        let index = node as usize;
        self.flags[index] |= data.flags;
        self.file_id[index] = data.file_id;
        self.modified[index] = data.modified;
        self.add_sizes(node, data.logical, data.allocated);
    }

    /// Links the tree, aggregates it and sorts every folder by size.
    ///
    /// Nodes that do not lead up to the root, as a cycle in a damaged volume
    /// would, move under a new child of the root called `orphan_name`, so their
    /// space is still counted.
    pub fn finish(mut self, orphan_name: &str, summary: VolumeSummary) -> Model {
        let (mut child_start, mut children) = link_children(&self.parent);
        let mut order = breadth_first(&child_start, &children);
        if order.len() < self.parent.len() {
            let mut reached = vec![false; self.parent.len()];
            for &node in &order {
                reached[node as usize] = true;
            }
            let orphans = self.push(
                ROOT,
                orphan_name.as_bytes(),
                NodeData {
                    flags: FLAG_DIRECTORY | FLAG_SYNTHETIC,
                    ..NodeData::default()
                },
            );
            for (node, reached) in reached.iter().enumerate() {
                if !reached {
                    self.parent[node] = orphans;
                }
            }
            (child_start, children) = link_children(&self.parent);
            order = breadth_first(&child_start, &children);
        }

        let mut files = vec![0_u32; self.parent.len()];
        for &node in &order {
            let index = node as usize;
            let parent = self.parent[index] as usize;
            if self.flags[parent] & FLAG_METAFILE != 0 {
                self.flags[index] |= FLAG_METAFILE;
            }
        }
        for &node in order.iter().rev() {
            let index = node as usize;
            let flags = self.flags[index];
            if flags & (FLAG_DIRECTORY | FLAG_HARD_LINK | FLAG_SYNTHETIC) == 0 {
                files[index] += 1;
            }
            if node == ROOT || flags & FLAG_HARD_LINK != 0 {
                continue;
            }
            let parent = self.parent[index] as usize;
            self.logical[parent] = self.logical[parent].saturating_add(self.logical[index]);
            self.allocated[parent] = self.allocated[parent].saturating_add(self.allocated[index]);
            files[parent] += files[index];
        }

        let weights: Vec<u64> = (0..self.parent.len())
            .map(|index| {
                if self.flags[index] & FLAG_HARD_LINK != 0 {
                    0
                } else {
                    self.allocated[index]
                }
            })
            .collect();
        sort_children(&child_start, &mut children, &weights, &self.logical);

        let category = (0..self.parent.len())
            .into_par_iter()
            .map(|index| {
                if self.flags[index] & FLAG_DIRECTORY != 0 {
                    return Category::Directory as u8;
                }
                let start = self.name_start[index] as usize;
                categorize(&self.pool[start..start + usize::from(self.name_len[index])]) as u8
            })
            .collect();

        Model {
            parent: self.parent,
            name_start: self.name_start,
            name_len: self.name_len,
            flags: self.flags,
            file_id: self.file_id,
            logical: self.logical,
            allocated: self.allocated,
            modified: self.modified,
            files,
            category,
            child_start,
            children,
            pool: self.pool,
            summary,
        }
    }
}

/// Builds the children index in one pass with a counting sort.
fn link_children(parent: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let count = parent.len();
    let mut child_start = vec![0_u32; count + 2];
    for &up in &parent[1..] {
        child_start[up as usize + 2] += 1;
    }
    for index in 2..child_start.len() {
        child_start[index] += child_start[index - 1];
    }
    let mut children = vec![0_u32; count - 1];
    for (node, &up) in parent.iter().enumerate().skip(1) {
        let slot = &mut child_start[up as usize + 1];
        children[*slot as usize] = node as u32;
        *slot += 1;
    }
    child_start.pop();
    (child_start, children)
}

fn breadth_first(child_start: &[u32], children: &[u32]) -> Vec<u32> {
    let mut order = Vec::with_capacity(child_start.len() - 1);
    order.push(ROOT);
    let mut head = 0;
    while head < order.len() {
        let node = order[head] as usize;
        head += 1;
        order.extend_from_slice(
            &children[child_start[node] as usize..child_start[node + 1] as usize],
        );
    }
    order
}

fn sort_children(child_start: &[u32], children: &mut [u32], weights: &[u64], logical: &[u64]) {
    let mut folders = Vec::new();
    let mut rest = children;
    for window in child_start.windows(2) {
        let count = (window[1] - window[0]) as usize;
        let (folder, tail) = rest.split_at_mut(count);
        rest = tail;
        if count > 1 {
            folders.push(folder);
        }
    }
    folders.par_iter_mut().for_each(|folder| {
        folder.sort_unstable_by_key(|&node| {
            let index = node as usize;
            (
                std::cmp::Reverse(weights[index]),
                std::cmp::Reverse(logical[index]),
                node,
            )
        });
    });
}

/// A scanned tree.
pub struct Model {
    parent: Vec<u32>,
    name_start: Vec<u32>,
    name_len: Vec<u16>,
    flags: Vec<u16>,
    file_id: Vec<u32>,
    /// Own size for files, subtree total for folders.
    logical: Vec<u64>,
    /// Own size for files, subtree total for folders.
    allocated: Vec<u64>,
    modified: Vec<u64>,
    /// Files in the subtree.
    files: Vec<u32>,
    category: Vec<u8>,
    /// Where each node's children start in `children`.
    child_start: Vec<u32>,
    /// Sorted by size within each folder.
    children: Vec<u32>,
    pool: Vec<u8>,
    summary: VolumeSummary,
}

impl Model {
    pub fn len(&self) -> usize {
        self.parent.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parent.is_empty()
    }

    pub fn contains(&self, node: NodeId) -> bool {
        (node as usize) < self.parent.len()
    }

    pub fn summary(&self) -> &VolumeSummary {
        &self.summary
    }

    pub fn parent(&self, node: NodeId) -> NodeId {
        self.parent[node as usize]
    }

    /// The name as WTF-8.
    pub fn name_bytes(&self, node: NodeId) -> &[u8] {
        let index = node as usize;
        let start = self.name_start[index] as usize;
        &self.pool[start..start + usize::from(self.name_len[index])]
    }

    pub fn name(&self, node: NodeId) -> String {
        String::from_utf8_lossy(self.name_bytes(node)).into_owned()
    }

    pub fn flags(&self, node: NodeId) -> u16 {
        self.flags[node as usize]
    }

    pub fn file_id(&self, node: NodeId) -> u32 {
        self.file_id[node as usize]
    }

    pub fn logical(&self, node: NodeId) -> u64 {
        self.logical[node as usize]
    }

    pub fn allocated(&self, node: NodeId) -> u64 {
        self.allocated[node as usize]
    }

    pub fn modified(&self, node: NodeId) -> u64 {
        self.modified[node as usize]
    }

    pub fn files(&self, node: NodeId) -> u32 {
        self.files[node as usize]
    }

    pub fn category(&self, node: NodeId) -> Category {
        Category::from_byte(self.category[node as usize])
    }

    pub fn is_directory(&self, node: NodeId) -> bool {
        self.flags(node) & FLAG_DIRECTORY != 0
    }

    pub fn is_deleted(&self, node: NodeId) -> bool {
        self.flags(node) & FLAG_DELETED != 0
    }

    /// The node's share of its folder and of the whole tree.
    pub fn weight(&self, node: NodeId) -> u64 {
        let flags = self.flags(node);
        if flags & (FLAG_HARD_LINK | FLAG_DELETED) != 0 {
            0
        } else {
            self.allocated(node)
        }
    }

    /// Children sorted by size, including deleted ones and empty synthetic nodes.
    pub fn raw_children(&self, node: NodeId) -> &[NodeId] {
        let index = node as usize;
        &self.children[self.child_start[index] as usize..self.child_start[index + 1] as usize]
    }

    /// Whether a node shows up in listings.
    pub fn is_visible(&self, node: NodeId) -> bool {
        let flags = self.flags(node);
        if flags & FLAG_DELETED != 0 {
            return false;
        }
        flags & FLAG_SYNTHETIC == 0 || self.allocated(node) > 0 || self.files(node) > 0
    }

    /// The node and everything above it, from the root down.
    pub fn ancestry(&self, node: NodeId) -> Vec<NodeId> {
        let mut chain = vec![node];
        let mut current = node;
        while current != ROOT {
            current = self.parent(current);
            chain.push(current);
        }
        chain.reverse();
        chain
    }

    /// The full path for display, with `\` between names.
    pub fn display_path(&self, node: NodeId) -> String {
        let mut path = self.name(ROOT);
        for &step in &self.ancestry(node)[1..] {
            if !path.ends_with(['\\', '/']) {
                path.push('\\');
            }
            path.push_str(&String::from_utf8_lossy(self.name_bytes(step)));
        }
        path
    }

    /// The raw WTF-8 path, which keeps names that are not valid Unicode.
    pub fn path_bytes(&self, node: NodeId) -> Vec<u8> {
        let mut path = self.name_bytes(ROOT).to_vec();
        for &step in &self.ancestry(node)[1..] {
            if !matches!(path.last(), Some(b'\\' | b'/')) {
                path.push(b'\\');
            }
            path.extend_from_slice(self.name_bytes(step));
        }
        path
    }

    /// Removes a node from the totals after it was deleted. Only the ancestors
    /// change, so this is cheap, and the rest of the tree stays valid.
    pub fn mark_deleted(&mut self, node: NodeId) {
        if node == ROOT || self.is_deleted(node) {
            return;
        }
        let index = node as usize;
        let counted = self.flags[index] & FLAG_HARD_LINK == 0;
        let (logical, allocated, files) = (
            self.logical[index],
            self.allocated[index],
            self.files[index],
        );
        self.flags[index] |= FLAG_DELETED;
        if !counted {
            return;
        }
        let mut current = node;
        while current != ROOT {
            current = self.parent(current);
            let up = current as usize;
            self.logical[up] = self.logical[up].saturating_sub(logical);
            self.allocated[up] = self.allocated[up].saturating_sub(allocated);
            self.files[up] = self.files[up].saturating_sub(files);
        }
    }

    /// Other visible names of the same hard-linked file.
    pub fn other_links(&self, node: NodeId) -> Vec<NodeId> {
        let id = self.file_id(node);
        (0..self.len() as NodeId)
            .into_par_iter()
            .filter(|&other| {
                other != node
                    && self.file_id[other as usize] == id
                    && self.flags[other as usize] & (FLAG_DIRECTORY | FLAG_SYNTHETIC | FLAG_DELETED)
                        == 0
            })
            .collect()
    }

    /// Heap bytes held by the model.
    pub fn memory_bytes(&self) -> usize {
        self.parent.capacity() * size_of::<u32>()
            + self.name_start.capacity() * size_of::<u32>()
            + self.name_len.capacity() * size_of::<u16>()
            + self.flags.capacity() * size_of::<u16>()
            + self.file_id.capacity() * size_of::<u32>()
            + self.logical.capacity() * size_of::<u64>()
            + self.allocated.capacity() * size_of::<u64>()
            + self.modified.capacity() * size_of::<u64>()
            + self.files.capacity() * size_of::<u32>()
            + self.category.capacity()
            + self.child_start.capacity() * size_of::<u32>()
            + self.children.capacity() * size_of::<u32>()
            + self.pool.capacity()
    }
}

impl driveleaf_treemap::TreeSource for Model {
    fn children(&self, node: u32) -> &[u32] {
        self.raw_children(node)
    }

    fn weight(&self, node: u32) -> u64 {
        Model::weight(self, node)
    }

    fn is_directory(&self, node: u32) -> bool {
        Model::is_directory(self, node)
    }

    fn category(&self, node: u32) -> u8 {
        self.category[node as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> VolumeSummary {
        VolumeSummary {
            mode: ScanMode::Walk,
            used_bytes: None,
            total_bytes: None,
            problems: 0,
        }
    }

    fn dir() -> NodeData {
        NodeData {
            flags: FLAG_DIRECTORY,
            ..NodeData::default()
        }
    }

    fn file(allocated: u64) -> NodeData {
        NodeData {
            logical: allocated - 1,
            allocated,
            ..NodeData::default()
        }
    }

    /// root/{a/{one.txt 100, two.bin 300}, b/{three.png 50}, top.zip 10}
    fn sample() -> Model {
        let mut tree = TreeBuilder::new("C:\\", 8, 64);
        let a = tree.add(ROOT, b"a", dir());
        tree.add(a, b"one.txt", file(100));
        tree.add(a, b"two.bin", file(300));
        let b = tree.add(ROOT, b"b", dir());
        tree.add(b, b"three.png", file(50));
        tree.add(ROOT, b"top.zip", file(10));
        tree.finish("<orphans>", summary())
    }

    #[test]
    fn totals_add_up_the_tree() {
        let model = sample();
        assert_eq!(model.allocated(ROOT), 460);
        assert_eq!(model.logical(ROOT), 456);
        assert_eq!(model.files(ROOT), 4);
        assert_eq!(model.allocated(1), 400);
        assert_eq!(model.files(1), 2);
    }

    #[test]
    fn children_come_sorted_by_size() {
        let model = sample();
        let names: Vec<String> = model
            .raw_children(ROOT)
            .iter()
            .map(|&node| model.name(node))
            .collect();
        assert_eq!(names, ["a", "b", "top.zip"]);
        assert_eq!(model.name(model.raw_children(1)[0]), "two.bin");
    }

    #[test]
    fn paths_join_with_backslashes() {
        let model = sample();
        assert_eq!(model.display_path(ROOT), "C:\\");
        assert_eq!(model.display_path(2), "C:\\a\\one.txt");
        assert_eq!(model.path_bytes(2), b"C:\\a\\one.txt");
    }

    #[test]
    fn categories_come_from_the_names() {
        let model = sample();
        assert_eq!(model.category(2), Category::Document);
        assert_eq!(model.category(5), Category::Image);
        assert_eq!(model.category(6), Category::Archive);
        assert_eq!(model.category(1), Category::Directory);
    }

    #[test]
    fn hard_links_keep_their_size_but_count_once() {
        let mut tree = TreeBuilder::new("C:\\", 4, 16);
        tree.add(ROOT, b"first", file(100));
        let link = tree.add(
            ROOT,
            b"second",
            NodeData {
                flags: FLAG_HARD_LINK,
                ..file(100)
            },
        );
        let model = tree.finish("<orphans>", summary());
        assert_eq!(model.allocated(ROOT), 100);
        assert_eq!(model.files(ROOT), 1);
        assert_eq!(model.allocated(link), 100);
        assert_eq!(model.weight(link), 0);
    }

    #[test]
    fn deleting_only_touches_the_ancestors() {
        let mut model = sample();
        model.mark_deleted(3);
        assert_eq!(model.allocated(ROOT), 160);
        assert_eq!(model.allocated(1), 100);
        assert_eq!(model.files(1), 1);
        assert_eq!(model.logical(ROOT), 157);
        assert!(!model.is_visible(3));
        assert_eq!(model.weight(3), 0);
        // Deleting twice, or the root, changes nothing.
        model.mark_deleted(3);
        model.mark_deleted(ROOT);
        assert_eq!(model.allocated(ROOT), 160);
    }

    #[test]
    fn deleting_a_folder_removes_its_whole_subtree() {
        let mut model = sample();
        model.mark_deleted(1);
        assert_eq!(model.allocated(ROOT), 60);
        assert_eq!(model.files(ROOT), 2);
    }

    #[test]
    fn a_parent_cycle_ends_up_under_the_orphans_and_stays_counted() {
        let mut tree = TreeBuilder::new("C:\\", 4, 16);
        tree.add(ROOT, b"kept", file(10));
        // Nodes 2 and 3 point at each other and never reach the root.
        tree.add(3, b"loop-a", dir());
        tree.add(2, b"loop-b", dir());
        tree.add(2, b"inside", file(90));
        let model = tree.finish("<orphans>", summary());
        assert_eq!(model.allocated(ROOT), 100);
        assert_eq!(model.files(ROOT), 2);
        let orphans = model
            .raw_children(ROOT)
            .iter()
            .copied()
            .find(|&node| model.flags(node) & FLAG_SYNTHETIC != 0)
            .unwrap();
        assert_eq!(model.name(orphans), "<orphans>");
    }

    #[test]
    fn empty_synthetic_nodes_stay_out_of_listings() {
        let mut tree = TreeBuilder::new("C:\\", 4, 16);
        let empty = tree.add(
            ROOT,
            b"<nothing>",
            NodeData {
                flags: FLAG_DIRECTORY | FLAG_SYNTHETIC,
                ..NodeData::default()
            },
        );
        let model = tree.finish("<orphans>", summary());
        assert!(!model.is_visible(empty));
    }

    #[test]
    fn metafile_marking_reaches_descendants() {
        let mut tree = TreeBuilder::new("C:\\", 4, 16);
        let extend = tree.add(
            ROOT,
            b"$Extend",
            NodeData {
                flags: FLAG_DIRECTORY | FLAG_METAFILE,
                ..NodeData::default()
            },
        );
        let journal = tree.add(extend, b"$UsnJrnl", file(100));
        let model = tree.finish("<orphans>", summary());
        assert_ne!(model.flags(journal) & FLAG_METAFILE, 0);
    }

    #[test]
    fn a_single_root_is_a_valid_tree() {
        let model = TreeBuilder::new("C:\\", 1, 4).finish("<orphans>", summary());
        assert_eq!(model.len(), 1);
        assert!(model.raw_children(ROOT).is_empty());
        assert_eq!(model.ancestry(ROOT), [ROOT]);
    }
}
