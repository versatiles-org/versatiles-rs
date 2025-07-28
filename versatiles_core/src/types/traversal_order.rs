use anyhow::{Result, bail, ensure};
use enumset::{EnumSet, EnumSetType};
use std::ops::{BitAnd, BitOr};

/// Orderings that control how a tile pyramid is traversed.
///
/// Each variant describes the sequence in which tile bounding boxes or
/// coordinates are visited when exporting, indexing or streaming tiled
/// data.  Choosing the right order improves I/O locality for different
/// storage back‑ends:
///
/// * **`TopDown`** – Visit zoom levels in ascending order (`min_zoom → max_zoom`).
/// * **`BottomUp`** – Visit zoom levels in descending order (`max_zoom → min_zoom`).
/// * **`DepthFirst16`** – Depth‑first quadtree traversal in 16 × 16‐tile chunks.
/// * **`DepthFirst256`** – Depth‑first quadtree traversal in 256 × 256‐tile chunks.
/// * **`PMTiles64`** – 64 × 64 Hilbert curve order that matches the canonical layout used by **PMTiles v3** archives.
#[derive(EnumSetType, Debug, Hash)]
pub enum TraversalOrder {
	TopDown,
	BottomUp,
	DepthFirst16,
	DepthFirst256,
	PMTiles64,
}

/// A bit‑set wrapper around one or more [`TraversalOrder`] values.
///
/// The set lets callers express *acceptable* traversal orders and then pick
/// the “best” one that is supported by the current data sink or source.
/// It supports the usual boolean algebra via the `&` (intersection) and
/// `|` (union) operators and defaults to **all** variants enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraversalOrderSet {
	set: EnumSet<TraversalOrder>,
}

impl TraversalOrderSet {
	/// Creates a set with **no** traversal orders enabled.
	pub fn new_empty() -> Self {
		TraversalOrderSet { set: EnumSet::new() }
	}

	/// Creates a set that contains *every* [`TraversalOrder`] variant.
	pub fn new_all() -> Self {
		TraversalOrderSet { set: EnumSet::all() }
	}

	pub fn new(orders: Vec<TraversalOrder>) -> Self {
		TraversalOrderSet {
			set: orders.into_iter().collect(),
		}
	}

	/// Returns the first order in the built‑in preference list that is
	/// contained in this set.
	///
	/// Preference: `TopDown`, `BottomUp`, `DepthFirst256`,
	/// `DepthFirst16`, `PMTiles64`.
	///
	/// # Errors
	/// Fails with `TraversalOrderSet is empty` if the set has no variants.
	pub fn get_best(&self) -> Result<TraversalOrder> {
		use TraversalOrder::*;
		self.get_best_of(&[TopDown, BottomUp, DepthFirst256, DepthFirst16, PMTiles64])
	}

	/// Returns the first order from `orders` that is present in the set.
	///
	/// # Errors
	/// * `TraversalOrderSet is empty` – the set has no variants.  
	/// * `None of the specified traversal orders are available in the set`
	///   – none of the requested orders intersect with the set.
	pub fn get_best_of(&self, orders: &[TraversalOrder]) -> Result<TraversalOrder> {
		ensure!(!self.set.is_empty(), "TraversalOrderSet is empty");

		for order in orders.iter() {
			if self.set.contains(*order) {
				return Ok(*order);
			}
		}

		bail!("None of the specified traversal orders are available in the set");
	}
}

impl Default for TraversalOrderSet {
	fn default() -> Self {
		Self::new_all()
	}
}

/// Intersection (`&`) of two [`TraversalOrderSet`]s.
///
/// The result contains only orders present in **both** operands.
impl BitAnd for TraversalOrderSet {
	type Output = Self;

	fn bitand(self, rhs: Self) -> Self::Output {
		TraversalOrderSet {
			set: self.set & rhs.set,
		}
	}
}

/// Union (`|`) of two [`TraversalOrderSet`]s.
///
/// The result contains orders present in **either** operand.
impl BitOr for TraversalOrderSet {
	type Output = Self;

	fn bitor(self, rhs: Self) -> Self::Output {
		TraversalOrderSet {
			set: self.set | rhs.set,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use TraversalOrder::*;

	#[test]
	fn test_new_empty() {
		let set = TraversalOrderSet::new_empty();
		assert!(set.set.is_empty());
	}

	#[test]
	fn test_new_all() {
		let set = TraversalOrderSet::new_all();
		assert_eq!(set.set.len(), 5); // All 5 variants should be included
	}

	#[test]
	fn test_get_best() {
		let mut set = TraversalOrderSet::new_empty();
		set.set.insert(DepthFirst256);
		set.set.insert(TopDown);

		assert_eq!(set.get_best().unwrap(), TopDown);
	}

	#[test]
	fn test_get_best_of() {
		let mut set = TraversalOrderSet::new_empty();
		set.set.insert(DepthFirst16);

		let result = set.get_best_of(&[TopDown, BottomUp, DepthFirst16]);
		assert_eq!(result.unwrap(), DepthFirst16);
	}

	#[test]
	fn test_get_best_of_empty_set() {
		let set = TraversalOrderSet::new_empty();
		let result = set.get_best_of(&[TopDown, BottomUp]);
		assert!(result.is_err());
	}

	#[test]
	fn test_default() {
		let set = TraversalOrderSet::default();
		assert_eq!(set.set.len(), 5); // Default should include all variants
	}

	#[test]
	fn test_bitand() {
		let mut set1 = TraversalOrderSet::new_empty();
		set1.set.insert(TopDown);
		set1.set.insert(BottomUp);

		let mut set2 = TraversalOrderSet::new_empty();
		set2.set.insert(BottomUp);
		set2.set.insert(DepthFirst16);

		let result = set1 & set2;
		assert!(result.set.contains(BottomUp));
		assert!(!result.set.contains(TopDown));
		assert!(!result.set.contains(DepthFirst16));
	}

	#[test]
	fn test_bitor() {
		let mut set1 = TraversalOrderSet::new_empty();
		set1.set.insert(TopDown);

		let mut set2 = TraversalOrderSet::new_empty();
		set2.set.insert(BottomUp);

		let result = set1 | set2;
		assert!(result.set.contains(TopDown));
		assert!(result.set.contains(BottomUp));
	}
}
