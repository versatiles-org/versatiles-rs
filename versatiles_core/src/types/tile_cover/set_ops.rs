//! Set-algebra operations for [`TileCover`].

use super::TileCover;
use anyhow::Result;
use versatiles_derive::context;

impl TileCover {
	/// Returns the union of this cover and `other`.
	///
	/// - `Bbox` ∪ `Bbox` → `Bbox` (bounding rectangle of both; may over-approximate).
	/// - Any case involving a `Tree` → `Tree` (exact).
	///
	/// # Errors
	/// Returns an error if the zoom levels differ or a quadtree operation fails.
	#[context("Failed to union TileCovers at levels {} and {}", self.level(), other.level())]
	pub fn union(&self, other: &TileCover) -> Result<TileCover> {
		let a = self.to_tree();
		let b = other.to_tree();
		Ok(TileCover::Tree(a.union(&b)?))
	}

	/// Returns the intersection of this cover and `other`.
	///
	/// - `Bbox` ∩ `Bbox` → `Bbox` (rectangle intersection; exact).
	/// - Any case involving a `Tree` → `Tree` (exact).
	///
	/// # Errors
	/// Returns an error if the zoom levels differ or a quadtree operation fails.
	#[context("Failed to intersect TileCovers at levels {} and {}", self.level(), other.level())]
	pub fn intersection(&self, other: &TileCover) -> Result<TileCover> {
		if let (TileCover::Bbox(a), TileCover::Bbox(b)) = (self, other) {
			let mut result = *a;
			result.intersect_bbox(b)?;
			return Ok(TileCover::Bbox(result));
		}
		let a = self.to_tree();
		let b = other.to_tree();
		Ok(TileCover::Tree(a.intersection(&b)?))
	}

	/// Returns the set difference `self \ other`.
	///
	/// Always produces a `Tree` (exact subtraction is not generally expressible
	/// as a rectangle).
	///
	/// # Errors
	/// Returns an error if the zoom levels differ or a quadtree operation fails.
	#[context("Failed to compute difference of TileCovers at levels {} and {}", self.level(), other.level())]
	pub fn difference(&self, other: &TileCover) -> Result<TileCover> {
		let a = self.to_tree();
		let b = other.to_tree();
		Ok(TileCover::Tree(a.difference(&b)?))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{TileBBox, TileQuadtree};

	fn bbox(zoom: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> TileBBox {
		TileBBox::from_min_and_max(zoom, x0, y0, x1, y1).unwrap()
	}

	/// Which binary set operation to invoke.
	#[derive(Debug, Clone, Copy)]
	enum SetOp {
		Union,
		Intersection,
		Difference,
	}

	impl SetOp {
		fn apply(self, a: &TileCover, b: &TileCover) -> anyhow::Result<TileCover> {
			match self {
				SetOp::Union => a.union(b),
				SetOp::Intersection => a.intersection(b),
				SetOp::Difference => a.difference(b),
			}
		}
	}

	/// Expected storage kind (Bbox vs Tree) for a result + its tile count or
	/// bbox. Covers the core per-variant compaction rules in one table.
	#[rstest::rstest]
	#[case::union_bbox_bbox_becomes_tree(
		SetOp::Union,
		TileCover::from(bbox(4, 0, 0, 3, 3)),
		TileCover::from(bbox(4, 5, 5, 8, 8)),
		/* is_tree */ true,
		/* tile_count */ 32,
	)]
	#[case::union_with_full_tree_is_full(
		SetOp::Union,
		TileCover::from(bbox(3, 0, 0, 3, 3)),
		TileCover::from(TileQuadtree::new_full(3).unwrap()),
		true,
		64,
	)]
	#[case::intersection_bbox_bbox_stays_bbox(
		SetOp::Intersection,
		TileCover::from(bbox(4, 0, 0, 7, 7)),
		TileCover::from(bbox(4, 4, 4, 11, 11)),
		false,
		16
	)]
	#[case::difference_becomes_tree(
		SetOp::Difference,
		TileCover::from(bbox(3, 0, 0, 7, 7)), // 64 tiles
		TileCover::from(bbox(3, 0, 0, 3, 3)), // 16 tiles
		true,
		48,
	)]
	fn set_op_storage_and_count(
		#[case] op: SetOp,
		#[case] a: TileCover,
		#[case] b: TileCover,
		#[case] result_is_tree: bool,
		#[case] expected_count: u64,
	) {
		let out = op.apply(&a, &b).unwrap();
		assert_eq!(matches!(out, TileCover::Tree(_)), result_is_tree);
		assert_eq!(out.count_tiles(), expected_count);
	}

	#[rstest::rstest]
	#[case(SetOp::Union)]
	#[case(SetOp::Intersection)]
	#[case(SetOp::Difference)]
	fn zoom_mismatch_errors(#[case] op: SetOp) {
		let a = TileCover::from(bbox(3, 0, 0, 7, 7));
		let b = TileCover::from(bbox(4, 0, 0, 15, 15));
		assert!(op.apply(&a, &b).is_err());
	}

	// ── Algebraic identities: A ∪ A = A, A ∩ A = A, A \ A = ∅ ────────────────
	#[rstest::rstest]
	#[case(TileCover::new_empty(3).unwrap())]
	#[case(TileCover::new_full(3).unwrap())]
	#[case(TileCover::from(bbox(3, 1, 1, 5, 5)))]
	#[case(TileCover::from(TileQuadtree::from_bbox(&bbox(3, 1, 1, 5, 5))))]
	fn set_ops_with_self_identities(#[case] a: TileCover) {
		let u = a.union(&a).unwrap();
		let i = a.intersection(&a).unwrap();
		let d = a.difference(&a).unwrap();
		assert_eq!(u.count_tiles(), a.count_tiles(), "A ∪ A has same tile count");
		assert_eq!(i.count_tiles(), a.count_tiles(), "A ∩ A has same tile count");
		assert_eq!(d.count_tiles(), 0, "A \\ A is empty");
		assert!(d.is_empty());
	}

	// ── Identities with empty and full ──────────────────────────────────────
	#[rstest::rstest]
	#[case(TileCover::from(bbox(3, 1, 1, 5, 5)))]
	#[case(TileCover::from(TileQuadtree::from_bbox(&bbox(3, 1, 1, 5, 5))))]
	fn set_ops_vs_empty_and_full(#[case] a: TileCover) {
		let empty = TileCover::new_empty(3).unwrap();
		let full = TileCover::new_full(3).unwrap();
		// A ∪ ∅ = A, A ∩ full = A, A \ ∅ = A
		assert_eq!(a.union(&empty).unwrap().count_tiles(), a.count_tiles());
		assert_eq!(a.intersection(&full).unwrap().count_tiles(), a.count_tiles());
		assert_eq!(a.difference(&empty).unwrap().count_tiles(), a.count_tiles());
		// A ∩ ∅ = ∅, A \ full = ∅
		assert!(a.intersection(&empty).unwrap().is_empty());
		assert!(a.difference(&full).unwrap().is_empty());
		// A ∪ full = full (at level where full has 2^(2z) tiles)
		assert!(a.union(&full).unwrap().is_full());
	}

	/// Union and intersection are commutative.
	#[rstest::rstest]
	#[case::union(
		SetOp::Union,
		TileCover::from(bbox(3, 0, 0, 3, 7)),
		TileCover::from(bbox(3, 4, 0, 7, 7))
	)]
	#[case::intersection(
		SetOp::Intersection,
		TileCover::from(bbox(4, 0, 0, 7, 7)),
		TileCover::from(bbox(4, 4, 4, 11, 11))
	)]
	fn commutativity(#[case] op: SetOp, #[case] a: TileCover, #[case] b: TileCover) {
		let ab = op.apply(&a, &b).unwrap();
		let ba = op.apply(&b, &a).unwrap();
		assert_eq!(ab.count_tiles(), ba.count_tiles());
		assert_eq!(ab.to_bbox(), ba.to_bbox());
	}

	#[test]
	fn difference_disjoint_leaves_original_size() {
		let a = TileCover::from(bbox(3, 0, 0, 3, 3));
		let b = TileCover::from(bbox(3, 4, 4, 7, 7)); // fully disjoint
		assert_eq!(a.difference(&b).unwrap().count_tiles(), a.count_tiles());
	}
}
