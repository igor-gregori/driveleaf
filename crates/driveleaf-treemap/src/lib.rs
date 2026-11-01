//! Squarified treemap layout, after Bruls, Huizing and van Wijk (2000).
//!
//! The crate knows nothing about the GUI. It turns weights into rectangles and
//! serializes them in the binary tile format the frontend draws.

#![forbid(unsafe_code)]

mod synthetic;
mod tree;

pub use synthetic::synthetic_tiles;
pub use tree::{LayoutOptions, STYLE_FOLDER, TreeSource, layout_tree};

/// An axis-aligned rectangle in layout space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn area(&self) -> f64 {
        self.w * self.h
    }
}

/// One tile of the wire format: `x, y, w, h: f32`, `color, node_id: u32`, little-endian.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: u32,
    pub node_id: u32,
}

impl Tile {
    /// Size of one serialized tile.
    pub const BYTES: usize = 24;

    pub fn from_rect(rect: Rect, color: u32, node_id: u32) -> Self {
        Self {
            x: rect.x as f32,
            y: rect.y as f32,
            w: rect.w as f32,
            h: rect.h as f32,
            color,
            node_id,
        }
    }

    pub fn write_le(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.x.to_le_bytes());
        out.extend_from_slice(&self.y.to_le_bytes());
        out.extend_from_slice(&self.w.to_le_bytes());
        out.extend_from_slice(&self.h.to_le_bytes());
        out.extend_from_slice(&self.color.to_le_bytes());
        out.extend_from_slice(&self.node_id.to_le_bytes());
    }
}

/// Lays `weights` out inside `area`, calling `emit(index, rect)` for each one.
///
/// `weights` must be sorted in descending order. Non-positive weights get no
/// rectangle.
pub fn squarify(weights: &[f64], area: Rect, mut emit: impl FnMut(usize, Rect)) {
    let count = weights.partition_point(|&w| w > 0.0);
    let weights = &weights[..count];
    let total: f64 = weights.iter().sum();
    if total <= 0.0 || area.w <= 0.0 || area.h <= 0.0 {
        return;
    }
    let scale = area.area() / total;
    let mut free = area;
    let mut start = 0;
    while start < weights.len() {
        let side = free.w.min(free.h);
        let (end, row_area) = fill_row(weights, start, scale, side);
        let thickness = row_area / side;
        if free.w >= free.h {
            let mut y = free.y;
            for (offset, &weight) in weights[start..end].iter().enumerate() {
                let h = weight * scale / thickness;
                emit(
                    start + offset,
                    Rect {
                        x: free.x,
                        y,
                        w: thickness,
                        h,
                    },
                );
                y += h;
            }
            free.x += thickness;
            free.w = (free.w - thickness).max(0.0);
        } else {
            let mut x = free.x;
            for (offset, &weight) in weights[start..end].iter().enumerate() {
                let w = weight * scale / thickness;
                emit(
                    start + offset,
                    Rect {
                        x,
                        y: free.y,
                        w,
                        h: thickness,
                    },
                );
                x += w;
            }
            free.y += thickness;
            free.h = (free.h - thickness).max(0.0);
        }
        start = end;
    }
}

/// Extends a row from `start` while its worst aspect ratio keeps improving.
fn fill_row(weights: &[f64], start: usize, scale: f64, side: f64) -> (usize, f64) {
    let mut end = start;
    let (mut sum, mut min, mut max) = (0.0_f64, f64::MAX, 0.0_f64);
    let mut best = f64::MAX;
    while end < weights.len() {
        let area = weights[end] * scale;
        let (next_sum, next_min, next_max) = (sum + area, min.min(area), max.max(area));
        let worst = worst_ratio(next_sum, next_min, next_max, side);
        if end > start && worst > best {
            break;
        }
        (sum, min, max, best) = (next_sum, next_min, next_max, worst);
        end += 1;
    }
    (end, sum)
}

fn worst_ratio(sum: f64, min: f64, max: f64, side: f64) -> f64 {
    let (sum2, side2) = (sum * sum, side * side);
    (side2 * max / sum2).max(sum2 / (side2 * min))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(weights: &[f64], area: Rect) -> Vec<(usize, Rect)> {
        let mut out = Vec::new();
        squarify(weights, area, |index, rect| out.push((index, rect)));
        out
    }

    const AREA: Rect = Rect {
        x: 10.0,
        y: 20.0,
        w: 600.0,
        h: 400.0,
    };

    #[test]
    fn matches_the_paper_example_row_breaks() {
        // The worked example from the paper: a 6x4 rectangle.
        let weights = [6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0];
        let tiles = layout(
            &weights,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 6.0,
                h: 4.0,
            },
        );
        assert_eq!(tiles.len(), 7);
        // First row: the two 6s stacked on the left, each 3 wide by 2 tall.
        assert!((tiles[0].1.w - 3.0).abs() < 1e-9 && (tiles[0].1.h - 2.0).abs() < 1e-9);
        assert!((tiles[1].1.w - 3.0).abs() < 1e-9 && (tiles[1].1.y - 2.0).abs() < 1e-9);
    }

    #[test]
    fn areas_stay_proportional_and_cover_the_area() {
        let weights = [50.0, 25.0, 10.0, 5.0, 5.0, 3.0, 2.0];
        let total: f64 = weights.iter().sum();
        let tiles = layout(&weights, AREA);
        assert_eq!(tiles.len(), weights.len());
        let mut covered = 0.0;
        for (index, rect) in &tiles {
            let expected = AREA.area() * weights[*index] / total;
            assert!((rect.area() - expected).abs() < 1e-6);
            assert!(rect.x >= AREA.x - 1e-9 && rect.y >= AREA.y - 1e-9);
            assert!(rect.x + rect.w <= AREA.x + AREA.w + 1e-9);
            assert!(rect.y + rect.h <= AREA.y + AREA.h + 1e-9);
            covered += rect.area();
        }
        assert!((covered - AREA.area()).abs() < 1e-6);
    }

    #[test]
    fn empty_and_degenerate_inputs_emit_nothing() {
        assert!(layout(&[], AREA).is_empty());
        assert!(layout(&[0.0, 0.0], AREA).is_empty());
        let flat = Rect { h: 0.0, ..AREA };
        assert!(layout(&[1.0], flat).is_empty());
    }

    #[test]
    fn tile_serializes_to_the_documented_layout() {
        let tile = Tile {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
            color: 0x1122_3344,
            node_id: 7,
        };
        let mut bytes = Vec::new();
        tile.write_le(&mut bytes);
        assert_eq!(bytes.len(), Tile::BYTES);
        assert_eq!(&bytes[16..20], &0x1122_3344_u32.to_le_bytes());
        assert_eq!(&bytes[20..24], &7_u32.to_le_bytes());
    }
}
