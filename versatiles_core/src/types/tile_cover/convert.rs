//! Conversion methods for [`TileCover`].

use super::TileCover;
use crate::{TileBBox, TileQuadtree};
use anyhow::Result;

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
	pub fn to_tree(&self) -> Result<TileQuadtree> {
		match self {
			TileCover::Bbox(b) => TileQuadtree::from_bbox(b),
			TileCover::Tree(t) => Ok(t.clone()),
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
			let tree = TileQuadtree::from_bbox(b).expect("TileQuadtree::from_bbox should not fail for a valid TileBBox");
			*self = TileCover::Tree(tree);
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
