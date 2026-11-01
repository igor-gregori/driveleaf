//! Read-only questions about a [`Model`], asked page by page.

use std::collections::HashMap;

use rayon::prelude::*;

use crate::categories::{Category, categorize, extension};
use crate::model::{FLAG_DIRECTORY, FLAG_HARD_LINK, FLAG_SYNTHETIC, Model, NodeId};

/// What to order a listing by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Allocated,
    Logical,
    Name,
    Files,
    Modified,
}

/// One page of a longer listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub nodes: Vec<NodeId>,
    /// Length of the whole listing.
    pub total: usize,
}

fn page_of(nodes: Vec<NodeId>, offset: usize, limit: usize) -> Page {
    let total = nodes.len();
    Page {
        nodes: nodes.into_iter().skip(offset).take(limit).collect(),
        total,
    }
}

/// One line of the file types view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeRow {
    /// Lowercase, without the dot. Empty for files without an extension.
    pub extension: String,
    pub category: Category,
    pub files: u64,
    pub logical: u64,
    pub allocated: u64,
}

fn lowercase_name(name: &[u8]) -> Vec<u8> {
    if name.is_ascii() {
        name.to_ascii_lowercase()
    } else {
        String::from_utf8_lossy(name).to_lowercase().into_bytes()
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

impl Model {
    /// Visible children of a folder, in the requested order.
    pub fn list_children(
        &self,
        node: NodeId,
        key: SortKey,
        descending: bool,
        offset: usize,
        limit: usize,
    ) -> Page {
        let mut nodes: Vec<NodeId> = self
            .raw_children(node)
            .iter()
            .copied()
            .filter(|&child| self.is_visible(child))
            .collect();
        match key {
            // Children are stored sorted by size already.
            SortKey::Allocated => {}
            SortKey::Logical => nodes.sort_by_key(|&child| std::cmp::Reverse(self.logical(child))),
            SortKey::Files => nodes.sort_by_key(|&child| std::cmp::Reverse(self.files(child))),
            SortKey::Modified => {
                nodes.sort_by_key(|&child| std::cmp::Reverse(self.modified(child)))
            }
            SortKey::Name => {
                nodes.sort_by_cached_key(|&child| {
                    std::cmp::Reverse(lowercase_name(self.name_bytes(child)))
                });
            }
        }
        if !descending {
            nodes.reverse();
        }
        page_of(nodes, offset, limit)
    }

    /// Files in the whole tree, largest first. Hard-linked names beyond the
    /// first are left out, since they hold no space of their own.
    pub fn top_files(&self, offset: usize, limit: usize) -> Page {
        let mut nodes: Vec<NodeId> = (0..self.len() as NodeId)
            .into_par_iter()
            .filter(|&node| {
                self.flags(node) & (FLAG_DIRECTORY | FLAG_HARD_LINK | FLAG_SYNTHETIC) == 0
                    && self.is_visible(node)
            })
            .collect();
        nodes.par_sort_unstable_by_key(|&node| (std::cmp::Reverse(self.weight(node)), node));
        page_of(nodes, offset, limit)
    }

    /// Space per file extension, largest first.
    pub fn file_types(&self) -> Vec<TypeRow> {
        type Tally = HashMap<Vec<u8>, (u64, u64, u64)>;
        let tally: Tally = (0..self.len() as NodeId)
            .into_par_iter()
            .filter(|&node| {
                self.flags(node) & (FLAG_DIRECTORY | FLAG_HARD_LINK | FLAG_SYNTHETIC) == 0
                    && self.is_visible(node)
            })
            .fold(Tally::new, |mut tally, node| {
                let key = extension(self.name_bytes(node))
                    .map_or_else(Vec::new, |(bytes, length)| bytes[..length].to_vec());
                let entry = tally.entry(key).or_default();
                entry.0 += 1;
                entry.1 = entry.1.saturating_add(self.logical(node));
                entry.2 = entry.2.saturating_add(self.allocated(node));
                tally
            })
            .reduce(Tally::new, |mut merged, tally| {
                for (key, (files, logical, allocated)) in tally {
                    let entry = merged.entry(key).or_default();
                    entry.0 += files;
                    entry.1 = entry.1.saturating_add(logical);
                    entry.2 = entry.2.saturating_add(allocated);
                }
                merged
            });
        let mut rows: Vec<TypeRow> = tally
            .into_iter()
            .map(|(key, (files, logical, allocated))| {
                let mut name = b"x.".to_vec();
                name.extend_from_slice(&key);
                TypeRow {
                    category: categorize(&name),
                    extension: String::from_utf8_lossy(&key).into_owned(),
                    files,
                    logical,
                    allocated,
                }
            })
            .collect();
        rows.sort_unstable_by(|a, b| {
            b.allocated
                .cmp(&a.allocated)
                .then_with(|| a.extension.cmp(&b.extension))
        });
        rows
    }

    /// Nodes whose name contains `query`, ignoring case, largest first.
    pub fn search(&self, query: &str, limit: usize) -> Page {
        let needle = query.to_lowercase().into_bytes();
        if needle.is_empty() {
            return Page {
                nodes: Vec::new(),
                total: 0,
            };
        }
        let mut nodes: Vec<NodeId> = (1..self.len() as NodeId)
            .into_par_iter()
            .filter(|&node| {
                self.is_visible(node) && contains(&lowercase_name(self.name_bytes(node)), &needle)
            })
            .collect();
        nodes.par_sort_unstable_by_key(|&node| (std::cmp::Reverse(self.weight(node)), node));
        page_of(nodes, 0, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FLAG_HARD_LINK, NodeData, ROOT, ScanMode, TreeBuilder, VolumeSummary};

    fn dir() -> NodeData {
        NodeData {
            flags: FLAG_DIRECTORY,
            ..NodeData::default()
        }
    }

    fn file(allocated: u64, modified: u64) -> NodeData {
        NodeData {
            logical: 1000 - allocated,
            allocated,
            modified,
            ..NodeData::default()
        }
    }

    fn sample() -> Model {
        let mut tree = TreeBuilder::new("C:\\", 8, 64);
        let photos = tree.add(ROOT, b"Fotos", dir());
        tree.add(photos, b"Praia.JPG", file(300, 3));
        tree.add(photos, b"casa.jpg", file(100, 1));
        tree.add(ROOT, b"notes.txt", file(50, 9));
        tree.add(ROOT, b"Relat\xC3\x93RIO.pdf", file(500, 2));
        tree.add(ROOT, b"README", file(10, 5));
        tree.add(
            ROOT,
            b"casa-link.jpg",
            NodeData {
                flags: FLAG_HARD_LINK,
                ..file(100, 1)
            },
        );
        tree.finish(
            "<orphans>",
            VolumeSummary {
                mode: ScanMode::Walk,
                used_bytes: None,
                total_bytes: None,
                problems: 0,
            },
        )
    }

    fn names(model: &Model, page: &Page) -> Vec<String> {
        page.nodes.iter().map(|&node| model.name(node)).collect()
    }

    #[test]
    fn listings_sort_and_page() {
        let model = sample();
        let by_size = model.list_children(ROOT, SortKey::Allocated, true, 0, 2);
        assert_eq!(names(&model, &by_size), ["RelatÓRIO.pdf", "Fotos"]);
        assert_eq!(by_size.total, 5);

        let next = model.list_children(ROOT, SortKey::Allocated, true, 2, 2);
        assert_eq!(names(&model, &next), ["notes.txt", "README"]);

        let by_name = model.list_children(ROOT, SortKey::Name, false, 0, 10);
        assert_eq!(
            names(&model, &by_name),
            [
                "casa-link.jpg",
                "Fotos",
                "notes.txt",
                "README",
                "RelatÓRIO.pdf"
            ]
        );

        let newest = model.list_children(ROOT, SortKey::Modified, true, 0, 1);
        assert_eq!(names(&model, &newest), ["notes.txt"]);

        let fewest = model.list_children(ROOT, SortKey::Files, false, 0, 1);
        assert_eq!(model.files(fewest.nodes[0]), 0);
    }

    #[test]
    fn deleted_nodes_leave_the_listings() {
        let mut model = sample();
        let target = model.search("notes", 1).nodes[0];
        model.mark_deleted(target);
        let page = model.list_children(ROOT, SortKey::Allocated, true, 0, 10);
        assert!(!names(&model, &page).contains(&"notes.txt".to_owned()));
        assert_eq!(page.total, 4);
    }

    #[test]
    fn top_files_cross_folders_and_skip_links() {
        let model = sample();
        let page = model.top_files(0, 3);
        assert_eq!(
            names(&model, &page),
            ["RelatÓRIO.pdf", "Praia.JPG", "casa.jpg"]
        );
        assert_eq!(page.total, 5);
    }

    #[test]
    fn file_types_group_by_extension() {
        let rows = sample().file_types();
        let jpg = rows.iter().find(|row| row.extension == "jpg").unwrap();
        assert_eq!(jpg.files, 2);
        assert_eq!(jpg.allocated, 400);
        assert_eq!(jpg.category, Category::Image);
        assert_eq!(rows[0].extension, "pdf");
        let none = rows.iter().find(|row| row.extension.is_empty()).unwrap();
        assert_eq!(none.files, 1);
        assert_eq!(none.category, Category::Other);
    }

    #[test]
    fn search_ignores_case_and_ranks_by_size() {
        let model = sample();
        let page = model.search("JPG", 10);
        assert_eq!(
            names(&model, &page),
            ["Praia.JPG", "casa.jpg", "casa-link.jpg"]
        );
        assert_eq!(
            names(&model, &model.search("relatório", 10)),
            ["RelatÓRIO.pdf"]
        );
        assert_eq!(model.search("", 10).total, 0);
        assert_eq!(model.search("absent", 10).total, 0);
        assert_eq!(model.search("jpg", 1).total, 3);
    }
}
