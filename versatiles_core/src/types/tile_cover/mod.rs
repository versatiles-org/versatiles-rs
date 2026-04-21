//! A unified single-zoom tile coverage type.
//!
//! [`TileCover`] is an enum that wraps either a [`TileBBox`] (rectangular,
//! fast) or a [`TileQuadtree`] (arbitrary shape, memory-efficient). Callers
//! use the same API regardless of which representation is in use.
//!
//! # Representation choice
//! - Starts as `Bbox` for all constructors that produce rectangular coverage.
//! - Automatically upgrades to `Tree` only when a non-rectangular operation is
//!   requested: [`remove_coord`](TileCover::remove_coord),
//!   [`remove_bbox`](TileCover::remove_bbox),
//!   [`intersect_bbox`](TileCover::intersect_bbox), or
//!   [`difference`](TileCover::difference).
//! - [`TileQuadtree`]-based constructors always produce the `Tree` variant.

mod constructors;
mod convert;
mod fmt;
mod include;
pub(super) mod info_trait;
mod intersect;
mod iter;
mod mutate;
mod queries;
mod set_ops;

use crate::{TileBBox, TileQuadtree};

/// A set of tiles at a fixed zoom level, stored as either a bounding box or a
/// quadtree.
///
/// # Variants
/// - `Bbox(TileBBox)` — rectangular coverage; fast for axis-aligned regions.
/// - `Tree(TileQuadtree)` — arbitrary coverage; efficient for non-rectangular
///   shapes.
///
/// # Examples
/// ```rust
/// use versatiles_core::{TileBBox, TileCover};
///
/// let bbox = TileBBox::from_min_and_max(5, 3, 4, 10, 15).unwrap();
/// let cover = TileCover::from(bbox);
/// assert!(!cover.is_empty());
/// assert_eq!(cover.level(), 5);
/// assert_eq!(cover.count_tiles(), 96);
/// ```
#[derive(Clone)]
pub enum TileCover {
	/// Rectangular tile coverage.
	Bbox(TileBBox),
	/// Arbitrary (non-rectangular) tile coverage.
	Tree(TileQuadtree),
}
