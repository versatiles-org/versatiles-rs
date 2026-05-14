//! Web Mercator projection helpers as an extension trait over `geo_types`.
//!
//! Each coordinate is treated as `(longitude, latitude)` in degrees and
//! converted to/from Web Mercator (EPSG:3857) coordinates in meters.
//!
//! Latitude is clamped to ±[`MAX_LAT`] in `to_mercator` to avoid singularities.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use geo_types::{
	Coord, Geometry, Line, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, Rect, Triangle,
};
use versatiles_core::{EARTH_RADIUS, MAX_LAT};

/// Project a single coordinate from WGS84 (lon/lat in degrees) to Web Mercator (meters).
#[inline]
#[must_use]
pub fn coord_to_mercator(c: Coord<f64>) -> Coord<f64> {
	let lon = c.x;
	let lat = c.y.clamp(-MAX_LAT, MAX_LAT);
	Coord {
		x: lon.to_radians() * EARTH_RADIUS,
		y: (lat.to_radians() / 2.0 + FRAC_PI_4).tan().ln() * EARTH_RADIUS,
	}
}

/// Project a single coordinate from Web Mercator (meters) to WGS84 (lon/lat in degrees).
#[inline]
#[must_use]
pub fn coord_from_mercator(c: Coord<f64>) -> Coord<f64> {
	Coord {
		x: (c.x / EARTH_RADIUS).to_degrees(),
		y: (2.0 * (c.y / EARTH_RADIUS).exp().atan() - FRAC_PI_2).to_degrees(),
	}
}

/// Extension trait providing WGS84 ↔ Web Mercator projection for `geo_types` geometries.
#[allow(clippy::wrong_self_convention)] // `from_mercator` mirrors `to_mercator` and naturally takes `self`.
pub trait MercatorExt: Sized {
	/// Convert from WGS84 (lon/lat in degrees) to Web Mercator (meters).
	#[must_use]
	fn to_mercator(self) -> Self;
	/// Convert from Web Mercator (meters) to WGS84 (lon/lat in degrees).
	#[must_use]
	fn from_mercator(self) -> Self;
}

impl MercatorExt for Coord<f64> {
	fn to_mercator(self) -> Self {
		coord_to_mercator(self)
	}
	fn from_mercator(self) -> Self {
		coord_from_mercator(self)
	}
}

impl MercatorExt for Point<f64> {
	fn to_mercator(self) -> Self {
		Point(coord_to_mercator(self.0))
	}
	fn from_mercator(self) -> Self {
		Point(coord_from_mercator(self.0))
	}
}

impl MercatorExt for LineString<f64> {
	fn to_mercator(self) -> Self {
		LineString::new(self.0.into_iter().map(coord_to_mercator).collect())
	}
	fn from_mercator(self) -> Self {
		LineString::new(self.0.into_iter().map(coord_from_mercator).collect())
	}
}

impl MercatorExt for Polygon<f64> {
	fn to_mercator(self) -> Self {
		let (exterior, interiors) = self.into_inner();
		Polygon::new(
			exterior.to_mercator(),
			interiors.into_iter().map(MercatorExt::to_mercator).collect(),
		)
	}
	fn from_mercator(self) -> Self {
		let (exterior, interiors) = self.into_inner();
		Polygon::new(
			exterior.from_mercator(),
			interiors.into_iter().map(MercatorExt::from_mercator).collect(),
		)
	}
}

impl MercatorExt for MultiPoint<f64> {
	fn to_mercator(self) -> Self {
		MultiPoint(self.0.into_iter().map(MercatorExt::to_mercator).collect())
	}
	fn from_mercator(self) -> Self {
		MultiPoint(self.0.into_iter().map(MercatorExt::from_mercator).collect())
	}
}

impl MercatorExt for MultiLineString<f64> {
	fn to_mercator(self) -> Self {
		MultiLineString(self.0.into_iter().map(MercatorExt::to_mercator).collect())
	}
	fn from_mercator(self) -> Self {
		MultiLineString(self.0.into_iter().map(MercatorExt::from_mercator).collect())
	}
}

impl MercatorExt for MultiPolygon<f64> {
	fn to_mercator(self) -> Self {
		MultiPolygon(self.0.into_iter().map(MercatorExt::to_mercator).collect())
	}
	fn from_mercator(self) -> Self {
		MultiPolygon(self.0.into_iter().map(MercatorExt::from_mercator).collect())
	}
}

impl MercatorExt for Line<f64> {
	fn to_mercator(self) -> Self {
		Line::new(coord_to_mercator(self.start), coord_to_mercator(self.end))
	}
	fn from_mercator(self) -> Self {
		Line::new(coord_from_mercator(self.start), coord_from_mercator(self.end))
	}
}

impl MercatorExt for Rect<f64> {
	fn to_mercator(self) -> Self {
		Rect::new(coord_to_mercator(self.min()), coord_to_mercator(self.max()))
	}
	fn from_mercator(self) -> Self {
		Rect::new(coord_from_mercator(self.min()), coord_from_mercator(self.max()))
	}
}

impl MercatorExt for Triangle<f64> {
	fn to_mercator(self) -> Self {
		Triangle::new(
			coord_to_mercator(self.v1()),
			coord_to_mercator(self.v2()),
			coord_to_mercator(self.v3()),
		)
	}
	fn from_mercator(self) -> Self {
		Triangle::new(
			coord_from_mercator(self.v1()),
			coord_from_mercator(self.v2()),
			coord_from_mercator(self.v3()),
		)
	}
}

impl MercatorExt for Geometry<f64> {
	fn to_mercator(self) -> Self {
		match self {
			Geometry::Point(g) => Geometry::Point(g.to_mercator()),
			Geometry::Line(g) => Geometry::Line(g.to_mercator()),
			Geometry::LineString(g) => Geometry::LineString(g.to_mercator()),
			Geometry::Polygon(g) => Geometry::Polygon(g.to_mercator()),
			Geometry::MultiPoint(g) => Geometry::MultiPoint(g.to_mercator()),
			Geometry::MultiLineString(g) => Geometry::MultiLineString(g.to_mercator()),
			Geometry::MultiPolygon(g) => Geometry::MultiPolygon(g.to_mercator()),
			Geometry::Rect(g) => Geometry::Rect(g.to_mercator()),
			Geometry::Triangle(g) => Geometry::Triangle(g.to_mercator()),
			Geometry::GeometryCollection(gc) => Geometry::GeometryCollection(geo_types::GeometryCollection(
				gc.0.into_iter().map(MercatorExt::to_mercator).collect(),
			)),
		}
	}
	fn from_mercator(self) -> Self {
		match self {
			Geometry::Point(g) => Geometry::Point(g.from_mercator()),
			Geometry::Line(g) => Geometry::Line(g.from_mercator()),
			Geometry::LineString(g) => Geometry::LineString(g.from_mercator()),
			Geometry::Polygon(g) => Geometry::Polygon(g.from_mercator()),
			Geometry::MultiPoint(g) => Geometry::MultiPoint(g.from_mercator()),
			Geometry::MultiLineString(g) => Geometry::MultiLineString(g.from_mercator()),
			Geometry::MultiPolygon(g) => Geometry::MultiPolygon(g.from_mercator()),
			Geometry::Rect(g) => Geometry::Rect(g.from_mercator()),
			Geometry::Triangle(g) => Geometry::Triangle(g.from_mercator()),
			Geometry::GeometryCollection(gc) => Geometry::GeometryCollection(geo_types::GeometryCollection(
				gc.0.into_iter().map(MercatorExt::from_mercator).collect(),
			)),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use approx::assert_relative_eq;
	use geo_types::GeometryCollection;

	/// Tight equality tolerance for round-trips that go through `to_radians()`
	/// then `from_radians()`. f64 round-tripping introduces ~1e-9 noise; we
	/// pick a hair below that to catch real divergence.
	const EPS: f64 = 1e-6;

	fn assert_coord_eq(a: Coord<f64>, b: Coord<f64>) {
		assert_relative_eq!(a.x, b.x, epsilon = EPS);
		assert_relative_eq!(a.y, b.y, epsilon = EPS);
	}

	// ── free-function tests ──────────────────────────────────────────────

	#[test]
	fn coord_to_mercator_origin() {
		let m = coord_to_mercator(Coord { x: 0.0, y: 0.0 });
		assert_relative_eq!(m.x, 0.0, epsilon = EPS);
		assert_relative_eq!(m.y, 0.0, epsilon = EPS);
	}

	#[test]
	fn coord_to_mercator_from_mercator_round_trip_at_multiple_locations() {
		for original in [
			Coord { x: 10.0, y: 50.0 },
			Coord { x: -73.99, y: 40.74 },
			Coord { x: 139.69, y: 35.69 },
			Coord { x: -100.0, y: -45.0 },
			Coord { x: 180.0, y: 0.0 },
		] {
			let projected = coord_to_mercator(original);
			let back = coord_from_mercator(projected);
			assert_coord_eq(back, original);
		}
	}

	#[test]
	fn latitude_clamping_avoids_infinity_at_both_poles() {
		let north = coord_to_mercator(Coord { x: 0.0, y: 90.0 });
		let south = coord_to_mercator(Coord { x: 0.0, y: -90.0 });
		assert!(north.y.is_finite());
		assert!(south.y.is_finite());
		// Clamping is symmetric around the equator.
		assert_relative_eq!(north.y, -south.y, epsilon = EPS);
	}

	#[test]
	fn longitude_passes_through_unclamped() {
		// `coord_to_mercator` does not clamp x. ±180° at the antimeridian
		// projects to the spherical equivalent without normalisation.
		let east = coord_to_mercator(Coord { x: 180.0, y: 0.0 });
		let west = coord_to_mercator(Coord { x: -180.0, y: 0.0 });
		assert_relative_eq!(east.x, -west.x, epsilon = EPS);
	}

	// ── Coord trait impl ─────────────────────────────────────────────────

	#[test]
	fn coord_trait_to_and_from_match_free_functions() {
		let c = Coord { x: 1.0, y: 2.0 };
		assert_eq!(c.to_mercator(), coord_to_mercator(c));
		assert_eq!(c.from_mercator(), coord_from_mercator(c));
	}

	// ── Point trait impl ─────────────────────────────────────────────────

	#[test]
	fn point_round_trip() {
		let p = Point::new(13.4, 52.5);
		let back = p.to_mercator().from_mercator();
		assert_relative_eq!(back.x(), p.x(), epsilon = EPS);
		assert_relative_eq!(back.y(), p.y(), epsilon = EPS);
	}

	// ── LineString trait impl ────────────────────────────────────────────

	#[test]
	fn line_string_round_trip_preserves_all_vertices() {
		let ls = LineString::from(vec![[0.0, 0.0], [10.0, 20.0], [-5.0, 30.0]]);
		let back = ls.clone().to_mercator().from_mercator();
		assert_eq!(back.0.len(), ls.0.len());
		for (a, b) in back.0.iter().zip(ls.0.iter()) {
			assert_coord_eq(*a, *b);
		}
	}

	// ── Polygon trait impl ───────────────────────────────────────────────

	#[test]
	fn polygon_round_trip_preserves_exterior_and_interiors() {
		let exterior = LineString::from(vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]]);
		let inner = LineString::from(vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0], [1.0, 1.0]]);
		let p = Polygon::new(exterior, vec![inner]);
		let back = p.clone().to_mercator().from_mercator();
		assert_eq!(back.interiors().len(), 1);
		for (a, b) in back.exterior().0.iter().zip(p.exterior().0.iter()) {
			assert_coord_eq(*a, *b);
		}
		for (a, b) in back.interiors()[0].0.iter().zip(p.interiors()[0].0.iter()) {
			assert_coord_eq(*a, *b);
		}
	}

	// ── MultiPoint / MultiLineString / MultiPolygon trait impls ──────────

	#[test]
	fn multi_point_round_trip() {
		let mp = MultiPoint(vec![
			Point::new(1.0, 2.0),
			Point::new(-5.0, -10.0),
			Point::new(170.0, 60.0),
		]);
		let back = mp.clone().to_mercator().from_mercator();
		assert_eq!(back.0.len(), mp.0.len());
		for (a, b) in back.0.iter().zip(mp.0.iter()) {
			assert_relative_eq!(a.x(), b.x(), epsilon = EPS);
			assert_relative_eq!(a.y(), b.y(), epsilon = EPS);
		}
	}

	#[test]
	fn multi_line_string_round_trip() {
		let mls = MultiLineString(vec![
			LineString::from(vec![[0.0, 0.0], [1.0, 1.0]]),
			LineString::from(vec![[10.0, 20.0], [30.0, 40.0], [50.0, 0.0]]),
		]);
		let back = mls.clone().to_mercator().from_mercator();
		assert_eq!(back.0.len(), mls.0.len());
		for (a, b) in back.0.iter().zip(mls.0.iter()) {
			assert_eq!(a.0.len(), b.0.len());
		}
	}

	#[test]
	fn multi_polygon_round_trip() {
		let mp = MultiPolygon(vec![
			Polygon::new(
				LineString::from(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]),
				vec![],
			),
			Polygon::new(
				LineString::from(vec![[10.0, 10.0], [11.0, 10.0], [11.0, 11.0], [10.0, 10.0]]),
				vec![LineString::from(vec![
					[10.2, 10.2],
					[10.8, 10.2],
					[10.8, 10.8],
					[10.2, 10.2],
				])],
			),
		]);
		let back = mp.clone().to_mercator().from_mercator();
		assert_eq!(back.0.len(), 2);
		assert_eq!(back.0[1].interiors().len(), 1);
	}

	// ── Line / Rect / Triangle trait impls ───────────────────────────────

	#[test]
	fn line_round_trip() {
		let l = Line::new(Coord { x: 1.0, y: 2.0 }, Coord { x: 3.0, y: 4.0 });
		let back = l.to_mercator().from_mercator();
		assert_coord_eq(back.start, l.start);
		assert_coord_eq(back.end, l.end);
	}

	#[test]
	fn rect_round_trip_preserves_min_max() {
		let r = Rect::new(Coord { x: -1.0, y: -2.0 }, Coord { x: 3.0, y: 4.0 });
		let back = r.to_mercator().from_mercator();
		assert_coord_eq(back.min(), r.min());
		assert_coord_eq(back.max(), r.max());
	}

	#[test]
	fn triangle_round_trip_preserves_all_vertices() {
		let t = Triangle::new(
			Coord { x: 0.0, y: 0.0 },
			Coord { x: 5.0, y: 0.0 },
			Coord { x: 0.0, y: 5.0 },
		);
		let back = t.to_mercator().from_mercator();
		assert_coord_eq(back.v1(), t.v1());
		assert_coord_eq(back.v2(), t.v2());
		assert_coord_eq(back.v3(), t.v3());
	}

	// ── Geometry dispatch — every variant ────────────────────────────────

	/// Round-trips a single `Geometry<f64>` value through both directions
	/// and asserts the variant comes back as the same kind. Specific value
	/// equality is checked per-variant in the dedicated tests above; here
	/// we only need to verify the dispatch wiring.
	fn assert_geometry_round_trip(g: &Geometry<f64>) {
		let projected = g.clone().to_mercator();
		let back = projected.from_mercator();
		assert!(
			std::mem::discriminant(g) == std::mem::discriminant(&back),
			"variant changed during round-trip"
		);
	}

	#[test]
	fn geometry_dispatch_point() {
		assert_geometry_round_trip(&Geometry::Point(Point::new(1.5, -2.5)));
	}

	#[test]
	fn geometry_dispatch_line() {
		assert_geometry_round_trip(&Geometry::Line(Line::new(
			Coord { x: 1.0, y: 2.0 },
			Coord { x: 3.0, y: 4.0 },
		)));
	}

	#[test]
	fn geometry_dispatch_line_string() {
		assert_geometry_round_trip(&Geometry::LineString(LineString::from(vec![[0.0, 0.0], [1.0, 1.0]])));
	}

	#[test]
	fn geometry_dispatch_polygon() {
		assert_geometry_round_trip(&Geometry::Polygon(Polygon::new(
			LineString::from(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]),
			vec![],
		)));
	}

	#[test]
	fn geometry_dispatch_multi_point() {
		assert_geometry_round_trip(&Geometry::MultiPoint(MultiPoint(vec![
			Point::new(1.0, 2.0),
			Point::new(3.0, 4.0),
		])));
	}

	#[test]
	fn geometry_dispatch_multi_line_string() {
		assert_geometry_round_trip(&Geometry::MultiLineString(MultiLineString(vec![LineString::from(
			vec![[0.0, 0.0], [1.0, 1.0]],
		)])));
	}

	#[test]
	fn geometry_dispatch_multi_polygon() {
		assert_geometry_round_trip(&Geometry::MultiPolygon(MultiPolygon(vec![Polygon::new(
			LineString::from(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]),
			vec![],
		)])));
	}

	#[test]
	fn geometry_dispatch_rect() {
		assert_geometry_round_trip(&Geometry::Rect(Rect::new(
			Coord { x: -1.0, y: -2.0 },
			Coord { x: 3.0, y: 4.0 },
		)));
	}

	#[test]
	fn geometry_dispatch_triangle() {
		assert_geometry_round_trip(&Geometry::Triangle(Triangle::new(
			Coord { x: 0.0, y: 0.0 },
			Coord { x: 5.0, y: 0.0 },
			Coord { x: 0.0, y: 5.0 },
		)));
	}

	#[test]
	fn geometry_dispatch_geometry_collection_round_trips_each_inner() {
		let gc = Geometry::GeometryCollection(GeometryCollection(vec![
			Geometry::Point(Point::new(1.0, 2.0)),
			Geometry::Line(Line::new(Coord { x: 0.0, y: 0.0 }, Coord { x: 3.0, y: 4.0 })),
			Geometry::Triangle(Triangle::new(
				Coord { x: 0.0, y: 0.0 },
				Coord { x: 5.0, y: 0.0 },
				Coord { x: 0.0, y: 5.0 },
			)),
		]));
		let back = gc.clone().to_mercator().from_mercator();
		match (gc, back) {
			(Geometry::GeometryCollection(a), Geometry::GeometryCollection(b)) => {
				assert_eq!(a.0.len(), b.0.len());
				for (left, right) in a.0.iter().zip(b.0.iter()) {
					assert_eq!(std::mem::discriminant(left), std::mem::discriminant(right));
				}
			}
			_ => panic!("expected GeometryCollection on both sides"),
		}
	}
}
