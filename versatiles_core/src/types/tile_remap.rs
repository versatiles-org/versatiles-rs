//! Remapping tile coordinates: TMS row order, transposed axes, and the rest.
//!
//! Flipping each axis and exchanging them generates the eight symmetries of the
//! square — four rotations and four reflections — which is every relabelling
//! that maps the tile grid onto itself. Three booleans therefore cover all of
//! them, and because the set is closed, applying two remaps in succession
//! always equals a single remap: chaining is never necessary.
//!
//! The order is fixed and part of this type rather than a convention at each
//! call site. `flip_x` and `flip_y` commute with one another, but neither
//! commutes with `swap_xy`, and spelling that out per call site is how
//! `TilesConvertReader` came to answer `tile()` and `tile_stream()` differently
//! for the same coordinate (issue #230).

use crate::{TileBBox, TileCoord, TileCover, TilePyramid};

/// A relabelling of tile coordinates within each zoom level.
///
/// Applied in a fixed order: `flip_x`, then `flip_y`, then `swap_xy`. With
/// `M = 2^z - 1`:
///
/// | `flip_x` | `flip_y` | `swap_xy` | result       | meaning                            |
/// | -------- | -------- | --------- | ------------ | ---------------------------------- |
/// | false    | false    | false     | `(x, y)`     | identity                           |
/// | true     | false    | false     | `(M−x, y)`   | mirror horizontally                |
/// | false    | true     | false     | `(x, M−y)`   | mirror vertically: TMS ↔ XYZ       |
/// | true     | true     | false     | `(M−x, M−y)` | rotate 180°                        |
/// | false    | false    | true      | `(y, x)`     | transpose — `z/y/x` layouts        |
/// | true     | false    | true      | `(y, M−x)`   | rotate 90°                         |
/// | false    | true     | true      | `(M−y, x)`   | rotate 270°                        |
/// | true     | true     | true      | `(M−y, M−x)` | transpose about the other diagonal |
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileRemap {
	/// Mirror horizontally: `x → 2^z − 1 − x`.
	pub flip_x: bool,
	/// Mirror vertically: `y → 2^z − 1 − y`. Converts between TMS row order
	/// (origin bottom-left) and the XYZ order used everywhere else.
	pub flip_y: bool,
	/// Exchange the axes: `(x, y) → (y, x)`. Applied *after* the flips.
	pub swap_xy: bool,
}

impl TileRemap {
	/// A remap that changes nothing.
	#[must_use]
	pub fn identity() -> Self {
		TileRemap::default()
	}

	/// A remap from its three flags, applied in the documented order.
	#[must_use]
	pub fn new(flip_x: bool, flip_y: bool, swap_xy: bool) -> Self {
		TileRemap {
			flip_x,
			flip_y,
			swap_xy,
		}
	}

	/// `true` when this remap leaves every coordinate untouched.
	#[must_use]
	pub fn is_identity(&self) -> bool {
		self == &TileRemap::default()
	}

	/// The remap that undoes this one.
	///
	/// Without `swap_xy` every remap is its own inverse, since each flip is an
	/// involution and the two commute. With it, undoing means exchanging the
	/// axes back first, which turns a flip of one axis into a flip of the other
	/// — so the two flip flags trade places.
	#[must_use]
	pub fn inverted(&self) -> Self {
		if self.swap_xy {
			TileRemap {
				flip_x: self.flip_y,
				flip_y: self.flip_x,
				swap_xy: true,
			}
		} else {
			*self
		}
	}

	/// Applies this remap to a single coordinate.
	pub fn apply_to_coord(&self, coord: &mut TileCoord) {
		if self.flip_x {
			coord.flip_x();
		}
		if self.flip_y {
			coord.flip_y();
		}
		if self.swap_xy {
			coord.swap_xy();
		}
	}

	/// Applies this remap to a bounding box.
	pub fn apply_to_bbox(&self, bbox: &mut TileBBox) {
		if self.flip_x {
			bbox.flip_x();
		}
		if self.flip_y {
			bbox.flip_y();
		}
		if self.swap_xy {
			bbox.swap_xy();
		}
	}

	/// Applies this remap to a tile cover.
	pub fn apply_to_cover(&self, cover: &mut TileCover) {
		if self.flip_x {
			cover.flip_x();
		}
		if self.flip_y {
			cover.flip_y();
		}
		if self.swap_xy {
			cover.swap_xy();
		}
	}

	/// Applies this remap to every level of a pyramid.
	pub fn apply_to_pyramid(&self, pyramid: &mut TilePyramid) {
		if self.flip_x {
			pyramid.flip_x();
		}
		if self.flip_y {
			pyramid.flip_y();
		}
		if self.swap_xy {
			pyramid.swap_xy();
		}
	}

	/// The same coordinate, remapped.
	#[must_use]
	pub fn remapped_coord(&self, mut coord: TileCoord) -> TileCoord {
		self.apply_to_coord(&mut coord);
		coord
	}

	/// The same bounding box, remapped.
	#[must_use]
	pub fn remapped_bbox(&self, mut bbox: TileBBox) -> TileBBox {
		self.apply_to_bbox(&mut bbox);
		bbox
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const LEVEL: u8 = 3;
	const MAX: u32 = 7; // 2^3 - 1

	fn all() -> Vec<TileRemap> {
		let mut out = Vec::new();
		for swap_xy in [false, true] {
			for flip_y in [false, true] {
				for flip_x in [false, true] {
					out.push(TileRemap::new(flip_x, flip_y, swap_xy));
				}
			}
		}
		out
	}

	fn grid() -> Vec<TileCoord> {
		(0..=MAX)
			.flat_map(|x| (0..=MAX).map(move |y| TileCoord::new(LEVEL, x, y).unwrap()))
			.collect()
	}

	fn signature(remap: &TileRemap) -> Vec<TileCoord> {
		grid().into_iter().map(|c| remap.remapped_coord(c)).collect()
	}

	/// Three booleans, eight genuinely different relabellings.
	#[test]
	fn the_eight_combinations_are_eight_distinct_maps() {
		let signatures = all().iter().map(signature).collect::<std::collections::HashSet<_>>();
		assert_eq!(signatures.len(), 8);
	}

	/// The set is closed, which is why chaining two remaps is never necessary:
	/// any composition is itself one of the eight.
	#[test]
	fn composing_two_remaps_always_yields_another() {
		let known = all().iter().map(signature).collect::<std::collections::HashSet<_>>();

		for first in all() {
			for second in all() {
				let composed = grid()
					.into_iter()
					.map(|c| second.remapped_coord(first.remapped_coord(c)))
					.collect::<Vec<_>>();
				assert!(known.contains(&composed), "{first:?} then {second:?} escapes the set");
			}
		}
	}

	/// The property every wrapper depends on: undoing a remap returns the
	/// original coordinate.
	#[test]
	fn inverting_round_trips_every_combination() {
		for remap in all() {
			let inverse = remap.inverted();
			for coord in grid() {
				assert_eq!(
					inverse.remapped_coord(remap.remapped_coord(coord)),
					coord,
					"{remap:?} did not round-trip {coord:?}"
				);
			}
		}
	}

	/// Only `swap_xy` makes a remap differ from its own inverse — the reason
	/// `inverted` can be a flag exchange rather than a stored sequence.
	#[test]
	fn only_swapping_makes_a_remap_asymmetric() {
		for remap in all() {
			assert_eq!(
				remap.inverted() == remap,
				!(remap.swap_xy && remap.flip_x != remap.flip_y),
				"unexpected self-inverse status for {remap:?}"
			);
		}
	}

	/// Why the order is fixed inside this type rather than left to the caller.
	#[test]
	fn flips_and_swap_do_not_commute() {
		let coord = TileCoord::new(LEVEL, 1, 2).unwrap();

		let flip_then_swap = TileRemap::new(false, true, true).remapped_coord(coord);
		let mut swap_then_flip = coord;
		swap_then_flip.swap_xy();
		swap_then_flip.flip_y();

		assert_ne!(
			flip_then_swap, swap_then_flip,
			"if these ever agree, the reason for fixing the order has gone"
		);
	}

	#[test]
	fn identity_is_identity() {
		let remap = TileRemap::identity();
		assert!(remap.is_identity());
		assert_eq!(signature(&remap), grid());
		assert_eq!(remap.inverted(), remap);
		assert!(!TileRemap::new(false, true, false).is_identity());
	}

	#[test]
	fn a_bbox_follows_its_corners() {
		for remap in all() {
			let bbox = TileBBox::from_min_and_max(LEVEL, 1, 2, 3, 4).unwrap();
			let remapped = remap.remapped_bbox(bbox);
			for x in 1..=3 {
				for y in 2..=4 {
					let corner = remap.remapped_coord(TileCoord::new(LEVEL, x, y).unwrap());
					assert!(remapped.includes_coord(&corner), "{remap:?} lost {corner:?}");
				}
			}
		}
	}
}
