#![allow(dead_code)]

use super::*;
use std::fmt::Debug;

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
	pub fn new_point<T: Convertible>(value: [T; 2]) -> Self {
		Self::Point(PointGeometry::from(value))
	}
	pub fn new_line_string<T: Convertible>(value: Vec<[T; 2]>) -> Self {
		Self::LineString(LineStringGeometry::from(value))
	}
	pub fn new_polygon<T: Convertible>(value: Vec<Vec<[T; 2]>>) -> Self {
		Self::Polygon(PolygonGeometry::from(value))
	}
	pub fn new_multi_point<T: Convertible>(value: Vec<[T; 2]>) -> Self {
		Self::MultiPoint(MultiPointGeometry::from(value))
	}
	pub fn new_multi_line_string<T: Convertible>(value: Vec<Vec<[T; 2]>>) -> Self {
		Self::MultiLineString(MultiLineStringGeometry::from(value))
	}
	pub fn new_multi_polygon<T: Convertible>(value: Vec<Vec<Vec<[T; 2]>>>) -> Self {
		Self::MultiPolygon(MultiPolygonGeometry::from(value))
	}

	fn get_type(&self) -> &str {
		match self {
			Geometry::Point(_) => "Point",
			Geometry::LineString(_) => "LineString",
			Geometry::Polygon(_) => "Polygon",
			Geometry::MultiPoint(_) => "MultiPoint",
			Geometry::MultiLineString(_) => "MultiLineString",
			Geometry::MultiPolygon(_) => "MultiPolygon",
		}
	}
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
