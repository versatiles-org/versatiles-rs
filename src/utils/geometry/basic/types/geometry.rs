#![allow(dead_code)]

use std::fmt::Debug;

pub type LineStringGeometry = Vec<PointGeometry>;
pub type MultiLineStringGeometry = Vec<LineStringGeometry>;
pub type MultiPointGeometry = Vec<PointGeometry>;
pub type MultiPolygonGeometry = Vec<PolygonGeometry>;
pub type PolygonGeometry = Vec<RingGeometry>;
pub type RingGeometry = Vec<PointGeometry>;

#[derive(Clone, PartialEq)]
pub enum Geometry {
	Point(PointGeometry),
	LineString(LineStringGeometry),
	Polygon(PolygonGeometry),
	MultiPoint(MultiPointGeometry),
	MultiLineString(MultiLineStringGeometry),
	MultiPolygon(MultiPolygonGeometry),
}

use Geometry::*;

use super::PointGeometry;

impl Geometry {
	pub fn new_point(geometry: PointGeometry) -> Self {
		Self::Point(geometry)
	}
	pub fn new_line_string(geometry: LineStringGeometry) -> Self {
		Self::LineString(geometry)
	}
	pub fn new_polygon(geometry: PolygonGeometry) -> Self {
		Self::Polygon(geometry)
	}
	pub fn new_multi_point(geometry: MultiPointGeometry) -> Self {
		Self::MultiPoint(geometry)
	}
	pub fn new_multi_line_string(geometry: MultiLineStringGeometry) -> Self {
		Self::MultiLineString(geometry)
	}
	pub fn new_multi_polygon(geometry: MultiPolygonGeometry) -> Self {
		Self::MultiPolygon(geometry)
	}
	fn get_type(&self) -> &str {
		match self {
			Point(_) => "Point",
			LineString(_) => "LineString",
			Polygon(_) => "Polygon",
			MultiPoint(_) => "MultiPoint",
			MultiLineString(_) => "MultiLineString",
			MultiPolygon(_) => "MultiPolygon",
		}
	}
	pub fn into_multi(self) -> Self {
		match self {
			Point(g) => MultiPoint(vec![g]),
			LineString(g) => MultiLineString(vec![g]),
			Polygon(g) => MultiPolygon(vec![g]),
			MultiPoint(g) => MultiPoint(g),
			MultiLineString(g) => MultiLineString(g),
			MultiPolygon(g) => MultiPolygon(g),
		}
	}
}

impl Debug for Geometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let (type_name, inner): (&str, &dyn Debug) = match self {
			Point(g) => ("Point", g),
			LineString(g) => ("LineString", g),
			Polygon(g) => ("Polygon", g),
			MultiPoint(g) => ("MultiPoint", g),
			MultiLineString(g) => ("MultiLineString", g),
			MultiPolygon(g) => ("MultiPolygon", g),
		};
		f.debug_tuple(type_name).field(inner).finish()
	}
}

pub trait AreaTrait {
	fn area(&self) -> f64;
}

impl AreaTrait for RingGeometry {
	fn area(&self) -> f64 {
		let mut sum = 0f64;
		let mut p2 = &self[self.len() - 1];
		for p1 in self.iter() {
			sum += (p2.x - p1.x) * (p1.y + p2.y);
			p2 = p1
		}
		sum
	}
}
