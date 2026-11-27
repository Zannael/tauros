//! Walkable boot points (EMU-006).
//!
//! The game places the player wherever it is told, even inside a wall, so a boot
//! point is checked against the layout's blockdata first. Each block of `map.bin`
//! carries a collision value and an elevation (include/global.fieldmap.h):
//! - collision other than 0 is solid (`MapGridGetCollisionAt`);
//! - elevation 1 (`ELEVATION_SURF`) is water the player can only cross surfing, and
//!   every land tile next to it is an elevation mismatch, so a player placed there
//!   on foot cannot walk away.
//!
//! Object events are not considered: booting onto an NPC's tile works
//! (the EMU-005 point in front of the Petalburg gym is one), so they do not block a point.
//!
//! Elevation 0 (transition) and 15 (multi-level) match every elevation.

use crate::boot::BootError;
use crate::maps::{Block, Blockdata, Layout, Layouts, LAYOUTS_FILE};
use std::fs;
use std::path::Path;

pub const ELEVATION_TRANSITION: u8 = 0;
pub const ELEVATION_SURF: u8 = 1;
pub const ELEVATION_MULTI_LEVEL: u8 = 15;

/// Why a tile cannot be stood on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    OutsideMap,
    Solid,
    Water,
}

impl Blocked {
    pub fn describe(self) -> &'static str {
        match self {
            Blocked::OutsideMap => "outside the map",
            Blocked::Solid => "solid (collision)",
            Blocked::Water => "deep water (needs Surf)",
        }
    }
}

/// A map's collision and elevation grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Walkability {
    width: u16,
    height: u16,
    blocks: Vec<Block>,
}

/// The result of checking a point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PointCheck {
    Walkable,
    Blocked {
        reason: Blocked,
        /// The closest walkable tile, absent when the map has none.
        nearest: Option<(i16, i16)>,
    },
}

fn elevations_match(a: u8, b: u8) -> bool {
    a == b || [a, b].iter().any(|e| *e == ELEVATION_TRANSITION || *e == ELEVATION_MULTI_LEVEL)
}

impl Walkability {
    pub fn new(width: u16, height: u16, words: &[u16]) -> Result<Self, BootError> {
        if words.len() != width as usize * height as usize {
            return Err(BootError(format!("blockdata has {} blocks, the layout is {width}x{height}", words.len())));
        }
        Ok(Self { width, height, blocks: words.iter().map(|w| Block::unpack(*w)).collect() })
    }

    /// Reads the blockdata of `layout_id` from the project.
    pub fn read(project: &Path, layout_id: &str) -> Result<Self, BootError> {
        let text = fs::read_to_string(project.join(LAYOUTS_FILE)).map_err(|e| BootError(format!("{LAYOUTS_FILE}: {e}")))?;
        let layouts = Layouts::parse(&text).map_err(|e| BootError(e.0))?;
        let layout = layouts.get(layout_id).ok_or_else(|| BootError(format!("unknown layout {layout_id}")))?;
        Self::read_layout(project, layout)
    }

    pub fn read_layout(project: &Path, layout: &Layout) -> Result<Self, BootError> {
        let bytes = fs::read(project.join(&layout.blockdata_filepath))
            .map_err(|e| BootError(format!("{}: {e}", layout.blockdata_filepath)))?;
        let data = Blockdata::parse(&bytes).map_err(|e| BootError(format!("{}: {}", layout.blockdata_filepath, e.0)))?;
        Self::new(layout.width, layout.height, &data.0)
    }

    pub fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    fn block(&self, x: i32, y: i32) -> Option<Block> {
        if x < 0 || y < 0 || x >= i32::from(self.width) || y >= i32::from(self.height) {
            return None;
        }
        self.blocks.get(y as usize * self.width as usize + x as usize).copied()
    }

    /// Why the player cannot stand at (`x`, `y`) on foot, or `None` when they can.
    pub fn blocked(&self, x: i32, y: i32) -> Option<Blocked> {
        let Some(block) = self.block(x, y) else { return Some(Blocked::OutsideMap) };
        if block.collision != 0 {
            Some(Blocked::Solid)
        } else if block.elevation == ELEVATION_SURF {
            Some(Blocked::Water)
        } else {
            None
        }
    }

    pub fn is_walkable(&self, x: i32, y: i32) -> bool {
        self.blocked(x, y).is_none()
    }

    /// A walkable tile with a walkable neighbour of a matching elevation, so the
    /// player is not sealed into a one-tile pocket.
    fn has_exit(&self, x: i32, y: i32) -> bool {
        let Some(here) = self.block(x, y) else { return false };
        [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
            self.is_walkable(x + dx, y + dy)
                && self.block(x + dx, y + dy).is_some_and(|b| elevations_match(here.elevation, b.elevation))
        })
    }

    /// The walkable tile closest to (`x`, `y`) by distance, preferring tiles the
    /// player can leave; ties go to the smaller y, then the smaller x.
    pub fn nearest_walkable(&self, x: i32, y: i32) -> Option<(i16, i16)> {
        let mut best: Option<((bool, i64), (i16, i16))> = None;
        for ty in 0..i32::from(self.height) {
            for tx in 0..i32::from(self.width) {
                if !self.is_walkable(tx, ty) {
                    continue;
                }
                let distance = i64::from(tx - x).pow(2) + i64::from(ty - y).pow(2);
                let key = (!self.has_exit(tx, ty), distance);
                if best.is_none_or(|(k, _)| key < k) {
                    best = Some((key, (tx as i16, ty as i16)));
                }
            }
        }
        best.map(|(_, p)| p)
    }

    pub fn check(&self, x: i32, y: i32) -> PointCheck {
        match self.blocked(x, y) {
            None => PointCheck::Walkable,
            Some(reason) => PointCheck::Blocked { reason, nearest: self.nearest_walkable(x, y) },
        }
    }

    /// The walkable tile nearest the map's centre, the default boot point.
    pub fn default_point(&self) -> Option<(i16, i16)> {
        self.nearest_walkable(i32::from(self.width / 2), i32::from(self.height / 2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOOR: u16 = 3 << 12;
    const WALL: u16 = (1 << 10) | (3 << 12);
    const WATER: u16 = 1 << 12;

    fn grid(rows: &[&[u16]]) -> Walkability {
        let words: Vec<u16> = rows.iter().flat_map(|r| r.iter().copied()).collect();
        Walkability::new(rows[0].len() as u16, rows.len() as u16, &words).unwrap()
    }

    #[test]
    fn collision_water_and_the_map_edge_block_a_point() {
        let g = grid(&[&[FLOOR, WALL, WATER], &[FLOOR, FLOOR, 0]]);
        assert_eq!(g.blocked(0, 0), None);
        assert_eq!(g.blocked(1, 0), Some(Blocked::Solid));
        assert_eq!(g.blocked(2, 0), Some(Blocked::Water));
        assert_eq!(g.blocked(2, 1), None, "elevation 0 matches every elevation");
        assert_eq!(g.blocked(3, 0), Some(Blocked::OutsideMap));
        assert_eq!(g.blocked(-1, 0), Some(Blocked::OutsideMap));
    }

    #[test]
    fn a_blocked_point_resolves_to_the_nearest_walkable_tile() {
        let g = grid(&[&[WALL, WALL, WALL, WALL], &[WALL, WALL, FLOOR, FLOOR], &[WALL, WALL, WALL, WALL]]);
        assert_eq!(g.check(2, 1), PointCheck::Walkable);
        assert_eq!(g.check(0, 0), PointCheck::Blocked { reason: Blocked::Solid, nearest: Some((2, 1)) });
        assert_eq!(g.default_point(), Some((2, 1)));
    }

    #[test]
    fn a_sealed_pocket_is_the_last_resort() {
        // (0, 1) is nearer to (0, 0) but walled in; (3, 0) can be left.
        let g = grid(&[&[WALL, WALL, FLOOR, FLOOR], &[FLOOR, WALL, WALL, WALL]]);
        assert_eq!(g.nearest_walkable(0, 0), Some((2, 0)));
        let sealed = grid(&[&[WALL, FLOOR], &[WALL, WALL]]);
        assert_eq!(sealed.nearest_walkable(0, 0), Some((1, 0)), "still offered when it is the only tile");
    }

    #[test]
    fn a_map_without_walkable_tiles_has_no_suggestion() {
        let g = grid(&[&[WALL, WATER]]);
        assert_eq!(g.check(0, 0), PointCheck::Blocked { reason: Blocked::Solid, nearest: None });
        assert_eq!(g.default_point(), None);
    }

    #[test]
    fn blockdata_size_must_match_the_layout() {
        assert!(Walkability::new(2, 2, &[FLOOR; 3]).is_err());
    }
}
