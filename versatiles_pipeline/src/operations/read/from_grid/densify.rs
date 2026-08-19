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

/// The mercator vertices of the edge from `from` to `to`, excluding `to`.
///
/// Both are coordinates in the grid's CRS. The result always starts with `from`
/// projected, so concatenating the four edges of a cell yields its ring.
pub fn edge_vertices(from: Coord<f64>, to: Coord<f64>, projection: &dyn Projection, tolerance: f64) -> Vec<Coord<f64>> {
	let flipped = !is_canonical(from, to);
	let (a, b) = if flipped { (to, from) } else { (from, to) };

	let (a_mercator, b_mercator) = (projection.to_mercator(a), projection.to_mercator(b));
	let mut interior = Vec::new();
	subdivide(a, b, a_mercator, b_mercator, projection, tolerance, 0, &mut interior);

	let mut vertices = Vec::with_capacity(interior.len() + 1);
	if flipped {
		vertices.push(b_mercator);
		vertices.extend(interior.into_iter().rev());
	} else {
		vertices.push(a_mercator);
		vertices.extend(interior);
	}
	vertices
}

/// Order two endpoints the same way from either side of the edge.
fn is_canonical(from: Coord<f64>, to: Coord<f64>) -> bool {
	(from.x, from.y) <= (to.x, to.y)
}

/// Push the interior vertices between `a` and `b`, in order.
#[allow(clippy::too_many_arguments)]
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

	#[test]
	fn a_straight_projection_adds_nothing() {
		let a = coord! { x: 0.0, y: 0.0 };
		let b = coord! { x: 1_000_000.0, y: 0.0 };
		assert_eq!(edge_vertices(a, b, &WebMercator, 0.1).len(), 1);

		// Meridians and parallels are straight lines in mercator too.
		let a = coord! { x: 10.0, y: 50.0 };
		let b = coord! { x: 11.0, y: 50.0 };
		assert_eq!(edge_vertices(a, b, &Wgs84, 0.1).len(), 1);
	}

	#[test]
	fn a_curved_projection_adds_vertices() {
		let laea = Laea::etrs89();
		// A 100 km edge far from the projection's origin curves noticeably.
		let a = coord! { x: 2_500_000.0, y: 1_500_000.0 };
		let b = coord! { x: 2_600_000.0, y: 1_500_000.0 };
		assert!(edge_vertices(a, b, &laea, 1.0).len() > 1);
		// A tighter tolerance asks for more.
		assert!(edge_vertices(a, b, &laea, 0.01).len() > edge_vertices(a, b, &laea, 1.0).len());
	}

	#[test]
	fn both_sides_of_an_edge_agree_bit_for_bit() {
		let laea = Laea::etrs89();
		let a = coord! { x: 4_321_000.0, y: 2_691_000.0 };
		let b = coord! { x: 4_322_000.0, y: 2_691_000.0 };

		let forward = edge_vertices(a, b, &laea, 0.001);
		let mut backward = edge_vertices(b, a, &laea, 0.001);
		backward.reverse();

		// `edge_vertices` excludes its endpoint, so the two lists describe the
		// same edge shifted by one: compare the vertices they share.
		assert_eq!(forward.len(), backward.len());
		assert_eq!(forward[1..], backward[..backward.len() - 1]);
	}

	#[test]
	fn the_first_vertex_is_the_starting_corner() {
		let laea = Laea::etrs89();
		let a = coord! { x: 4_321_000.0, y: 2_691_000.0 };
		let b = coord! { x: 4_322_000.0, y: 2_691_000.0 };
		assert_eq!(edge_vertices(a, b, &laea, 0.001)[0], laea.to_mercator(a));
		assert_eq!(edge_vertices(b, a, &laea, 0.001)[0], laea.to_mercator(b));
	}
}
