//! Mutation methods for [`TileCover`].

use super::TileCover;
use crate::{TileBBox, TileCoord};
use anyhow::Result;
use versatiles_derive::context;

impl TileCover {
	/// Inserts a single tile coordinate into this cover.
	///
	/// If the coordinate is already covered (Bbox contains it), this is a no-op.
	/// Otherwise the cover is upgraded to a `Tree` for exact representation and
	/// the coordinate is inserted.
	///
	/// # Errors
	/// Returns an error if the coordinate's level does not match this cover's level.
	#[context("Failed to include TileCoord {coord:?} into TileCover at level {}", self.level())]
	pub fn insert_coord(&mut self, coord: &TileCoord) -> Result<()> {
		if let TileCover::Bbox(b) = self {
			if b.is_empty() {
				return b.insert_coord(coord);
			}
			if b.includes_coord(coord) {
				return Ok(());
			}
			self.upgrade_to_tree();
		}
		self.as_tree_mut().insert_coord(coord)
	}

	/// Inserts all tiles in `bbox` into this cover.
	///
	/// If the bbox is already fully covered (Bbox contains it), this is a no-op.
	/// Otherwise the cover is upgraded to a `Tree` for exact representation.
	///
	/// # Errors
	/// Returns an error if `bbox`'s level does not match this cover's level.
	#[context("Failed to insert TileBBox {bbox:?} into TileCover at level {}", self.level())]
	pub fn insert_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		if let TileCover::Bbox(b) = self {
			if b.is_empty() {
				return b.insert_bbox(bbox);
			}
			if b.includes_bbox(bbox) {
				return Ok(());
			}
			self.upgrade_to_tree();
		}
		self.as_tree_mut().insert_bbox(bbox)
	}

	/// Removes a single tile coordinate from this cover.
	///
	/// If the coordinate is not covered (Bbox does not contain it), this is a no-op.
	/// Otherwise the cover is upgraded to a `Tree` for exact subtraction.
	///
	/// # Errors
	/// Returns an error if the coordinate's level does not match this cover's level.
	#[context("Failed to remove TileCoord {coord:?} from TileCover at level {}", self.level())]
	pub fn remove_coord(&mut self, coord: &TileCoord) -> Result<()> {
		if let TileCover::Bbox(b) = self {
			if b.is_empty() || !b.includes_coord(coord) {
				return Ok(());
			}
			self.upgrade_to_tree();
		}
		self.as_tree_mut().remove_coord(coord)
	}

	/// Removes all tiles in `bbox` from this cover.
	///
	/// If there is no overlap (Bbox does not intersect it), this is a no-op.
	/// Otherwise the cover is upgraded to a `Tree` for exact subtraction.
	///
	/// # Errors
	/// Returns an error if `bbox`'s level does not match this cover's level.
	#[context("Failed to remove TileBBox {bbox:?} from TileCover at level {}", self.level())]
	pub fn remove_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		if let TileCover::Bbox(b) = self {
			if b.is_empty() || !b.intersects_bbox(bbox) {
				return Ok(());
			}
			self.upgrade_to_tree();
		}
		self.as_tree_mut().remove_bbox(bbox)
	}

	/// Expands tile coverage outward by `size` tiles in all directions.
	///
	/// For `Bbox` covers this expands the rectangle (clamped to level bounds).
	/// For `Tree` covers this uses Full-node decomposition: each `Full` subtree
	/// rectangle is expanded independently, then the results are unioned.
	pub fn buffer(&mut self, size: u32) {
		match self {
			TileCover::Bbox(b) => b.buffer(size),
			TileCover::Tree(t) => t.buffer(size),
		}
	}

	/// Applies a Y-flip.
	pub fn flip_y(&mut self) {
		match self {
			TileCover::Bbox(b) => b.flip_y(),
			TileCover::Tree(t) => t.flip_y(),
		}
	}

	/// Swaps x and y coordinates: `(x, y) → (y, x)`.
	pub fn swap_xy(&mut self) {
		match self {
			TileCover::Bbox(b) => b.swap_xy(),
			TileCover::Tree(t) => t.swap_xy(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TileQuadtree;

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}
	fn coord(z: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(z, x, y).unwrap()
	}

	#[test]
	fn insert_tile_expands_bbox() {
		let mut c = TileCover::new_empty(4).unwrap();
		c.insert_coord(&coord(4, 3, 3)).unwrap();
		assert!(!c.is_empty());
		assert_eq!(c.count_tiles(), 1);
	}

	#[test]
	fn insert_bbox() {
		let mut c = TileCover::new_empty(4).unwrap();
		c.insert_bbox(&bbox(4, 2, 2, 5, 5)).unwrap();
		assert_eq!(c.count_tiles(), 16);
	}

	#[test]
	fn remove_tile_upgrades_to_tree() {
		let mut c = TileCover::from(bbox(3, 0, 0, 3, 3)); // 16 tiles
		assert!(matches!(c, TileCover::Bbox(_)));
		c.remove_coord(&coord(3, 0, 0)).unwrap();
		assert!(matches!(c, TileCover::Tree(_)));
		assert_eq!(c.count_tiles(), 15);
	}

	#[test]
	fn remove_bbox_upgrades_to_tree() {
		let mut c = TileCover::from(bbox(3, 0, 0, 7, 7)); // full z=3, 64 tiles
		c.remove_bbox(&bbox(3, 0, 0, 3, 3)).unwrap(); // remove 16 tiles
		assert!(matches!(c, TileCover::Tree(_)));
		assert_eq!(c.count_tiles(), 48);
	}

	#[test]
	fn include_coord_noop_when_already_covered() {
		let mut c = TileCover::from(bbox(4, 0, 0, 15, 15));
		// Already covered; stays Bbox and count unchanged.
		c.insert_coord(&coord(4, 5, 5)).unwrap();
		assert!(matches!(c, TileCover::Bbox(_)));
		assert_eq!(c.count_tiles(), 256);
	}

	#[test]
	fn insert_bbox_noop_when_already_covered() {
		let mut c = TileCover::from(bbox(4, 0, 0, 15, 15));
		c.insert_bbox(&bbox(4, 2, 2, 8, 8)).unwrap();
		assert!(matches!(c, TileCover::Bbox(_)));
		assert_eq!(c.count_tiles(), 256);
	}

	#[test]
	fn remove_coord_noop_when_not_in_bbox() {
		let mut c = TileCover::from(bbox(4, 5, 5, 10, 10));
		// coord outside bbox → no-op, stays Bbox
		c.remove_coord(&coord(4, 0, 0)).unwrap();
		assert!(matches!(c, TileCover::Bbox(_)));
	}

	#[test]
	fn remove_bbox_noop_when_no_overlap() {
		let mut c = TileCover::from(bbox(4, 5, 5, 10, 10));
		// non-overlapping bbox → no-op, stays Bbox
		c.remove_bbox(&bbox(4, 12, 12, 15, 15)).unwrap();
		assert!(matches!(c, TileCover::Bbox(_)));
	}

	// ── buffer: identity, empty no-op, expand, clamp to bounds ──────────────
	#[rstest::rstest]
	#[case(TileCover::new_empty(4).unwrap(), 5, 0)] // empty stays empty
	#[case(TileCover::from(bbox(4, 7, 7, 7, 7)), 0, 1)] // buffer(0) is no-op
	#[case(TileCover::from(bbox(4, 7, 7, 7, 7)), 1, 9)] // 3x3
	#[case(TileCover::from(bbox(4, 0, 0, 0, 0)), 2, 9)] // clamped at 0,0 → (0,0)..(2,2)
	#[case(TileCover::new_full(2).unwrap(), 3, 16)] // full stays full
	fn buffer_cases(#[case] mut c: TileCover, #[case] size: u32, #[case] expected: u64) {
		c.buffer(size);
		assert_eq!(c.count_tiles(), expected);
	}

	// ── flip_y involution ───────────────────────────────────────────────────
	#[rstest::rstest]
	#[case(TileCover::from(bbox(4, 1, 1, 3, 3)))]
	#[case(TileCover::from(bbox(3, 0, 0, 2, 4)))]
	#[case(TileCover::from(TileQuadtree::from_bbox(&bbox(3, 2, 5, 4, 7))))]
	#[case(TileCover::new_empty(3).unwrap())]
	#[case(TileCover::new_full(2).unwrap())]
	fn flip_y_is_involution(#[case] original: TileCover) {
		let mut c = original.clone();
		c.flip_y();
		c.flip_y();
		assert_eq!(c, original);
	}

	// ── swap_xy involution ──────────────────────────────────────────────────
	#[rstest::rstest]
	#[case(TileCover::from(bbox(4, 1, 3, 2, 5)))]
	#[case(TileCover::from(TileQuadtree::from_bbox(&bbox(3, 1, 2, 3, 4))))]
	#[case(TileCover::new_full(3).unwrap())]
	#[case(TileCover::new_empty(3).unwrap())]
	fn swap_xy_is_involution(#[case] original: TileCover) {
		let mut c = original.clone();
		c.swap_xy();
		c.swap_xy();
		assert_eq!(c, original);
	}

	// ── Bbox→Tree upgrade triggers ──────────────────────────────────────────
	#[rstest::rstest]
	// insert_coord outside → upgrades
	#[case::insert_outside(|c: &mut TileCover| c.insert_coord(&coord(3, 6, 6)).unwrap(), true)]
	// insert_bbox outside → upgrades
	#[case::insert_bbox_outside(|c: &mut TileCover| c.insert_bbox(&bbox(3, 5, 5, 7, 7)).unwrap(), true)]
	// insert_coord inside → stays Bbox
	#[case::insert_inside(|c: &mut TileCover| c.insert_coord(&coord(3, 1, 1)).unwrap(), false)]
	// remove of inside → upgrades
	#[case::remove_inside(|c: &mut TileCover| c.remove_coord(&coord(3, 1, 1)).unwrap(), true)]
	// remove outside → no-op
	#[case::remove_outside(|c: &mut TileCover| c.remove_coord(&coord(3, 7, 7)).unwrap(), false)]
	fn upgrade_to_tree_triggers(#[case] op: fn(&mut TileCover), #[case] upgrades: bool) {
		let mut c = TileCover::from(bbox(3, 0, 0, 3, 3));
		op(&mut c);
		assert_eq!(matches!(c, TileCover::Tree(_)), upgrades);
	}

	#[test]
	fn insert_into_empty_cover_stays_bbox() {
		let mut c = TileCover::new_empty(3).unwrap();
		c.insert_coord(&coord(3, 4, 4)).unwrap();
		assert!(matches!(c, TileCover::Bbox(_)));
		assert_eq!(c.count_tiles(), 1);

		// Inserting a *subset* of the current bbox is a no-op and stays Bbox.
		let mut c2 = TileCover::from(bbox(3, 1, 1, 5, 5));
		c2.insert_bbox(&bbox(3, 2, 2, 3, 3)).unwrap();
		assert!(matches!(c2, TileCover::Bbox(_)));
	}

	#[test]
	fn insert_disjoint_bbox_upgrades_to_tree() {
		let mut c = TileCover::from(bbox(3, 0, 0, 1, 1));
		// (5..7) is disjoint from (0..1); growing to cover both upgrades to Tree.
		c.insert_bbox(&bbox(3, 5, 5, 7, 7)).unwrap();
		assert!(matches!(c, TileCover::Tree(_)));
		assert_eq!(c.count_tiles(), 4 + 9);
	}
}
