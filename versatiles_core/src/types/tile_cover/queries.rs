//! Query methods for [`TileCover`].

use super::TileCover;

impl TileCover {
	/// Returns the zoom level of this cover.
	#[must_use]
	pub fn level(&self) -> u8 {
		match self {
			TileCover::Bbox(b) => b.level(),
			TileCover::Tree(t) => t.level(),
		}
	}

	/// Returns `true` if this cover contains no tiles.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		match self {
			TileCover::Bbox(b) => b.is_empty(),
			TileCover::Tree(t) => t.is_empty(),
		}
	}

	/// Returns `true` if this cover contains all tiles at its zoom level.
	#[must_use]
	pub fn is_full(&self) -> bool {
		match self {
			TileCover::Bbox(b) => b.is_full(),
			TileCover::Tree(t) => t.is_full(),
		}
	}

	/// Returns the total number of tiles in this cover.
	#[must_use]
	pub fn count_tiles(&self) -> u64 {
		match self {
			TileCover::Bbox(b) => b.count_tiles(),
			TileCover::Tree(t) => t.count_tiles(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::TileQuadtree;

	#[test]
	fn level() {
		assert_eq!(TileCover::new_empty(7).unwrap().level(), 7);
		assert_eq!(TileCover::from(TileQuadtree::new_empty(5).unwrap()).level(), 5);
	}

	#[test]
	fn is_full_tree_variant() {
		let c = TileCover::from(TileQuadtree::new_full(3).unwrap());
		assert!(c.is_full());
		let c2 = TileCover::from(TileQuadtree::new_empty(3).unwrap());
		assert!(!c2.is_full());
	}
}
