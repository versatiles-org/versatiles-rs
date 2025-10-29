use crate::geo::CompositeGeometryTrait;

use super::{
	GeometryTrait, LineStringGeometry, MultiLineStringGeometry, MultiPointGeometry, MultiPolygonGeometry, PointGeometry,
	PolygonGeometry, SingleGeometryTrait,
};
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

	pub fn is_single_geometry(&self) -> bool {
		matches!(
			self,
			Geometry::Point(_) | Geometry::LineString(_) | Geometry::Polygon(_)
		)
	}

	pub fn is_multi_geometry(&self) -> bool {
		matches!(
			self,
			Geometry::MultiPoint(_) | Geometry::MultiLineString(_) | Geometry::MultiPolygon(_)
		)
	}

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

	pub fn into_multi_geometry(self) -> Self {
		match self {
			Geometry::Point(g) => Geometry::MultiPoint(g.into_multi()),
			Geometry::LineString(g) => Geometry::MultiLineString(g.into_multi()),
			Geometry::Polygon(g) => Geometry::MultiPolygon(g.into_multi()),
			Geometry::MultiPoint(_) => self,
			Geometry::MultiLineString(_) => self,
			Geometry::MultiPolygon(_) => self,
		}
	}

	pub fn into_single_geometry(self) -> Self {
		match self {
			Geometry::Point(_) => self,
			Geometry::LineString(_) => self,
			Geometry::Polygon(_) => self,
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
