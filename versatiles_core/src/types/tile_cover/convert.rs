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
			TileCover::Bbox(_) => unreachable!("upgrade_to_tree leaves self as TileCover::Tree"),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}

	/// `to_bbox()` — empty cover yields empty bbox; populated yields the
	/// originating bbox.
	#[rstest]
	#[case::empty(TileCover::new_empty(3).unwrap(), TileBBox::new_empty(3).unwrap())]
	#[case::bbox_variant(TileCover::from(bbox(3, 1, 2, 3, 4)), bbox(3, 1, 2, 3, 4))]
	fn to_bbox_cases(#[case] c: TileCover, #[case] expected: TileBBox) {
		assert_eq!(c.to_bbox(), expected);
	}

	/// `as_bbox` / `as_tree` are mutually exclusive per variant.
	#[rstest]
	#[case::bbox_variant(TileCover::from(bbox(2, 0, 0, 1, 1)), true, false)]
	#[case::tree_variant(TileCover::from(TileQuadtree::new_empty(2).unwrap()), false, true)]
	#[case::full_tree(TileCover::from(TileQuadtree::new_full(2).unwrap()), false, true)]
	fn as_bbox_and_as_tree_are_mutually_exclusive(#[case] c: TileCover, #[case] is_bbox: bool, #[case] is_tree: bool) {
		assert_eq!(c.as_bbox().is_some(), is_bbox);
		assert_eq!(c.as_tree().is_some(), is_tree);
	}

	/// `to_geo_bbox()` — None for empty, Some otherwise.
	#[rstest]
	#[case::empty(TileCover::new_empty(4).unwrap(), false)]
	#[case::populated(TileCover::from(bbox(4, 0, 0, 15, 15)), true)]
	fn to_geo_bbox_some_when_populated(#[case] c: TileCover, #[case] expect_some: bool) {
		assert_eq!(c.to_geo_bbox().is_some(), expect_some);
	}

	/// `at_level` preserves the requested level for both variants and across
	/// up/down/identity/extreme transitions.
	#[rstest]
	#[case::up(0, 5)]
	#[case::down(5, 0)]
	#[case::identity(5, 5)]
	#[case::to_max(5, 30)]
	fn at_level_changes_level_on_both_variants(#[case] from: u8, #[case] to: u8) {
		let c_bbox = TileCover::from(TileBBox::new_full(from).unwrap());
		let c_tree = TileCover::from(TileQuadtree::new_full(from).unwrap());
		assert_eq!(c_bbox.at_level(to).level(), to);
		assert_eq!(c_tree.at_level(to).level(), to);
	}

	/// `to_bbox/tree` (by ref) and `into_bbox/tree` (consuming) yield
	/// equivalent outputs across both variants.
	#[rstest]
	#[case::from_bbox(TileCover::from(bbox(3, 1, 1, 4, 4)), 16)]
	#[case::from_tree(TileCover::from(TileQuadtree::from_bbox(&bbox(3, 0, 0, 3, 3))), 16)]
	fn to_and_into_equivalents_match(#[case] c: TileCover, #[case] expected_tiles: u64) {
		let by_ref_bbox = c.to_bbox();
		let by_ref_tree = c.to_tree();
		assert_eq!(by_ref_tree.count_tiles(), expected_tiles);

		// Consuming variants must match their by-ref siblings.
		assert_eq!(c.clone().into_bbox(), by_ref_bbox);
		assert_eq!(c.into_tree().count_tiles(), expected_tiles);
	}
}
