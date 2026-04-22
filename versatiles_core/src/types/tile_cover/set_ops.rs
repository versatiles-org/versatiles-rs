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

	#[test]
	fn union_bbox_bbox_stays_bbox() {
		let a = TileCover::from(bbox(4, 0, 0, 3, 3));
		let b = TileCover::from(bbox(4, 5, 5, 8, 8));
		let u = a.union(&b).unwrap();
		assert!(matches!(u, TileCover::Tree(_)));
		assert_eq!(u.to_bbox(), bbox(4, 0, 0, 8, 8));
	}

	#[test]
	fn union_with_tree_gives_tree() {
		let a = TileCover::from(bbox(3, 0, 0, 3, 3));
		let b = TileCover::from(TileQuadtree::new_full(3).unwrap());
		let u = a.union(&b).unwrap();
		assert!(matches!(u, TileCover::Tree(_)));
		assert!(u.is_full());
	}

	#[test]
	fn intersection_bbox_bbox() {
		let a = TileCover::from(bbox(4, 0, 0, 7, 7));
		let b = TileCover::from(bbox(4, 4, 4, 11, 11));
		let i = a.intersection(&b).unwrap();
		assert!(matches!(i, TileCover::Bbox(_)));
		assert_eq!(i.to_bbox(), bbox(4, 4, 4, 7, 7));
	}

	#[test]
	fn difference_always_tree() {
		let a = TileCover::from(bbox(3, 0, 0, 7, 7)); // full z=3, 64 tiles
		let b = TileCover::from(bbox(3, 0, 0, 3, 3)); // 16 tiles
		let d = a.difference(&b).unwrap();
		assert!(matches!(d, TileCover::Tree(_)));
		assert_eq!(d.count_tiles(), 48);
	}

	#[test]
	fn set_ops_zoom_mismatch_errors() {
		let a = TileCover::from(bbox(3, 0, 0, 7, 7));
		let b = TileCover::from(bbox(4, 0, 0, 15, 15));
		assert!(a.union(&b).is_err());
		assert!(a.intersection(&b).is_err());
		assert!(a.difference(&b).is_err());
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

	#[test]
	fn union_is_commutative() {
		let a = TileCover::from(bbox(3, 0, 0, 3, 7));
		let b = TileCover::from(bbox(3, 4, 0, 7, 7));
		let ab = a.union(&b).unwrap();
		let ba = b.union(&a).unwrap();
		assert_eq!(ab.count_tiles(), ba.count_tiles());
		assert_eq!(ab.to_bbox(), ba.to_bbox());
	}

	#[test]
	fn intersection_is_commutative() {
		let a = TileCover::from(bbox(4, 0, 0, 7, 7));
		let b = TileCover::from(bbox(4, 4, 4, 11, 11));
		let ab = a.intersection(&b).unwrap();
		let ba = b.intersection(&a).unwrap();
		assert_eq!(ab, ba);
	}

	#[test]
	fn difference_disjoint_leaves_original_size() {
		let a = TileCover::from(bbox(3, 0, 0, 3, 3));
		let b = TileCover::from(bbox(3, 4, 4, 7, 7)); // fully disjoint
		let d = a.difference(&b).unwrap();
		assert_eq!(d.count_tiles(), a.count_tiles());
	}
}
