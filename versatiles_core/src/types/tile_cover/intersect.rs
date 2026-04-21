use anyhow::Result;

use crate::{TileBBox, TileCover, TilePyramid, TileQuadtree};

impl TileCover {
	/// Returns `true` if `self` and `bbox` share at least one tile.
	#[must_use]
	pub fn intersects_bbox(&self, bbox: &TileBBox) -> bool {
		match self {
			TileCover::Bbox(b) => b.intersects_bbox(bbox),
			TileCover::Tree(t) => t.intersects_bbox(bbox),
		}
	}

	/// Returns `true` if `self` and `tree` share at least one tile.
	#[must_use]
	pub fn intersects_tree(&self, tree: &TileQuadtree) -> bool {
		match self {
			TileCover::Bbox(b) => b.intersects_tree(tree),
			TileCover::Tree(t) => t.intersects_tree(tree),
		}
	}

	/// Returns `true` if `self` and `cover` share at least one tile.
	#[must_use]
	pub fn intersects_cover(&self, cover: &TileCover) -> bool {
		match (self, cover) {
			(TileCover::Bbox(b1), TileCover::Bbox(b2)) => b1.intersects_bbox(b2),
			(TileCover::Bbox(b), TileCover::Tree(t)) | (TileCover::Tree(t), TileCover::Bbox(b)) => t.intersects_bbox(b),
			(TileCover::Tree(t1), TileCover::Tree(t2)) => t1.intersects_tree(t2),
		}
	}

	/// Returns `true` if `self` shares at least one tile with the corresponding
	/// level of `pyramid`.
	#[must_use]
	pub fn intersects_pyramid(&self, pyramid: &TilePyramid) -> bool {
		self.intersects_cover(pyramid.level_ref(self.level()))
	}

	/// Shrinks `self` in place to the tiles also present in `bbox`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersect_bbox(&mut self, bbox: &TileBBox) -> Result<()> {
		match self {
			TileCover::Bbox(b) => b.intersect_bbox(bbox),
			TileCover::Tree(t) => t.intersect_bbox(bbox),
		}
	}

	/// Shrinks `self` in place to the tiles also present in `tree`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersect_tree(&mut self, tree: &TileQuadtree) -> Result<()> {
		match self {
			TileCover::Bbox(b) => b.intersect_tree(tree),
			TileCover::Tree(t) => t.intersect_tree(tree),
		}
	}

	/// Shrinks `self` in place to the tiles also present in `cover`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersect_cover(&mut self, cover: &TileCover) -> Result<()> {
		match (self, cover) {
			(TileCover::Bbox(b1), TileCover::Bbox(b2)) => b1.intersect_bbox(b2),
			(TileCover::Bbox(b), TileCover::Tree(t)) => b.intersect_tree(t),
			(TileCover::Tree(t), TileCover::Bbox(b)) => t.intersect_bbox(b),
			(TileCover::Tree(t1), TileCover::Tree(t2)) => t1.intersect_tree(t2),
		}
	}

	/// Shrinks `self` in place to the tiles also present in the corresponding
	/// level of `pyramid`.
	pub fn intersect_pyramid(&mut self, pyramid: &TilePyramid) {
		self.intersect_cover(pyramid.level_ref(self.level())).unwrap();
	}

	/// Returns a new cover containing only the tiles shared by `self` and `bbox`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersection_bbox(&self, bbox: &TileBBox) -> Result<Self> {
		Ok(match self {
			TileCover::Bbox(b) => TileCover::from(b.intersection_bbox(bbox)?),
			TileCover::Tree(t) => TileCover::from(t.intersection_bbox(bbox)?),
		})
	}

	/// Returns a new cover containing only the tiles shared by `self` and `tree`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersection_tree(&self, tree: &TileQuadtree) -> Result<Self> {
		Ok(match self {
			TileCover::Bbox(b) => TileCover::from(b.intersection_tree(tree)?),
			TileCover::Tree(t) => TileCover::from(t.intersection_tree(tree)?),
		})
	}

	/// Returns a new cover containing only the tiles shared by `self` and `cover`.
	///
	/// # Errors
	/// Returns an error if the zoom levels differ.
	pub fn intersection_cover(&self, cover: &TileCover) -> Result<Self> {
		Ok(match (self, cover) {
			(TileCover::Bbox(b1), TileCover::Bbox(b2)) => TileCover::from(b1.intersection_bbox(b2)?),
			(TileCover::Bbox(b), TileCover::Tree(t)) | (TileCover::Tree(t), TileCover::Bbox(b)) => {
				TileCover::from(t.intersection_bbox(b)?)
			}
			(TileCover::Tree(t1), TileCover::Tree(t2)) => TileCover::from(t1.intersection_tree(t2)?),
		})
	}

	/// Returns a new cover containing only the tiles shared by `self` and the
	/// corresponding level of `pyramid`.
	#[must_use]
	pub fn intersection_pyramid(&self, pyramid: &TilePyramid) -> Self {
		self.intersection_cover(pyramid.level_ref(self.level())).unwrap()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn intersects_bbox() {
		let c = TileCover::from(bbox(4, 0, 0, 7, 7));
		assert!(c.intersects_bbox(&bbox(4, 5, 5, 10, 10)));
		assert!(!c.intersects_bbox(&bbox(4, 10, 10, 15, 15)));
	}

	#[test]
	fn intersects_bbox_tree_variant() {
		let c = TileCover::from(TileQuadtree::from_bbox(&bbox(4, 0, 0, 7, 7)));
		assert!(c.intersects_bbox(&bbox(4, 5, 5, 10, 10)));
		assert!(!c.intersects_bbox(&bbox(4, 10, 10, 15, 15)));
	}
}
