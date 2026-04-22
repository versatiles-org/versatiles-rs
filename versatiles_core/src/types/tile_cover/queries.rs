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
	use crate::{TileBBox, TileQuadtree};
	use rstest::rstest;

	#[rstest]
	#[case(0)]
	#[case(5)]
	#[case(15)]
	#[case(30)]
	fn level_is_preserved_across_both_variants(#[case] lvl: u8) {
		assert_eq!(TileCover::new_empty(lvl).unwrap().level(), lvl);
		assert_eq!(TileCover::from(TileQuadtree::new_empty(lvl).unwrap()).level(), lvl);
		assert_eq!(TileCover::from(TileQuadtree::new_full(lvl).unwrap()).level(), lvl);
		assert_eq!(TileCover::from(TileBBox::new_full(lvl).unwrap()).level(), lvl);
	}

	#[rstest]
	// (is_empty, is_full, count) pairs for both variants at several levels.
	#[case(TileCover::new_empty(3).unwrap(), true, false, 0)]
	#[case(TileCover::new_full(3).unwrap(), false, true, 64)]
	#[case(TileCover::from(TileQuadtree::new_empty(4).unwrap()), true, false, 0)]
	#[case(TileCover::from(TileQuadtree::new_full(4).unwrap()), false, true, 256)]
	#[case(TileCover::from(TileBBox::from_min_and_max(2, 0, 0, 1, 1).unwrap()), false, false, 4)]
	fn queries_match_expectations(
		#[case] c: TileCover,
		#[case] expected_empty: bool,
		#[case] expected_full: bool,
		#[case] expected_count: u64,
	) {
		assert_eq!(c.is_empty(), expected_empty);
		assert_eq!(c.is_full(), expected_full);
		assert_eq!(c.count_tiles(), expected_count);
	}
}
