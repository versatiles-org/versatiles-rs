//! Set-algebra operations for [`TileCover`].

use super::TileCover;
use anyhow::Result;

impl TileCover {
	/// Returns the union of this cover and `other`.
	///
	/// - `Bbox` ∪ `Bbox` → `Bbox` (bounding rectangle of both; may over-approximate).
	/// - Any case involving a `Tree` → `Tree` (exact).
	///
	/// # Errors
	/// Returns an error if the zoom levels differ or a quadtree operation fails.
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
	pub fn intersection(&self, other: &TileCover) -> Result<TileCover> {
		if let (TileCover::Bbox(a), TileCover::Bbox(b)) = (self, other) {
			let mut result = *a;
			result.intersect_with(b)?;
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
	pub fn difference(&self, other: &TileCover) -> Result<TileCover> {
		let a = self.to_tree();
		let b = other.to_tree();
		Ok(TileCover::Tree(a.difference(&b)?))
	}
}
