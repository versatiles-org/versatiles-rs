use super::{PointGeometry, LineStringGeometry, PolygonGeometry, MultiPointGeometry, MultiLineStringGeometry, MultiPolygonGeometry, SingleGeometryTrait, GeometryTrait, CompositeGeometryTrait};
use anyhow::Result;
use std::fmt::Debug;
use versatiles_core::json::{JsonObject, JsonValue};

#[derive(Clone, PartialEq)]
pub enum Geometry {
	Point(PointGeometry),
	LineString(LineStringGeometry),
	Polygon(PolygonGeometry),
	MultiPoint(MultiPointGeometry),
	MultiLineString(MultiLineStringGeometry),
	MultiPolygon(MultiPolygonGeometry),
}

impl Geometry {
	pub fn new_point<T>(value: T) -> Self
	where
		PointGeometry: From<T>,
	{
		Self::Point(PointGeometry::from(value))
	}
	pub fn new_line_string<T>(value: T) -> Self
	where
		LineStringGeometry: From<T>,
	{
		Self::LineString(LineStringGeometry::from(value))
	}
	pub fn new_polygon<T>(value: T) -> Self
	where
		PolygonGeometry: From<T>,
	{
		Self::Polygon(PolygonGeometry::from(value))
	}
	pub fn new_multi_point<T>(value: T) -> Self
	where
		MultiPointGeometry: From<T>,
	{
		Self::MultiPoint(MultiPointGeometry::from(value))
	}
	pub fn new_multi_line_string<T>(value: T) -> Self
	where
		MultiLineStringGeometry: From<T>,
	{
		Self::MultiLineString(MultiLineStringGeometry::from(value))
	}
	pub fn new_multi_polygon<T>(value: T) -> Self
	where
		MultiPolygonGeometry: From<T>,
	{
		Self::MultiPolygon(MultiPolygonGeometry::from(value))
	}

	#[must_use] 
	pub fn get_type_name(&self) -> &str {
		match self {
			Geometry::Point(_) => "Point",
			Geometry::LineString(_) => "LineString",
			Geometry::Polygon(_) => "Polygon",
			Geometry::MultiPoint(_) => "MultiPoint",
			Geometry::MultiLineString(_) => "MultiLineString",
			Geometry::MultiPolygon(_) => "MultiPolygon",
		}
	}
	#[must_use] 
	pub fn into_multi(self) -> Self {
		match self {
			Geometry::Point(g) => Geometry::MultiPoint(g.into_multi()),
			Geometry::LineString(g) => Geometry::MultiLineString(g.into_multi()),
			Geometry::Polygon(g) => Geometry::MultiPolygon(g.into_multi()),
			Geometry::MultiPoint(_) => self,
			Geometry::MultiLineString(_) => self,
			Geometry::MultiPolygon(_) => self,
		}
	}

	#[must_use] 
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

	#[must_use] 
	pub fn to_json(&self) -> JsonObject {
		let mut obj = JsonObject::new();
		let (type_name, coordinates) = match self {
			Geometry::Point(g) => ("Point", g.to_coord_json()),
			Geometry::LineString(g) => ("LineString", g.to_coord_json()),
			Geometry::Polygon(g) => ("Polygon", g.to_coord_json()),
			Geometry::MultiPoint(g) => ("MultiPoint", g.to_coord_json()),
			Geometry::MultiLineString(g) => ("MultiLineString", g.to_coord_json()),
			Geometry::MultiPolygon(g) => ("MultiPolygon", g.to_coord_json()),
		};
		obj.set("type", JsonValue::from(type_name));
		obj.set("coordinates", coordinates);
		obj
	}
}

impl From<geo::MultiPolygon<f64>> for Geometry {
	fn from(geometry: geo::MultiPolygon<f64>) -> Self {
		Self::MultiPolygon(MultiPolygonGeometry(
			geometry.into_iter().map(PolygonGeometry::from).collect::<Vec<_>>(),
		))
	}
}

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
