use crate::{TileBBox, TileCoord, TileCover, TilePyramid, TileQuadtree};

impl TilePyramid {
	/// Returns `true` if `coord` is covered at its zoom level in this pyramid.
	#[must_use]
	pub fn includes_coord(&self, coord: &TileCoord) -> bool {
		self.level_ref(coord.level).includes_coord(coord)
	}

	/// Returns `true` if every tile in `bbox` is covered at `bbox`'s zoom level.
	///
	/// An empty `bbox` is vacuously included (returns `true`).
	#[must_use]
	pub fn includes_bbox(&self, bbox: &TileBBox) -> bool {
		self.level_ref(bbox.level()).includes_bbox(bbox)
	}

	/// Returns `true` if every tile in `tree` is covered at `tree`'s zoom level.
	#[must_use]
	pub fn includes_tree(&self, tree: &TileQuadtree) -> bool {
		self.level_ref(tree.level()).includes_tree(tree)
	}

	/// Returns `true` if every tile in `cover` is covered at `cover`'s zoom level.
	#[must_use]
	pub fn includes_cover(&self, cover: &TileCover) -> bool {
		self.level_ref(cover.level()).includes_cover(cover)
	}

	/// Returns `true` if for every zoom level, every tile in `pyramid` is also
	/// in `self`.
	///
	/// An empty `pyramid` is vacuously included (returns `true`).
	#[must_use]
	pub fn includes_pyramid(&self, pyramid: &TilePyramid) -> bool {
		self.iter().zip(pyramid.iter()).all(|(a, b)| a.includes_cover(b))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}
	fn coord(z: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(z, x, y).unwrap()
	}

	#[test]
	fn includes_coord() {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(5, 3, 4, 10, 15)).unwrap();
		assert!(p.includes_coord(&coord(5, 5, 7)));
		assert!(!p.includes_coord(&coord(5, 0, 0)));
		assert!(!p.includes_coord(&coord(6, 5, 7)));
	}

	#[test]
	fn includes_bbox() {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(5, 0, 0, 15, 15)).unwrap();
		assert!(p.includes_bbox(&bbox(5, 2, 2, 8, 8)));
		assert!(!p.includes_bbox(&bbox(5, 0, 0, 20, 20)));
	}

	#[test]
	fn includes_pyramid() {
		let mut a = TilePyramid::new_empty();
		a.insert_bbox(&bbox(5, 0, 0, 15, 15)).unwrap();

		let mut b = TilePyramid::new_empty();
		b.insert_bbox(&bbox(5, 2, 2, 8, 8)).unwrap();

		assert!(a.includes_pyramid(&b));
		assert!(!b.includes_pyramid(&a));
	}

	#[test]
	fn includes_empty_pyramid() {
		let p = TilePyramid::new_full_up_to(5);
		// Every pyramid includes an empty pyramid.
		assert!(p.includes_pyramid(&TilePyramid::new_empty()));
	}
}
