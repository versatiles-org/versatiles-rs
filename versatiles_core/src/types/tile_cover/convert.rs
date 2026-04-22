//! Conversion methods for [`TileCover`].

use super::TileCover;
use crate::{GeoBBox, TileBBox, TileQuadtree};

impl TileCover {
	/// Returns a reference to the inner [`TileBBox`], or `None` if this is the
	/// `Tree` variant.
	#[must_use]
	pub fn as_bbox(&self) -> Option<&TileBBox> {
		match self {
			TileCover::Bbox(b) => Some(b),
			TileCover::Tree(_) => None,
		}
	}

	/// Returns the axis-aligned bounding box of all covered tiles.
	///
	/// For the `Bbox` variant this is a clone; for `Tree` it is the tight
	/// enclosing bbox computed by the quadtree.
	#[must_use]
	pub fn to_bbox(&self) -> TileBBox {
		match self {
			TileCover::Bbox(b) => *b,
			TileCover::Tree(t) => t.to_bbox(),
		}
	}

	/// Consumes `self` and returns the axis-aligned bounding box of all covered
	/// tiles, avoiding a clone when the `Bbox` variant is already owned.
	#[must_use]
	pub fn into_bbox(self) -> TileBBox {
		match self {
			TileCover::Bbox(b) => b,
			TileCover::Tree(t) => t.to_bbox(),
		}
	}

	/// Converts the covered area to a geographic [`GeoBBox`], or `None` if empty.
	#[must_use]
	pub fn to_geo_bbox(&self) -> Option<GeoBBox> {
		match self {
			TileCover::Bbox(b) => b.to_geo_bbox(),
			TileCover::Tree(t) => t.to_geo_bbox(),
		}
	}

	/// Returns a reference to the inner [`TileQuadtree`], or `None` if this is
	/// the `Bbox` variant.
	#[must_use]
	pub fn as_tree(&self) -> Option<&TileQuadtree> {
		match self {
			TileCover::Bbox(_) => None,
			TileCover::Tree(t) => Some(t),
		}
	}

	/// Converts this cover to a [`TileQuadtree`].
	///
	/// Clones the tree if already a `Tree`; builds one from the bbox otherwise.
	///
	/// # Errors
	/// Returns an error if quadtree construction from the bbox fails.
	#[must_use]
	pub fn to_tree(&self) -> TileQuadtree {
		match self {
			TileCover::Bbox(b) => TileQuadtree::from_bbox(b),
			TileCover::Tree(t) => t.clone(),
		}
	}

	/// Consumes `self` and returns the inner [`TileQuadtree`], building one from
	/// the bbox if necessary without an extra clone.
	#[must_use]
	pub fn into_tree(self) -> TileQuadtree {
		match self {
			TileCover::Bbox(b) => TileQuadtree::from_bbox(&b),
			TileCover::Tree(t) => t,
		}
	}

	/// Returns a copy of this cover scaled to the given zoom `level`.
	#[must_use]
	pub fn at_level(&self, level: u8) -> TileCover {
		match self {
			TileCover::Bbox(b) => TileCover::Bbox(b.at_level(level)),
			TileCover::Tree(t) => TileCover::Tree(t.at_level(level)),
		}
	}

	/// Upgrades this cover from `Bbox` to `Tree` in place.
	///
	/// No-op if already a `Tree`. Used internally before exact-subtraction
	/// operations.
	pub(super) fn upgrade_to_tree(&mut self) {
		if let TileCover::Bbox(b) = self {
			*self = TileCover::Tree(TileQuadtree::from_bbox(b));
		}
	}

	/// Returns a mutable reference to the inner [`TileQuadtree`], upgrading from
	/// `Bbox` if necessary. Used internally to avoid repeated `upgrade_to_tree` +
	/// nested-match patterns.
	pub(super) fn as_tree_mut(&mut self) -> &mut TileQuadtree {
		self.upgrade_to_tree();
		match self {
			TileCover::Tree(t) => t,
			TileCover::Bbox(_) => unreachable!(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}

	#[test]
	fn bounds_empty_and_nonempty() {
		assert!(TileCover::new_empty(3).unwrap().to_bbox().is_empty());
		let c = TileCover::from(bbox(3, 1, 2, 3, 4));
		assert_eq!(c.to_bbox(), bbox(3, 1, 2, 3, 4));
	}

	#[test]
	fn at_level() {
		let c = TileCover::from(bbox(5, 4, 4, 8, 8));
		let c2 = c.at_level(6);
		assert_eq!(c2.level(), 6);
	}

	#[test]
	fn as_bbox_and_as_tree() {
		let cb = TileCover::from(bbox(2, 0, 0, 1, 1));
		assert!(cb.as_bbox().is_some());
		assert!(cb.as_tree().is_none());

		let ct = TileCover::from(TileQuadtree::new_empty(2).unwrap());
		assert!(ct.as_bbox().is_none());
		assert!(ct.as_tree().is_some());
	}

	#[test]
	fn to_tree_from_bbox() {
		let c = TileCover::from(bbox(3, 1, 1, 4, 4));
		let tree = c.to_tree();
		assert_eq!(tree.count_tiles(), 16);
	}

	#[test]
	fn to_geo_bbox_empty_is_none() {
		assert!(TileCover::new_empty(4).unwrap().to_geo_bbox().is_none());
	}

	#[test]
	fn to_geo_bbox_nonempty() {
		let c = TileCover::from(bbox(4, 0, 0, 15, 15));
		assert!(c.to_geo_bbox().is_some());
	}

	#[rstest::rstest]
	#[case(0, 5)] // up
	#[case(5, 0)] // down
	#[case(5, 5)] // identity
	#[case(5, 30)] // to max
	fn at_level_changes_level_on_both_variants(#[case] from: u8, #[case] to: u8) {
		let c_bbox = TileCover::from(TileBBox::new_full(from).unwrap());
		let c_tree = TileCover::from(TileQuadtree::new_full(from).unwrap());
		assert_eq!(c_bbox.at_level(to).level(), to);
		assert_eq!(c_tree.at_level(to).level(), to);
	}

	#[test]
	fn into_bbox_and_into_tree_match_non_consuming() {
		let c1 = TileCover::from(bbox(3, 1, 1, 4, 4));
		assert_eq!(c1.to_bbox(), c1.clone().into_bbox());
		let t1 = c1.to_tree();
		let t2 = c1.into_tree();
		assert_eq!(t1.count_tiles(), t2.count_tiles());
	}

	#[test]
	fn to_tree_from_tree_clones() {
		let tree = TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3));
		let c = TileCover::from(tree.clone());
		assert_eq!(c.to_tree().count_tiles(), tree.count_tiles());
		assert_eq!(c.into_tree().count_tiles(), tree.count_tiles());
	}

	#[test]
	fn as_bbox_as_tree_none_when_wrong_variant() {
		let c_bbox = TileCover::from(bbox(2, 0, 0, 1, 1));
		let c_tree = TileCover::from(TileQuadtree::new_full(2).unwrap());
		assert!(c_bbox.as_tree().is_none());
		assert!(c_tree.as_bbox().is_none());
	}
}
