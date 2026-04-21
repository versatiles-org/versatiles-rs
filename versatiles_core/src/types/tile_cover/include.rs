use crate::{TileBBox, TileCoord, TileCover, TileQuadtree};

impl TileCover {
	/// Returns `true` if `coord` is covered by `self`.
	///
	/// # Panics
	/// Panics if `coord` is at a different zoom level than `self`.
	#[must_use]
	pub fn includes_coord(&self, coord: &TileCoord) -> bool {
		match self {
			TileCover::Bbox(b) => b.includes_coord(coord),
			TileCover::Tree(t) => t.includes_coord(coord),
		}
	}

	/// Returns `true` if every tile in `bbox` is also covered by `self`.
	///
	/// An empty `bbox` is vacuously included (returns `true`).
	///
	/// # Panics
	/// Panics if `bbox` is at a different zoom level than `self`.
	#[must_use]
	pub fn includes_bbox(&self, bbox: &TileBBox) -> bool {
		match self {
			TileCover::Bbox(b) => b.includes_bbox(bbox),
			TileCover::Tree(t) => t.includes_bbox(bbox),
		}
	}

	/// Returns `true` if every tile in `tree` is also covered by `self`.
	///
	/// # Panics
	/// Panics if `tree` is at a different zoom level than `self`.
	#[must_use]
	pub fn includes_tree(&self, tree: &TileQuadtree) -> bool {
		match self {
			TileCover::Bbox(b) => b.includes_tree(tree),
			TileCover::Tree(t) => t.includes_tree(tree),
		}
	}

	/// Returns `true` if every tile in `cover` is also covered by `self`.
	///
	/// # Panics
	/// Panics if `cover` is at a different zoom level than `self`.
	#[must_use]
	pub fn includes_cover(&self, cover: &TileCover) -> bool {
		match (self, cover) {
			(TileCover::Bbox(b1), TileCover::Bbox(b2)) => b1.includes_bbox(b2),
			(TileCover::Bbox(b), TileCover::Tree(t)) => b.includes_tree(t),
			(TileCover::Tree(t), TileCover::Bbox(b)) => t.includes_bbox(b),
			(TileCover::Tree(t1), TileCover::Tree(t2)) => t1.includes_tree(t2),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}
	fn coord(z: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(z, x, y).unwrap()
	}

	#[test]
	fn contains_tile() {
		let c = TileCover::from(bbox(5, 3, 4, 10, 15));
		assert!(c.includes_coord(&coord(5, 5, 7)));
		assert!(!c.includes_coord(&coord(5, 0, 0)));
	}

	#[test]
	fn contains_bbox() {
		let c = TileCover::from(bbox(5, 0, 0, 15, 15));
		assert!(c.includes_bbox(&bbox(5, 2, 2, 8, 8)));
		assert!(!c.includes_bbox(&bbox(5, 0, 0, 16, 16)));
	}

	#[test]
	#[should_panic(expected = "assertion `left == right` failed")]
	fn includes_coord_level_mismatch_tree_panics() {
		let c = TileCover::from(TileQuadtree::new_full(4).unwrap());
		let _ = c.includes_coord(&coord(5, 0, 0));
	}

	#[test]
	#[should_panic(expected = "Cannot compare TileBBox with level=")]
	fn includes_coord_level_mismatch_bbox_panics() {
		let c = TileCover::from(bbox(4, 0, 0, 15, 15));
		let _ = c.includes_coord(&coord(5, 0, 0));
	}

	#[test]
	#[should_panic(expected = "assertion `left == right` failed")]
	fn includes_bbox_level_mismatch_tree_panics() {
		let c = TileCover::from(TileQuadtree::new_full(4).unwrap());
		let _ = c.includes_bbox(&bbox(5, 0, 0, 15, 15));
	}

	#[test]
	#[should_panic(expected = "Cannot compare TileBBox with level=")]
	fn includes_bbox_level_mismatch_bbox_panics() {
		let c = TileCover::from(bbox(4, 0, 0, 15, 15));
		let _ = c.includes_bbox(&bbox(5, 0, 0, 15, 15));
	}
}
