#![allow(dead_code)]

use std::fmt::Debug;

#[derive(Clone, PartialEq)]
pub struct PointGeometry {
	pub x: f64,
	pub y: f64,
}

pub type MultiPointGeometry = Vec<PointGeometry>;
pub type LinestringGeometry = Vec<PointGeometry>;
pub type RingGeometry = Vec<PointGeometry>;

pub type MultiLinestringGeometry = Vec<LinestringGeometry>;
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

pub enum Geometry {
	Point(PointGeometry),
	Linestring(LinestringGeometry),
	Polygon(PolygonGeometry),

	MultiPoint(MultiPointGeometry),
	MultiLinestring(MultiLinestringGeometry),
	MultiPolygon(MultiPolygonGeometry),
}

pub enum MultiGeometry {
	Point(MultiPointGeometry),
	Linestring(MultiLinestringGeometry),
	Polygon(MultiPolygonGeometry),
}
