//! Nested layout of a whole tree, largest folders first, within a tile budget.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::{Rect, Tile, squarify};

/// Marks a tile as a folder, drawn behind its contents.
pub const STYLE_FOLDER: u32 = 1 << 16;
/// Marks a folder whose contents were laid out on top of it.
pub const STYLE_OPENED: u32 = 1 << 17;

/// What the layout needs to know about a tree.
pub trait TreeSource: Sync {
    /// Children of a node, sorted by descending weight.
    fn children(&self, node: u32) -> &[u32];
    /// Share of the area. Zero means the node gets no tile.
    fn weight(&self, node: u32) -> u64;
    fn is_directory(&self, node: u32) -> bool;
    /// Style index for the renderer.
    fn category(&self, node: u32) -> u8;
}

/// Tuning knobs for [`layout_tree`].
#[derive(Clone, Copy, Debug)]
pub struct LayoutOptions {
    /// The most tiles to emit.
    pub max_tiles: usize,
    /// Gap between a folder's edge and its contents.
    pub padding: f64,
    /// Folders narrower than this are drawn flat, without their contents.
    pub min_folder_side: f64,
    /// Children to lay out per folder. The rest becomes part of the folder tile.
    pub max_children: usize,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            max_tiles: 200_000,
            padding: 1.0,
            min_folder_side: 6.0,
            max_children: 16_384,
        }
    }
}

/// A folder waiting for its contents to be laid out.
struct Pending {
    area: f64,
    rect: Rect,
    node: u32,
    depth: u8,
}

impl PartialEq for Pending {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Pending {}

impl PartialOrd for Pending {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pending {
    fn cmp(&self, other: &Self) -> Ordering {
        self.area
            .total_cmp(&other.area)
            .then_with(|| other.node.cmp(&self.node))
    }
}

fn style(category: u8, depth: u8) -> u32 {
    u32::from(category) | (u32::from(depth) << 8)
}

fn inset(rect: Rect, by: f64) -> Rect {
    Rect {
        x: rect.x + by,
        y: rect.y + by,
        w: rect.w - 2.0 * by,
        h: rect.h - 2.0 * by,
    }
}

/// Lays out `root` and everything below it inside a `width` by `height` canvas.
///
/// Folders open largest first, so when the tile budget runs out it is the
/// smallest detail that goes missing. A parent always comes before its children,
/// so scanning the tiles backwards finds the deepest one under a point. Anything
/// smaller than a pixel is dropped and shows as part of its folder.
pub fn layout_tree<T: TreeSource + ?Sized>(
    tree: &T,
    root: u32,
    width: f32,
    height: f32,
    options: &LayoutOptions,
) -> Vec<Tile> {
    let canvas = Rect {
        x: 0.0,
        y: 0.0,
        w: f64::from(width),
        h: f64::from(height),
    };
    let mut tiles = Vec::with_capacity(options.max_tiles.min(1 << 16));
    let mut pending = BinaryHeap::new();
    pending.push(Pending {
        area: canvas.area(),
        rect: canvas,
        node: root,
        depth: 0,
    });
    let mut shown = Vec::new();
    let mut weights = Vec::new();
    let mut rects = Vec::new();

    while let Some(folder) = pending.pop() {
        let folder_tile = tiles.len();
        tiles.push(Tile::from_rect(
            folder.rect,
            style(tree.category(folder.node), folder.depth) | STYLE_FOLDER,
            folder.node,
        ));
        let inner = inset(folder.rect, options.padding);
        let budget = options
            .max_tiles
            .saturating_sub(tiles.len() + pending.len())
            .min(options.max_children);
        if budget == 0 || inner.w < options.min_folder_side || inner.h < options.min_folder_side {
            continue;
        }

        shown.clear();
        weights.clear();
        for &child in tree.children(folder.node) {
            let weight = tree.weight(child);
            if weight > 0 {
                shown.push(child);
                weights.push(weight as f64);
                if shown.len() == budget {
                    break;
                }
            }
        }
        let laid_out: f64 = weights.iter().sum();
        let whole = tree.weight(folder.node) as f64;
        if whole > laid_out {
            // What is left over stays blank, so the folder's own tile shows there.
            weights.push(whole - laid_out);
        }

        rects.clear();
        squarify(&weights, inner, |index, rect| {
            if index < shown.len() && rect.w >= 1.0 && rect.h >= 1.0 {
                rects.push((shown[index], rect));
            }
        });
        if !rects.is_empty() {
            tiles[folder_tile].color |= STYLE_OPENED;
        }
        for &(child, rect) in &rects {
            let depth = folder.depth.saturating_add(1);
            let opens = tree.is_directory(child) && !tree.children(child).is_empty();
            if opens {
                pending.push(Pending {
                    area: rect.area(),
                    rect,
                    node: child,
                    depth,
                });
            } else {
                let mut word = style(tree.category(child), depth);
                if tree.is_directory(child) {
                    word |= STYLE_FOLDER;
                }
                tiles.push(Tile::from_rect(rect, word, child));
            }
        }
    }
    tiles
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree given as `(parent, weight, category)` rows, with node 0 as root.
    struct Table {
        children: Vec<Vec<u32>>,
        weights: Vec<u64>,
        categories: Vec<u8>,
        directories: Vec<bool>,
    }

    impl Table {
        fn new(rows: &[(u32, u64, u8)]) -> Self {
            let mut children = vec![Vec::new(); rows.len()];
            let mut weights: Vec<u64> = rows.iter().map(|row| row.1).collect();
            for (node, row) in rows.iter().enumerate().skip(1).rev() {
                children[row.0 as usize].push(node as u32);
                weights[row.0 as usize] += weights[node];
            }
            for list in &mut children {
                list.sort_by_key(|&node| std::cmp::Reverse(weights[node as usize]));
            }
            let directories = children.iter().map(|list| !list.is_empty()).collect();
            Self {
                children,
                weights,
                categories: rows.iter().map(|row| row.2).collect(),
                directories,
            }
        }
    }

    impl TreeSource for Table {
        fn children(&self, node: u32) -> &[u32] {
            &self.children[node as usize]
        }

        fn weight(&self, node: u32) -> u64 {
            self.weights[node as usize]
        }

        fn is_directory(&self, node: u32) -> bool {
            self.directories[node as usize]
        }

        fn category(&self, node: u32) -> u8 {
            self.categories[node as usize]
        }
    }

    fn wide_tree(folders: u32, files: u32) -> Table {
        let mut rows = vec![(0, 0, 0)];
        for folder in 0..folders {
            let id = rows.len() as u32;
            rows.push((0, 0, 0));
            for file in 0..files {
                rows.push((id, 1 + u64::from(folder + file) % 7, 3));
            }
        }
        Table::new(&rows)
    }

    fn inside(child: &Tile, parent: &Tile) -> bool {
        const SLACK: f32 = 0.01;
        child.x >= parent.x - SLACK
            && child.y >= parent.y - SLACK
            && child.x + child.w <= parent.x + parent.w + SLACK
            && child.y + child.h <= parent.y + parent.h + SLACK
    }

    #[test]
    fn folders_come_before_and_around_their_contents() {
        let tree = wide_tree(4, 4);
        let tiles = layout_tree(&tree, 0, 800.0, 600.0, &LayoutOptions::default());
        assert_eq!(tiles.len(), 1 + 4 + 16);
        assert_eq!(tiles[0].node_id, 0);
        assert_ne!(tiles[0].color & STYLE_FOLDER, 0);
        for (position, tile) in tiles.iter().enumerate() {
            let parent = tree
                .children
                .iter()
                .position(|list| list.contains(&tile.node_id));
            if let Some(parent) = parent {
                let found = tiles[..position]
                    .iter()
                    .find(|other| other.node_id == parent as u32)
                    .expect("the parent comes first");
                assert!(inside(tile, found));
            }
        }
    }

    #[test]
    fn files_carry_their_category_and_depth() {
        let tree = wide_tree(1, 2);
        let tiles = layout_tree(&tree, 0, 400.0, 300.0, &LayoutOptions::default());
        let file = tiles.iter().find(|tile| tile.node_id == 2).unwrap();
        assert_eq!(file.color & 0xFF, 3);
        assert_eq!((file.color >> 8) & 0xFF, 2);
        assert_eq!(file.color & STYLE_FOLDER, 0);
        assert_eq!(file.color & STYLE_OPENED, 0);
        assert_ne!(tiles[0].color & STYLE_OPENED, 0);
    }

    #[test]
    fn the_budget_is_respected_and_big_folders_open_first() {
        let mut rows = vec![(0, 0, 0)];
        let big = rows.len() as u32;
        rows.push((0, 0, 0));
        for _ in 0..50 {
            rows.push((big, 1000, 3));
        }
        let small = rows.len() as u32;
        rows.push((0, 0, 0));
        for _ in 0..50 {
            rows.push((small, 1, 3));
        }
        let tree = Table::new(&rows);
        let options = LayoutOptions {
            max_tiles: 40,
            ..LayoutOptions::default()
        };
        let tiles = layout_tree(&tree, 0, 1000.0, 800.0, &options);
        assert!(tiles.len() <= 40);
        let opened = |low: u32, high: u32| {
            tiles
                .iter()
                .any(|tile| tile.node_id > low && tile.node_id < high)
        };
        assert!(opened(big, small));
        assert!(!opened(small, u32::MAX));
    }

    #[test]
    fn specks_are_dropped_and_their_area_stays_with_the_folder() {
        let mut rows = vec![(0, 1_000_000_000, 3)];
        rows.push((0, 1_000_000_000, 3));
        for _ in 0..1000 {
            rows.push((0, 1, 3));
        }
        let tree = Table::new(&rows);
        let tiles = layout_tree(&tree, 0, 100.0, 100.0, &LayoutOptions::default());
        assert!(tiles.len() < 20, "{} tiles", tiles.len());
        for tile in &tiles {
            assert!(tile.w >= 1.0 && tile.h >= 1.0);
        }
    }

    #[test]
    fn a_folder_holding_more_than_its_children_leaves_room() {
        struct Heavy;
        impl TreeSource for Heavy {
            fn children(&self, node: u32) -> &[u32] {
                if node == 0 { &[1] } else { &[] }
            }
            fn weight(&self, node: u32) -> u64 {
                if node == 0 { 100 } else { 25 }
            }
            fn is_directory(&self, node: u32) -> bool {
                node == 0
            }
            fn category(&self, _node: u32) -> u8 {
                0
            }
        }
        let tiles = layout_tree(&Heavy, 0, 100.0, 100.0, &LayoutOptions::default());
        let child = tiles.iter().find(|tile| tile.node_id == 1).unwrap();
        let share = f64::from(child.w * child.h) / (98.0 * 98.0);
        assert!((share - 0.25).abs() < 0.01, "share {share}");
    }

    #[test]
    fn an_empty_tree_is_just_its_root() {
        let tree = Table::new(&[(0, 0, 0)]);
        let tiles = layout_tree(&tree, 0, 100.0, 100.0, &LayoutOptions::default());
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].color & STYLE_OPENED, 0);
    }
}
