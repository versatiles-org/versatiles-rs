//! Core geometry enum and helpers.
//!
//! This module defines the `Geometry` enum used throughout `versatiles_geometry` to
//! represent GeoJSON-like geometry types: `Point`, `LineString`, `Polygon` and their
//! multi-geometry counterparts. It provides constructors, conversions between single
//! and multi variants, validation, and serialization to GeoJSON-compatible JSON.

use crate::geo::CompositeGeometryTrait;

use super::{
	GeometryTrait, LineStringGeometry, MultiLineStringGeometry, MultiPointGeometry, MultiPolygonGeometry, PointGeometry,
	PolygonGeometry, SingleGeometryTrait,
};
use anyhow::Result;
use std::fmt::Debug;
use versatiles_core::json::{JsonObject, JsonValue};

/// A GeoJSON-like sum type covering point/line/polygon and their multi variants.
///
/// Each variant wraps the corresponding concrete geometry type. The enum offers
/// convenience constructors (e.g., `new_point`, `new_multi_polygon`), validation via
/// `verify`, and JSON export via `to_json` that matches the GeoJSON `type` and
/// `coordinates` structure.
#[derive(Clone, PartialEq)]
pub enum Geometry {
	/// A single Point geometry.
	Point(PointGeometry),
	/// A single LineString geometry.
	LineString(LineStringGeometry),
	/// A single Polygon geometry.
	Polygon(PolygonGeometry),
	/// A multi-Point geometry.
	MultiPoint(MultiPointGeometry),
	/// A multi-LineString geometry.
	MultiLineString(MultiLineStringGeometry),
	/// A multi-Polygon geometry.
	MultiPolygon(MultiPolygonGeometry),
}

impl Geometry {
	/// Constructs a `Geometry::Point` from any value convertible into `PointGeometry`.
	/// Useful for ergonomic creation from tuples, arrays, or existing `Coordinates`.
	pub fn new_point<T>(value: T) -> Self
	where
		PointGeometry: From<T>,
	{
		Self::Point(PointGeometry::from(value))
	}
	/// Constructs a `Geometry::LineString` from any value convertible into `LineStringGeometry`.
	pub fn new_line_string<T>(value: T) -> Self
	where
		LineStringGeometry: From<T>,
	{
		Self::LineString(LineStringGeometry::from(value))
	}
	/// Constructs a `Geometry::Polygon` from any value convertible into `PolygonGeometry`.
	pub fn new_polygon<T>(value: T) -> Self
	where
		PolygonGeometry: From<T>,
	{
		Self::Polygon(PolygonGeometry::from(value))
	}
	/// Constructs a `Geometry::MultiPoint` from any value convertible into `MultiPointGeometry`.
	pub fn new_multi_point<T>(value: T) -> Self
	where
		MultiPointGeometry: From<T>,
	{
		Self::MultiPoint(MultiPointGeometry::from(value))
	}
	/// Constructs a `Geometry::MultiLineString` from any value convertible into `MultiLineStringGeometry`.
	pub fn new_multi_line_string<T>(value: T) -> Self
	where
		MultiLineStringGeometry: From<T>,
	{
		Self::MultiLineString(MultiLineStringGeometry::from(value))
	}
	/// Constructs a `Geometry::MultiPolygon` from any value convertible into `MultiPolygonGeometry`.
	pub fn new_multi_polygon<T>(value: T) -> Self
	where
		MultiPolygonGeometry: From<T>,
	{
		Self::MultiPolygon(MultiPolygonGeometry::from(value))
	}

	/// Returns `true` if this is a single-geometry variant (`Point`, `LineString`, or `Polygon`).
	pub fn is_single_geometry(&self) -> bool {
		matches!(
			self,
			Geometry::Point(_) | Geometry::LineString(_) | Geometry::Polygon(_)
		)
	}

	/// Returns `true` if this is a multi-geometry variant (`MultiPoint`, `MultiLineString`, or `MultiPolygon`).
	pub fn is_multi_geometry(&self) -> bool {
		matches!(
			self,
			Geometry::MultiPoint(_) | Geometry::MultiLineString(_) | Geometry::MultiPolygon(_)
		)
	}

	/// Returns the GeoJSON-like type name for this geometry (e.g., "Polygon", "MultiPoint").
	pub fn type_name(&self) -> &str {
		match self {
			Geometry::Point(_) => "Point",
			Geometry::LineString(_) => "LineString",
			Geometry::Polygon(_) => "Polygon",
			Geometry::MultiPoint(_) => "MultiPoint",
			Geometry::MultiLineString(_) => "MultiLineString",
			Geometry::MultiPolygon(_) => "MultiPolygon",
		}
	}

	/// Converts single-geometry variants into their corresponding multi-geometry variants.
	///
	/// `Point`→`MultiPoint`, `LineString`→`MultiLineString`, `Polygon`→`MultiPolygon`.
	/// Multi variants are returned unchanged.
	pub fn into_multi_geometry(self) -> Self {
		match self {
			Geometry::Point(g) => Geometry::MultiPoint(g.into_multi()),
			Geometry::LineString(g) => Geometry::MultiLineString(g.into_multi()),
			Geometry::Polygon(g) => Geometry::MultiPolygon(g.into_multi()),
			Geometry::MultiPoint(_) | Geometry::MultiLineString(_) | Geometry::MultiPolygon(_) => self,
		}
	}

	/// Converts multi-geometry variants into single variants *when possible*.
	///
	/// If the multi geometry contains exactly one element, it is unwrapped to the
	/// corresponding single variant; otherwise the original multi geometry is returned.
	pub fn into_single_geometry(self) -> Self {
		match self {
			Geometry::Point(_) | Geometry::LineString(_) | Geometry::Polygon(_) => self,
			Geometry::MultiPoint(mut g) => {
				if g.len() == 1 {
					Geometry::Point(g.pop().unwrap())
				} else {
					Geometry::MultiPoint(g)
				}
			}
			Geometry::MultiLineString(mut g) => {
				if g.len() == 1 {
					Geometry::LineString(g.pop().unwrap())
				} else {
					Geometry::MultiLineString(g)
				}
			}
			Geometry::MultiPolygon(mut g) => {
				if g.len() == 1 {
					Geometry::Polygon(g.pop().unwrap())
				} else {
					Geometry::MultiPolygon(g)
				}
			}
		}
	}

	/// Test helper: returns a deterministic example `MultiPolygon` geometry with holes.
	#[cfg(any(test, feature = "test"))]
	pub fn new_example() -> Self {
		Self::new_multi_polygon(vec![
			vec![
				vec![[0.0, 0.0], [5.0, 0.0], [2.5, 4.0], [0.0, 0.0]],
				vec![[2.0, 1.0], [2.5, 2.0], [3.0, 1.0], [2.0, 1.0]],
			],
			vec![
				vec![[6.0, 0.0], [9.0, 0.0], [9.0, 4.0], [6.0, 4.0], [6.0, 0.0]],
				vec![[7.0, 1.0], [7.0, 3.0], [8.0, 3.0], [8.0, 1.0], [7.0, 1.0]],
			],
		])
	}

	/// Verifies the internal geometry by delegating to the inner type's `verify()`.
	/// Returns an error if the geometry is invalid.
	pub fn verify(&self) -> Result<()> {
		match self {
			Geometry::Point(g) => g.verify(),
			Geometry::LineString(g) => g.verify(),
			Geometry::Polygon(g) => g.verify(),
			Geometry::MultiPoint(g) => g.verify(),
			Geometry::MultiLineString(g) => g.verify(),
			Geometry::MultiPolygon(g) => g.verify(),
		}
	}

	/// Serializes the geometry into a GeoJSON-compatible object with `type` and `coordinates`.
	/// Coordinates may be rounded to `precision` fractional digits if provided.
	pub fn to_json(&self, precision: Option<u8>) -> JsonObject {
		let mut obj = JsonObject::new();
		let type_name = self.type_name();
		let coordinates = match self {
			Geometry::Point(g) => g.to_coord_json(precision),
			Geometry::LineString(g) => g.to_coord_json(precision),
			Geometry::Polygon(g) => g.to_coord_json(precision),
			Geometry::MultiPoint(g) => g.to_coord_json(precision),
			Geometry::MultiLineString(g) => g.to_coord_json(precision),
			Geometry::MultiPolygon(g) => g.to_coord_json(precision),
		};
		obj.set("type", JsonValue::from(type_name));
		obj.set("coordinates", coordinates);
		obj
	}
}

/// Converts a `geo::MultiPolygon<f64>` into a `Geometry::MultiPolygon`, converting each
/// ringed polygon into the crate's `PolygonGeometry` representation.
impl From<geo::MultiPolygon<f64>> for Geometry {
	fn from(geometry: geo::MultiPolygon<f64>) -> Self {
		Self::MultiPolygon(MultiPolygonGeometry(
			geometry.into_iter().map(PolygonGeometry::from).collect::<Vec<_>>(),
		))
	}
}

/// Formats the enum as `Variant(inner)` for developer-friendly debugging.
impl Debug for Geometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let (type_name, inner): (&str, &dyn Debug) = match self {
			Geometry::Point(g) => ("Point", g),
			Geometry::LineString(g) => ("LineString", g),
			Geometry::Polygon(g) => ("Polygon", g),
			Geometry::MultiPoint(g) => ("MultiPoint", g),
			Geometry::MultiLineString(g) => ("MultiLineString", g),
			Geometry::MultiPolygon(g) => ("MultiPolygon", g),
		};
		f.debug_tuple(type_name).field(inner).finish()
	}
}
