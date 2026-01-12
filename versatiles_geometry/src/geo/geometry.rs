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

#[cfg(test)]
mod tests {
	use super::*;

	// ========================================================================
	// Constructor tests
	// ========================================================================

	#[test]
	fn test_new_point() {
		let geom = Geometry::new_point([1.0, 2.0]);
		assert!(matches!(geom, Geometry::Point(_)));
		assert_eq!(geom.type_name(), "Point");
	}

	#[test]
	fn test_new_line_string() {
		let geom = Geometry::new_line_string(vec![[0.0, 0.0], [1.0, 1.0]]);
		assert!(matches!(geom, Geometry::LineString(_)));
		assert_eq!(geom.type_name(), "LineString");
	}

	#[test]
	fn test_new_polygon() {
		let geom = Geometry::new_polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]);
		assert!(matches!(geom, Geometry::Polygon(_)));
		assert_eq!(geom.type_name(), "Polygon");
	}

	#[test]
	fn test_new_multi_point() {
		let geom = Geometry::new_multi_point(vec![[0.0, 0.0], [1.0, 1.0]]);
		assert!(matches!(geom, Geometry::MultiPoint(_)));
		assert_eq!(geom.type_name(), "MultiPoint");
	}

	#[test]
	fn test_new_multi_line_string() {
		let geom = Geometry::new_multi_line_string(vec![vec![[0.0, 0.0], [1.0, 1.0]], vec![[2.0, 2.0], [3.0, 3.0]]]);
		assert!(matches!(geom, Geometry::MultiLineString(_)));
		assert_eq!(geom.type_name(), "MultiLineString");
	}

	#[test]
	fn test_new_multi_polygon() {
		let geom = Geometry::new_multi_polygon(vec![vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]]);
		assert!(matches!(geom, Geometry::MultiPolygon(_)));
		assert_eq!(geom.type_name(), "MultiPolygon");
	}

	// ========================================================================
	// Type check tests
	// ========================================================================

	#[test]
	fn test_is_single_geometry() {
		assert!(Geometry::new_point([0.0, 0.0]).is_single_geometry());
		assert!(Geometry::new_line_string(vec![[0.0, 0.0], [1.0, 1.0]]).is_single_geometry());
		assert!(Geometry::new_polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]).is_single_geometry());

		assert!(!Geometry::new_multi_point(vec![[0.0, 0.0]]).is_single_geometry());
		assert!(!Geometry::new_multi_line_string(vec![vec![[0.0, 0.0], [1.0, 1.0]]]).is_single_geometry());
		assert!(
			!Geometry::new_multi_polygon(vec![vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]])
				.is_single_geometry()
		);
	}

	#[test]
	fn test_is_multi_geometry() {
		assert!(!Geometry::new_point([0.0, 0.0]).is_multi_geometry());
		assert!(!Geometry::new_line_string(vec![[0.0, 0.0], [1.0, 1.0]]).is_multi_geometry());
		assert!(!Geometry::new_polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]).is_multi_geometry());

		assert!(Geometry::new_multi_point(vec![[0.0, 0.0]]).is_multi_geometry());
		assert!(Geometry::new_multi_line_string(vec![vec![[0.0, 0.0], [1.0, 1.0]]]).is_multi_geometry());
		assert!(
			Geometry::new_multi_polygon(vec![vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]])
				.is_multi_geometry()
		);
	}

	// ========================================================================
	// type_name tests
	// ========================================================================

	#[test]
	fn test_type_name_all_variants() {
		assert_eq!(Geometry::new_point([0.0, 0.0]).type_name(), "Point");
		assert_eq!(
			Geometry::new_line_string(vec![[0.0, 0.0], [1.0, 1.0]]).type_name(),
			"LineString"
		);
		assert_eq!(
			Geometry::new_polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]).type_name(),
			"Polygon"
		);
		assert_eq!(Geometry::new_multi_point(vec![[0.0, 0.0]]).type_name(), "MultiPoint");
		assert_eq!(
			Geometry::new_multi_line_string(vec![vec![[0.0, 0.0], [1.0, 1.0]]]).type_name(),
			"MultiLineString"
		);
		assert_eq!(
			Geometry::new_multi_polygon(vec![vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]]).type_name(),
			"MultiPolygon"
		);
	}

	// ========================================================================
	// into_multi_geometry tests
	// ========================================================================

	#[test]
	fn test_into_multi_geometry_from_point() {
		let point = Geometry::new_point([1.0, 2.0]);
		let multi = point.into_multi_geometry();
		assert!(matches!(multi, Geometry::MultiPoint(_)));
		assert_eq!(multi.type_name(), "MultiPoint");
	}

	#[test]
	fn test_into_multi_geometry_from_line_string() {
		let line = Geometry::new_line_string(vec![[0.0, 0.0], [1.0, 1.0]]);
		let multi = line.into_multi_geometry();
		assert!(matches!(multi, Geometry::MultiLineString(_)));
		assert_eq!(multi.type_name(), "MultiLineString");
	}

	#[test]
	fn test_into_multi_geometry_from_polygon() {
		let polygon = Geometry::new_polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]);
		let multi = polygon.into_multi_geometry();
		assert!(matches!(multi, Geometry::MultiPolygon(_)));
		assert_eq!(multi.type_name(), "MultiPolygon");
	}

	#[test]
	fn test_into_multi_geometry_multi_unchanged() {
		let multi_point = Geometry::new_multi_point(vec![[0.0, 0.0], [1.0, 1.0]]);
		let result = multi_point.clone().into_multi_geometry();
		assert_eq!(result, multi_point);

		let multi_line = Geometry::new_multi_line_string(vec![vec![[0.0, 0.0], [1.0, 1.0]]]);
		let result = multi_line.clone().into_multi_geometry();
		assert_eq!(result, multi_line);

		let multi_poly = Geometry::new_multi_polygon(vec![vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]]);
		let result = multi_poly.clone().into_multi_geometry();
		assert_eq!(result, multi_poly);
	}

	// ========================================================================
	// into_single_geometry tests
	// ========================================================================

	#[test]
	fn test_into_single_geometry_single_unchanged() {
		let point = Geometry::new_point([1.0, 2.0]);
		let result = point.clone().into_single_geometry();
		assert_eq!(result, point);

		let line = Geometry::new_line_string(vec![[0.0, 0.0], [1.0, 1.0]]);
		let result = line.clone().into_single_geometry();
		assert_eq!(result, line);

		let polygon = Geometry::new_polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]);
		let result = polygon.clone().into_single_geometry();
		assert_eq!(result, polygon);
	}

	#[test]
	fn test_into_single_geometry_multi_point_one_element() {
		let multi = Geometry::new_multi_point(vec![[1.0, 2.0]]);
		let single = multi.into_single_geometry();
		assert!(matches!(single, Geometry::Point(_)));
		assert_eq!(single.type_name(), "Point");
	}

	#[test]
	fn test_into_single_geometry_multi_point_multiple_elements() {
		let multi = Geometry::new_multi_point(vec![[1.0, 2.0], [3.0, 4.0]]);
		let result = multi.clone().into_single_geometry();
		assert!(matches!(result, Geometry::MultiPoint(_)));
	}

	#[test]
	fn test_into_single_geometry_multi_line_string_one_element() {
		let multi = Geometry::new_multi_line_string(vec![vec![[0.0, 0.0], [1.0, 1.0]]]);
		let single = multi.into_single_geometry();
		assert!(matches!(single, Geometry::LineString(_)));
		assert_eq!(single.type_name(), "LineString");
	}

	#[test]
	fn test_into_single_geometry_multi_line_string_multiple_elements() {
		let multi = Geometry::new_multi_line_string(vec![vec![[0.0, 0.0], [1.0, 1.0]], vec![[2.0, 2.0], [3.0, 3.0]]]);
		let result = multi.clone().into_single_geometry();
		assert!(matches!(result, Geometry::MultiLineString(_)));
	}

	#[test]
	fn test_into_single_geometry_multi_polygon_one_element() {
		let multi = Geometry::new_multi_polygon(vec![vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]]);
		let single = multi.into_single_geometry();
		assert!(matches!(single, Geometry::Polygon(_)));
		assert_eq!(single.type_name(), "Polygon");
	}

	#[test]
	fn test_into_single_geometry_multi_polygon_multiple_elements() {
		let multi = Geometry::new_multi_polygon(vec![
			vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
			vec![vec![[2.0, 2.0], [3.0, 2.0], [3.0, 3.0], [2.0, 2.0]]],
		]);
		let result = multi.clone().into_single_geometry();
		assert!(matches!(result, Geometry::MultiPolygon(_)));
	}

	// ========================================================================
	// verify tests
	// ========================================================================

	#[test]
	fn test_verify_valid_geometries() {
		assert!(Geometry::new_point([1.0, 2.0]).verify().is_ok());
		assert!(Geometry::new_line_string(vec![[0.0, 0.0], [1.0, 1.0]]).verify().is_ok());
		assert!(
			Geometry::new_polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]])
				.verify()
				.is_ok()
		);
		assert!(Geometry::new_multi_point(vec![[0.0, 0.0], [1.0, 1.0]]).verify().is_ok());
		assert!(
			Geometry::new_multi_line_string(vec![vec![[0.0, 0.0], [1.0, 1.0]]])
				.verify()
				.is_ok()
		);
		assert!(
			Geometry::new_multi_polygon(vec![vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]])
				.verify()
				.is_ok()
		);
	}

	#[test]
	fn test_verify_example_geometry() {
		let geom = Geometry::new_example();
		assert!(geom.verify().is_ok());
	}

	// ========================================================================
	// to_json tests
	// ========================================================================

	#[test]
	fn test_to_json_point() {
		let geom = Geometry::new_point([1.5, 2.5]);
		let json = geom.to_json(None);
		assert_eq!(json.get("type").unwrap().as_str().unwrap(), "Point");
		assert!(json.get("coordinates").is_some());
	}

	#[test]
	fn test_to_json_line_string() {
		let geom = Geometry::new_line_string(vec![[0.0, 0.0], [1.0, 1.0]]);
		let json = geom.to_json(None);
		assert_eq!(json.get("type").unwrap().as_str().unwrap(), "LineString");
		assert!(json.get("coordinates").is_some());
	}

	#[test]
	fn test_to_json_polygon() {
		let geom = Geometry::new_polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]);
		let json = geom.to_json(None);
		assert_eq!(json.get("type").unwrap().as_str().unwrap(), "Polygon");
		assert!(json.get("coordinates").is_some());
	}

	#[test]
	fn test_to_json_multi_point() {
		let geom = Geometry::new_multi_point(vec![[0.0, 0.0], [1.0, 1.0]]);
		let json = geom.to_json(None);
		assert_eq!(json.get("type").unwrap().as_str().unwrap(), "MultiPoint");
		assert!(json.get("coordinates").is_some());
	}

	#[test]
	fn test_to_json_multi_line_string() {
		let geom = Geometry::new_multi_line_string(vec![vec![[0.0, 0.0], [1.0, 1.0]]]);
		let json = geom.to_json(None);
		assert_eq!(json.get("type").unwrap().as_str().unwrap(), "MultiLineString");
		assert!(json.get("coordinates").is_some());
	}

	#[test]
	fn test_to_json_multi_polygon() {
		let geom = Geometry::new_multi_polygon(vec![vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]]);
		let json = geom.to_json(None);
		assert_eq!(json.get("type").unwrap().as_str().unwrap(), "MultiPolygon");
		assert!(json.get("coordinates").is_some());
	}

	#[test]
	fn test_to_json_with_precision() {
		let geom = Geometry::new_point([1.123456789, 2.987654321]);
		let json = geom.to_json(Some(2));
		let coords = json.get("coordinates").unwrap();
		let coords_str = format!("{coords:?}");
		// With precision 2, coordinates should be rounded
		assert!(coords_str.contains("1.12") || coords_str.contains("2.99"));
	}

	// ========================================================================
	// From<geo::MultiPolygon<f64>> tests
	// ========================================================================

	#[test]
	fn test_from_geo_multi_polygon() {
		use geo::{MultiPolygon, Polygon, coord};

		let exterior = vec![
			coord! { x: 0.0, y: 0.0 },
			coord! { x: 1.0, y: 0.0 },
			coord! { x: 1.0, y: 1.0 },
			coord! { x: 0.0, y: 0.0 },
		];
		let polygon = Polygon::new(exterior.into(), vec![]);
		let multi_polygon = MultiPolygon::new(vec![polygon]);

		let geom: Geometry = multi_polygon.into();
		assert!(matches!(geom, Geometry::MultiPolygon(_)));
		assert_eq!(geom.type_name(), "MultiPolygon");
	}

	// ========================================================================
	// Debug tests
	// ========================================================================

	#[test]
	fn test_debug_point() {
		let geom = Geometry::new_point([1.0, 2.0]);
		let debug_str = format!("{geom:?}");
		assert!(debug_str.starts_with("Point("));
	}

	#[test]
	fn test_debug_line_string() {
		let geom = Geometry::new_line_string(vec![[0.0, 0.0], [1.0, 1.0]]);
		let debug_str = format!("{geom:?}");
		assert!(debug_str.starts_with("LineString("));
	}

	#[test]
	fn test_debug_polygon() {
		let geom = Geometry::new_polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]);
		let debug_str = format!("{geom:?}");
		assert!(debug_str.starts_with("Polygon("));
	}

	#[test]
	fn test_debug_multi_point() {
		let geom = Geometry::new_multi_point(vec![[0.0, 0.0]]);
		let debug_str = format!("{geom:?}");
		assert!(debug_str.starts_with("MultiPoint("));
	}

	#[test]
	fn test_debug_multi_line_string() {
		let geom = Geometry::new_multi_line_string(vec![vec![[0.0, 0.0], [1.0, 1.0]]]);
		let debug_str = format!("{geom:?}");
		assert!(debug_str.starts_with("MultiLineString("));
	}

	#[test]
	fn test_debug_multi_polygon() {
		let geom = Geometry::new_multi_polygon(vec![vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]]);
		let debug_str = format!("{geom:?}");
		assert!(debug_str.starts_with("MultiPolygon("));
	}

	// ========================================================================
	// Clone and PartialEq tests
	// ========================================================================

	#[test]
	fn test_clone() {
		let geom = Geometry::new_point([1.0, 2.0]);
		let cloned = geom.clone();
		assert_eq!(geom, cloned);
	}

	#[test]
	fn test_partial_eq() {
		let geom1 = Geometry::new_point([1.0, 2.0]);
		let geom2 = Geometry::new_point([1.0, 2.0]);
		let geom3 = Geometry::new_point([3.0, 4.0]);

		assert_eq!(geom1, geom2);
		assert_ne!(geom1, geom3);
	}

	#[test]
	fn test_partial_eq_different_types() {
		let point = Geometry::new_point([1.0, 2.0]);
		let multi_point = Geometry::new_multi_point(vec![[1.0, 2.0]]);

		assert_ne!(point, multi_point);
	}

	// ========================================================================
	// new_example test
	// ========================================================================

	#[test]
	fn test_new_example() {
		let geom = Geometry::new_example();
		assert!(matches!(geom, Geometry::MultiPolygon(_)));
		assert_eq!(geom.type_name(), "MultiPolygon");
		assert!(geom.verify().is_ok());
	}

	// ========================================================================
	// Roundtrip tests
	// ========================================================================

	#[test]
	fn test_roundtrip_single_to_multi_to_single() {
		let point = Geometry::new_point([1.0, 2.0]);
		let multi = point.clone().into_multi_geometry();
		let single_again = multi.into_single_geometry();
		assert_eq!(point, single_again);

		let line = Geometry::new_line_string(vec![[0.0, 0.0], [1.0, 1.0]]);
		let multi = line.clone().into_multi_geometry();
		let single_again = multi.into_single_geometry();
		assert_eq!(line, single_again);

		let polygon = Geometry::new_polygon(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]);
		let multi = polygon.clone().into_multi_geometry();
		let single_again = multi.into_single_geometry();
		assert_eq!(polygon, single_again);
	}
}
