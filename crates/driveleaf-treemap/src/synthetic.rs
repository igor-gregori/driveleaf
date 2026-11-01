//! Deterministic synthetic tiles for benchmarks.
//!
//! Builds a three-level tree shaped like a disk, with folders of uneven size and
//! files in a handful of categories, and lays it out with the real algorithm.

use crate::{Rect, Tile, squarify};

const TOP_DIRS: usize = 36;
const MID_DIRS: usize = 36;
const CATEGORY_COLORS: [u32; 8] = [
    0x4C_8D_F6_FF,
    0x34_C7_8A_FF,
    0xF2_B1_34_FF,
    0xE8_6A_5C_FF,
    0xA7_7B_F3_FF,
    0x3C_C4_D6_FF,
    0xF0_8A_C0_FF,
    0x8F_9B_AE_FF,
];

/// Small xorshift generator, so the benchmark needs no dependency.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    fn unit(&mut self) -> f64 {
        f64::from(self.next()) / f64::from(u32::MAX)
    }

    /// A mildly long-tailed positive weight.
    fn weight(&mut self) -> f64 {
        let u = self.unit();
        1.0 + 15.0 * u * u * u * u
    }
}

/// Returns exactly `count` tiles filling a `width` by `height` canvas.
pub fn synthetic_tiles(count: u32, width: f32, height: f32, seed: u32) -> Vec<Tile> {
    let mut rng = Rng(seed.max(1));
    let area = Rect {
        x: 0.0,
        y: 0.0,
        w: f64::from(width),
        h: f64::from(height),
    };
    let mut tiles = Vec::with_capacity(count as usize);
    let mut remaining = count as usize;
    let tops = sorted_weights(&mut rng, TOP_DIRS);
    let mut top_rects = Vec::with_capacity(tops.len());
    squarify(&tops, area, |_, rect| top_rects.push(rect));
    for (top_index, top_rect) in top_rects.iter().enumerate() {
        let mids = sorted_weights(&mut rng, MID_DIRS);
        let mut mid_rects = Vec::with_capacity(mids.len());
        squarify(&mids, *top_rect, |_, rect| mid_rects.push(rect));
        for (mid_index, mid_rect) in mid_rects.iter().enumerate() {
            let dirs_left = (TOP_DIRS - top_index - 1) * MID_DIRS + (MID_DIRS - mid_index);
            let files = remaining.div_ceil(dirs_left);
            remaining -= files;
            let weights = sorted_weights(&mut rng, files);
            squarify(&weights, *mid_rect, |_, rect| {
                let color = CATEGORY_COLORS[rng.next() as usize % CATEGORY_COLORS.len()];
                let node_id = tiles.len() as u32;
                tiles.push(Tile::from_rect(rect, color, node_id));
            });
        }
    }
    tiles
}

fn sorted_weights(rng: &mut Rng, count: usize) -> Vec<f64> {
    let mut weights: Vec<f64> = (0..count).map(|_| rng.weight()).collect();
    weights.sort_unstable_by(|a, b| b.total_cmp(a));
    weights
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yields_exactly_the_requested_count_inside_the_canvas() {
        for count in [1, 37, 5_000, 200_000] {
            let tiles = synthetic_tiles(count, 1600.0, 900.0, 42);
            assert_eq!(tiles.len(), count as usize);
            for tile in &tiles {
                assert!(tile.x >= -0.01 && tile.y >= -0.01);
                assert!(tile.x + tile.w <= 1600.01 && tile.y + tile.h <= 900.01);
            }
        }
    }

    #[test]
    fn is_deterministic_for_a_seed() {
        assert_eq!(
            synthetic_tiles(1_000, 800.0, 600.0, 7),
            synthetic_tiles(1_000, 800.0, 600.0, 7)
        );
    }
}
