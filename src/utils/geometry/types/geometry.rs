#![allow(dead_code)]

use std::{fmt::Debug, mem};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
	pub value: GeometryValue,
}

impl Geometry {
	fn new(value: GeometryValue) -> Geometry {
		Geometry { value }
	}
	pub fn new_multi_point(data: Vec<PointGeometry>) -> Geometry {
		Geometry::new(GeometryValue::MultiPoint(data))
	}
	pub fn new_multi_line_string(data: Vec<Vec<PointGeometry>>) -> Geometry {
		Geometry::new(GeometryValue::MultiLineString(data))
	}
	pub fn new_multi_polygon(data: Vec<Vec<Vec<PointGeometry>>>) -> Geometry {
		Geometry::new(GeometryValue::MultiPolygon(data))
	}
	pub fn is_multi(&self) -> bool {
		use GeometryValue::*;
		match self.value {
			Point(_) => false,
			LineString(_) => false,
			Polygon(_) => false,
			MultiPoint(_) => true,
			MultiLineString(_) => true,
			MultiPolygon(_) => true,
		}
	}
	pub fn convert_to_multi(&mut self) {
		use GeometryValue::*;
		let value = mem::take(&mut self.value);
		self.value = match value {
			Point(g) => MultiPoint(vec![g]),
			LineString(g) => MultiLineString(vec![g]),
			Polygon(g) => MultiPolygon(vec![g]),
			MultiPoint(g) => MultiPoint(g),
			MultiLineString(g) => MultiLineString(g),
			MultiPolygon(g) => MultiPolygon(g),
		};
	}
}

#[derive(Clone, Debug, PartialEq)]
pub enum GeometryValue {
	Point(PointGeometry),
	LineString(LineStringGeometry),
	Polygon(PolygonGeometry),

	MultiPoint(MultiPointGeometry),
	MultiLineString(MultiLineStringGeometry),
	MultiPolygon(MultiPolygonGeometry),
}

#[derive(Clone, PartialEq)]
pub struct PointGeometry {
	pub x: f64,
	pub y: f64,
}

impl Default for GeometryValue {
	fn default() -> Self {
		GeometryValue::LineString(vec![])
	}
}

pub type MultiPointGeometry = Vec<PointGeometry>;
pub type LineStringGeometry = Vec<PointGeometry>;
pub type RingGeometry = Vec<PointGeometry>;

pub type MultiLineStringGeometry = Vec<LineStringGeometry>;
pub type PolygonGeometry = Vec<RingGeometry>;

pub type MultiPolygonGeometry = Vec<PolygonGeometry>;

pub trait Ring {
	fn area(&self) -> f64;
}

impl Ring for RingGeometry {
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

impl Debug for PointGeometry {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entry(&self.x).entry(&self.y).finish()
	}
}
