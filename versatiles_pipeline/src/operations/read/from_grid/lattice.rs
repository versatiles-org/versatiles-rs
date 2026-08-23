//! The grid itself: where cells sit, and which of them a tile needs.

use std::ops::RangeInclusive;

use geo_types::{Coord, coord};

use super::projection::Projection;

/// How many points each side of a tile is sampled at when looking for the cells
/// it covers. A tile's edges are straight in mercator but curved in the grid's
/// CRS, so its corners alone understate the area it spans.
const BOUNDARY_SAMPLES: usize = 8;

/// A regular grid of square cells in some CRS.
#[derive(Debug, Clone, Copy)]
pub struct Lattice {
	/// Cell edge length, in the CRS's own units.
	pub size: f64,
	/// Where the cell with index `(0, 0)` has its lower-left corner.
	pub offset: [f64; 2],
}

impl Lattice {
	/// The lower-left corner of cell `(ix, iy)`.
	///
	/// Always computed from the integer indices, never by adding `size` to a
	/// previous corner: two cells sharing a corner have to arrive at the very
	/// same coordinate, and accumulated addition would not.
	#[must_use]
	pub fn corner(&self, ix: i64, iy: i64) -> Coord<f64> {
		coord! {
			x: self.offset[0] + ix as f64 * self.size,
			y: self.offset[1] + iy as f64 * self.size,
		}
	}

	/// The index ranges of every cell that can reach into `tile_mercator`.
	///
	/// Errs on the generous side — one cell of margin around the sampled extent
	/// — since a cell wrongly left out is a hole in the map, while a cell
	/// wrongly included is clipped away.
	#[must_use]
	pub fn candidates(
		&self,
		projection: &dyn Projection,
		tile_mercator: [f64; 4],
	) -> (RangeInclusive<i64>, RangeInclusive<i64>) {
		let [x_min, y_min, x_max, y_max] = tile_mercator;
		let (mut left, mut bottom) = (f64::INFINITY, f64::INFINITY);
		let (mut right, mut top) = (f64::NEG_INFINITY, f64::NEG_INFINITY);

		for step in 0..=BOUNDARY_SAMPLES {
			let t = step as f64 / BOUNDARY_SAMPLES as f64;
			let x = x_min + (x_max - x_min) * t;
			let y = y_min + (y_max - y_min) * t;

			for point in [
				coord! { x: x, y: y_min },
				coord! { x: x, y: y_max },
				coord! { x: x_min, y: y },
				coord! { x: x_max, y: y },
			] {
				let projected = projection.to_source(point);
				if !projected.x.is_finite() || !projected.y.is_finite() {
					continue;
				}
				left = left.min(projected.x);
				right = right.max(projected.x);
				bottom = bottom.min(projected.y);
				top = top.max(projected.y);
			}
		}

		if !left.is_finite() || !bottom.is_finite() {
			// Nothing projected: no cells rather than every cell.
			#[allow(clippy::reversed_empty_ranges)]
			return (0..=-1, 0..=-1);
		}

		// One cell of margin: the sampled outline can understate the extent
		// between samples where the projection curves, and a cell wrongly left
		// out is a hole in the map while a cell wrongly included is clipped away.
		(
			self.index(left, 0) - 1..=self.index(right, 0) + 1,
			self.index(bottom, 1) - 1..=self.index(top, 1) + 1,
		)
	}

	/// The index of the cell containing `value` along `axis`.
	#[allow(clippy::cast_possible_truncation)]
	fn index(&self, value: f64, axis: usize) -> i64 {
		((value - self.offset[axis]) / self.size).floor() as i64
	}
}

#[cfg(test)]
mod tests {
	use super::{
		super::projection::{Laea, WebMercator},
		*,
	};

	const GRID: Lattice = Lattice {
		size: 1000.0,
		offset: [0.0, 0.0],
	};

	#[test]
	fn corners_come_from_indices() {
		assert_eq!(GRID.corner(0, 0), coord! { x: 0.0, y: 0.0 });
		assert_eq!(GRID.corner(4341, 2691), coord! { x: 4_341_000.0, y: 2_691_000.0 });
		assert_eq!(GRID.corner(-3, -2), coord! { x: -3000.0, y: -2000.0 });

		let offset = Lattice {
			size: 1000.0,
			offset: [500.0, -250.0],
		};
		assert_eq!(offset.corner(2, 3), coord! { x: 2500.0, y: 2750.0 });
	}

	#[test]
	fn a_shared_corner_is_the_same_coordinate_from_either_cell() {
		// The top-right corner of (7, 8) and the bottom-left of (8, 9).
		assert_eq!(GRID.corner(8, 9), GRID.corner(7 + 1, 8 + 1));
	}

	#[test]
	fn candidates_cover_the_tile_with_margin() {
		// A tile 10 km across in mercator, on a 1 km grid in the same space.
		let (xs, ys) = GRID.candidates(&WebMercator, [10_000.0, 20_000.0, 20_000.0, 30_000.0]);
		assert!(xs.contains(&10) && xs.contains(&19), "{xs:?}");
		assert!(ys.contains(&20) && ys.contains(&29), "{ys:?}");
		// One cell of margin on each side, no more.
		assert_eq!(xs, 9..=21);
		assert_eq!(ys, 19..=31);
	}

	#[test]
	fn candidates_follow_a_curved_projection() {
		// Berlin at zoom 10, in a grid whose CRS is not mercator.
		let laea = Laea::etrs89();
		let tile = [1_487_158.0, 6_887_893.0, 1_526_301.0, 6_927_036.0];
		let (xs, ys) = GRID.candidates(&laea, tile);

		// The tile's own center has to be inside the returned range.
		let center = laea.to_source(coord! { x: 1_506_729.0, y: 6_907_464.0 });
		#[allow(clippy::cast_possible_truncation)]
		let (cx, cy) = ((center.x / 1000.0).floor() as i64, (center.y / 1000.0).floor() as i64);
		assert!(xs.contains(&cx) && ys.contains(&cy), "{xs:?} {ys:?} vs ({cx}, {cy})");
	}
}
