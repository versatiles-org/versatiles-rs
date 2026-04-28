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

	#[test]
	fn coord_to_mercator_origin() {
		let m = coord_to_mercator(Coord { x: 0.0, y: 0.0 });
		assert_relative_eq!(m.x, 0.0, epsilon = 1e-6);
		assert_relative_eq!(m.y, 0.0, epsilon = 1e-6);
	}

	#[test]
	fn mercator_roundtrip() {
		let original = Coord { x: 10.0, y: 50.0 };
		let projected = coord_to_mercator(original);
		let back = coord_from_mercator(projected);
		assert_relative_eq!(back.x, original.x, epsilon = 1e-6);
		assert_relative_eq!(back.y, original.y, epsilon = 1e-6);
	}

	#[test]
	fn latitude_clamping_avoids_infinity() {
		let m = coord_to_mercator(Coord { x: 0.0, y: 90.0 });
		assert!(m.y.is_finite());
	}

	#[test]
	fn coord_trait_impl() {
		let c = Coord { x: 1.0, y: 2.0 };
		let projected = c.to_mercator();
		assert_relative_eq!(projected, coord_to_mercator(c), epsilon = 1e-12);
	}

	#[test]
	fn point_trait_impl() {
		let p = Point::new(13.4, 52.5);
		let projected = p.to_mercator();
		assert!(projected.x() > 0.0 && projected.y() > 0.0);
	}

	#[test]
	fn polygon_trait_impl() {
		let exterior = LineString::from(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]);
		let p = Polygon::new(exterior, vec![]);
		let projected = p.clone().to_mercator();
		// First vertex stays at origin under mercator(0,0).
		assert_relative_eq!(projected.exterior().0[0].x, 0.0, epsilon = 1e-6);
	}

	#[test]
	fn geometry_dispatch_round_trip() {
		let g: Geometry<f64> = Point::new(1.5, -2.5).into();
		let projected = g.clone().to_mercator();
		let back = projected.from_mercator();
		match (g, back) {
			(Geometry::Point(a), Geometry::Point(b)) => {
				assert_relative_eq!(a.x(), b.x(), epsilon = 1e-6);
				assert_relative_eq!(a.y(), b.y(), epsilon = 1e-6);
			}
			_ => panic!("expected Point variants"),
		}
	}
}
