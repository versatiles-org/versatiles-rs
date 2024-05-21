#![allow(dead_code)]

use super::PointGeometry;
use std::fmt::Debug;
use Geometry::*;

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

	#[cfg(test)]
	pub fn new_example() -> Self {
		Self::new_multi_polygon(parse3(
			vec![
				vec![
					vec![[0.0, 0.0], [5.0, 0.0], [2.5, 4.0], [0.0, 0.0]],
					vec![[2.0, 1.0], [2.5, 2.0], [3.0, 1.0], [2.0, 1.0]],
				],
				vec![
					vec![[6.0, 0.0], [9.0, 0.0], [9.0, 4.0], [6.0, 4.0], [6.0, 0.0]],
					vec![[7.0, 1.0], [7.0, 3.0], [8.0, 3.0], [8.0, 1.0], [7.0, 1.0]],
				],
			]
			.into(),
		))
	}
}

fn parse3<I>(value: Vec<Vec<Vec<I>>>) -> MultiPolygonGeometry
where
	PointGeometry: From<I>,
{
	value
		.into_iter()
		.map(|a| {
			a.into_iter()
				.map(|b| b.into_iter().map(|c| PointGeometry::from(c)).collect())
				.collect()
		})
		.collect()
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

#[cfg(test)]
mod tests {
	//use super::Geometry;

	#[test]
	fn test_area() {
		//let multipolygon = Geometry::new_example();
	}
}
