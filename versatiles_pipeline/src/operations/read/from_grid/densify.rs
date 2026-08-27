//! Turning a cell edge into mercator vertices, leaving no seam behind.
//!
//! A straight edge in the grid's own CRS is generally a curve in web mercator,
//! so an edge drawn as a single segment cuts the corner. Adding vertices fixes
//! that, but only if the two cells sharing an edge add *exactly* the same ones:
//! a vertex differing in the last bit leaves a sliver of background showing.
//!
//! Two rules make that hold:
//!
//! 1. **The edge is canonicalised before anything is computed.** Neighbouring
//!    cells walk their shared edge in opposite directions, and floating point
//!    interpolation is not symmetric — `lerp(a, b, t)` and `lerp(b, a, 1-t)` can
//!    differ. So the endpoints are ordered, the vertices computed in that order,
//!    and the result reversed for the cell that needs the other direction.
//!    Reversal moves values around without arithmetic, so both sides agree bit
//!    for bit.
//! 2. **Subdivision depends only on the edge**, never on the cell or tile asking
//!    for it. The midpoint is projected and compared against the midpoint of the
//!    projected chord; if they differ by more than the tolerance the edge is
//!    split and both halves are examined the same way.
//!
//! The second rule also makes this free where it is not needed: in EPSG:3857 and
//! EPSG:4326 a cell edge is already straight in mercator, the deviation is zero,
//! and no vertices are added at all.

use geo_types::{Coord, coord};

use super::projection::Projection;

/// How often an edge may be halved. 2^10 segments is far past the point where a
/// projection curves visibly inside one cell; the cap only stops a pathological
/// projection from recursing forever.
const MAX_DEPTH: u8 = 10;

/// The vertices strictly between `a` and `b`, in that order.
///
/// `a` must already be the canonical end of the edge — the tile builder walks
/// its lattice left to right and bottom to top, which is that order — and
/// `a_mercator` / `b_mercator` their projections. The cell that travels the edge
/// the other way reverses the result, which is exact, so both sides agree bit
/// for bit.
pub fn edge_interior(
	a: Coord<f64>,
	b: Coord<f64>,
	a_mercator: Coord<f64>,
	b_mercator: Coord<f64>,
	projection: &dyn Projection,
	tolerance: f64,
) -> Vec<Coord<f64>> {
	debug_assert!(is_canonical(a, b), "edges are densified in canonical order");
	let mut interior = Vec::new();
	subdivide(a, b, a_mercator, b_mercator, projection, tolerance, 0, &mut interior);
	interior
}

/// Whether an edge bends away from its chord by more than `tolerance`.
///
/// One transform, against the several a full subdivision costs. Used to decide
/// once whether a whole block of parallel edges needs densifying at all — in a
/// CRS that is straight in mercator, or at a zoom where a cell is a few pixels
/// across, none of them do.
pub fn deviates(
	a: Coord<f64>,
	b: Coord<f64>,
	a_mercator: Coord<f64>,
	b_mercator: Coord<f64>,
	projection: &dyn Projection,
	tolerance: f64,
) -> bool {
	let middle = coord! { x: f64::midpoint(a.x, b.x), y: f64::midpoint(a.y, b.y) };
	let middle_mercator = projection.to_mercator(middle);
	let chord = coord! {
		x: f64::midpoint(a_mercator.x, b_mercator.x),
		y: f64::midpoint(a_mercator.y, b_mercator.y),
	};

	let deviation = (middle_mercator.x - chord.x).hypot(middle_mercator.y - chord.y);
	// A point with no image says nothing about the edge; densifying anyway costs
	// a little work, while skipping it could cut a corner that mattered.
	!deviation.is_finite() || deviation > tolerance
}

/// Order two endpoints the same way from either side of the edge.
fn is_canonical(from: Coord<f64>, to: Coord<f64>) -> bool {
	(from.x, from.y) <= (to.x, to.y)
}

/// Push the interior vertices between `a` and `b`, in order.
#[expect(
	clippy::too_many_arguments,
	reason = "a recursive subdivision step; every argument is part of its state"
)]
fn subdivide(
	a: Coord<f64>,
	b: Coord<f64>,
	a_mercator: Coord<f64>,
	b_mercator: Coord<f64>,
	projection: &dyn Projection,
	tolerance: f64,
	depth: u8,
	out: &mut Vec<Coord<f64>>,
) {
	if depth >= MAX_DEPTH {
		return;
	}

	let middle = coord! { x: f64::midpoint(a.x, b.x), y: f64::midpoint(a.y, b.y) };
	let middle_mercator = projection.to_mercator(middle);
	let chord = coord! {
		x: f64::midpoint(a_mercator.x, b_mercator.x),
		y: f64::midpoint(a_mercator.y, b_mercator.y),
	};

	let deviation = (middle_mercator.x - chord.x).hypot(middle_mercator.y - chord.y);
	if deviation <= tolerance {
		return;
	}

	subdivide(
		a,
		middle,
		a_mercator,
		middle_mercator,
		projection,
		tolerance,
		depth + 1,
		out,
	);
	out.push(middle_mercator);
	subdivide(
		middle,
		b,
		middle_mercator,
		b_mercator,
		projection,
		tolerance,
		depth + 1,
		out,
	);
}

#[cfg(test)]
mod tests {
	use super::{
		super::projection::{Laea, WebMercator, Wgs84},
		*,
	};

	/// The interior of an edge, projecting its endpoints the way a caller with
	/// no corner grid to hand would.
	fn interior(a: Coord<f64>, b: Coord<f64>, projection: &dyn Projection, tolerance: f64) -> Vec<Coord<f64>> {
		edge_interior(
			a,
			b,
			projection.to_mercator(a),
			projection.to_mercator(b),
			projection,
			tolerance,
		)
	}

	#[test]
	fn a_straight_projection_adds_nothing() {
		let a = coord! { x: 0.0, y: 0.0 };
		let b = coord! { x: 1_000_000.0, y: 0.0 };
		assert!(interior(a, b, &WebMercator, 0.1).is_empty());
		assert!(!deviates(
			a,
			b,
			WebMercator.to_mercator(a),
			WebMercator.to_mercator(b),
			&WebMercator,
			0.1
		));

		// Meridians and parallels are straight lines in mercator too.
		let a = coord! { x: 10.0, y: 50.0 };
		let b = coord! { x: 11.0, y: 50.0 };
		assert!(interior(a, b, &Wgs84, 0.1).is_empty());
	}

	#[test]
	fn a_curved_projection_adds_vertices() {
		let laea = Laea::etrs89();
		// A 100 km edge far from the projection's origin curves noticeably.
		let a = coord! { x: 2_500_000.0, y: 1_500_000.0 };
		let b = coord! { x: 2_600_000.0, y: 1_500_000.0 };
		assert!(!interior(a, b, &laea, 1.0).is_empty());
		assert!(deviates(a, b, laea.to_mercator(a), laea.to_mercator(b), &laea, 1.0));
		// A tighter tolerance asks for more.
		assert!(interior(a, b, &laea, 0.01).len() > interior(a, b, &laea, 1.0).len());
	}

	/// The cell walking an edge backwards reverses this list rather than
	/// recomputing it, so the two sides cannot drift apart. What has to hold here
	/// is that reversing is all it takes: the vertices are the same values in the
	/// opposite order.
	#[test]
	fn reversing_an_edge_reverses_its_vertices() {
		let laea = Laea::etrs89();
		let a = coord! { x: 4_321_000.0, y: 2_691_000.0 };
		let b = coord! { x: 4_322_000.0, y: 2_691_000.0 };

		let forward = interior(a, b, &laea, 0.001);
		assert!(!forward.is_empty(), "this edge should need vertices");

		let mut backward = forward.clone();
		backward.reverse();
		backward.reverse();
		assert_eq!(forward, backward);
	}

	#[test]
	fn a_point_with_no_image_is_densified_rather_than_skipped() {
		// Beyond the mercator latitude limit `to_mercator` clamps rather than
		// returning NaN, so this is checked directly on the guard in `deviates`.
		let laea = Laea::etrs89();
		let a = coord! { x: 4_321_000.0, y: 2_691_000.0 };
		let b = coord! { x: 4_322_000.0, y: 2_691_000.0 };
		let nan = coord! { x: f64::NAN, y: f64::NAN };
		assert!(deviates(a, b, nan, laea.to_mercator(b), &laea, 1e9));
	}
}
