use crate::{TileBBox, TileCover, TilePyramid, TileQuadtree};

impl TilePyramid {
	/// Returns `true` if the level matching `bbox`'s zoom shares at least one
	/// tile with `bbox`.
	#[must_use]
	pub fn intersects_bbox(&self, bbox: &TileBBox) -> bool {
		self.level_ref(bbox.level()).intersects_bbox(bbox)
	}

	/// Returns `true` if the level matching `tree`'s zoom shares at least one
	/// tile with `tree`.
	#[must_use]
	pub fn intersects_tree(&self, tree: &TileQuadtree) -> bool {
		self.level_ref(tree.level()).intersects_tree(tree)
	}

	/// Returns `true` if the level matching `cover`'s zoom shares at least one
	/// tile with `cover`.
	#[must_use]
	pub fn intersects_cover(&self, cover: &TileCover) -> bool {
		self.level_ref(cover.level()).intersects_cover(cover)
	}

	/// Returns `true` if any zoom level of `self` shares at least one tile with
	/// the same level of `pyramid`.
	#[must_use]
	pub fn intersects_pyramid(&self, pyramid: &TilePyramid) -> bool {
		self.iter().zip(pyramid.iter()).any(|(a, b)| a.intersects_cover(b))
	}

	/// Shrinks each level of `self` in place to the tiles also present in the
	/// corresponding level of `pyramid`.
	pub fn intersect_pyramid(&mut self, pyramid: &TilePyramid) {
		self
			.levels
			.iter_mut()
			.zip(pyramid.levels.iter())
			.for_each(|(a, b)| a.intersect_cover(b).expect("same-level operation"));
	}

	/// Returns a new pyramid where each level contains only the tiles shared by
	/// `self` and the corresponding level of `pyramid`.
	#[must_use]
	pub fn intersection_pyramid(&self, pyramid: &TilePyramid) -> Self {
		let mut result = self.clone();
		result.intersect_pyramid(pyramid);
		result
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{GeoBBox, TileCoord};

	fn bbox(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(level, x0, y0, x1, y1).unwrap()
	}
	fn coord(z: u8, x: u32, y: u32) -> TileCoord {
		TileCoord::new(z, x, y).unwrap()
	}

	#[test]
	fn intersects_bbox() {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(4, 0, 0, 7, 7)).unwrap();
		assert!(p.intersects_bbox(&bbox(4, 5, 5, 10, 10)));
		assert!(!p.intersects_bbox(&bbox(4, 10, 10, 15, 15)));
	}

	#[test]
	fn intersects_pyramid() {
		let mut a = TilePyramid::new_empty();
		a.insert_bbox(&bbox(5, 0, 0, 15, 15)).unwrap();

		let mut b = TilePyramid::new_empty();
		b.insert_bbox(&bbox(5, 2, 2, 8, 8)).unwrap();
		assert!(a.intersects_pyramid(&b));

		let mut c = TilePyramid::new_empty();
		c.insert_bbox(&bbox(5, 20, 20, 25, 25)).unwrap();
		assert!(!a.intersects_pyramid(&c));
	}

	#[test]
	fn intersect_pyramid() {
		let mut a = TilePyramid::new_empty();
		a.insert_bbox(&bbox(5, 0, 0, 15, 15)).unwrap();

		let mut b = TilePyramid::new_empty();
		b.insert_bbox(&bbox(5, 10, 10, 25, 25)).unwrap();

		a.intersect_pyramid(&b);
		assert!(a.includes_coord(&coord(5, 12, 12)));
		assert!(!a.includes_coord(&coord(5, 2, 2)));
	}

	#[test]
	fn intersect_geo_bbox() {
		let mut p = TilePyramid::new_full();
		let geo = GeoBBox::new(10.0, 50.0, 15.0, 55.0).unwrap();
		p.intersect_geo_bbox(&geo).unwrap();
		assert!(!p.is_empty());
		assert_eq!(p.level_ref(10).count_tiles(), 375);
	}

	#[test]
	fn intersected_bbox() {
		let mut p = TilePyramid::new_empty();
		p.insert_bbox(&bbox(4, 0, 0, 7, 7)).unwrap();
		let result = p.level_ref(4).intersection_bbox(&bbox(4, 4, 4, 11, 11)).unwrap();
		assert_eq!(result.to_bbox(), bbox(4, 4, 4, 7, 7));
	}
}
