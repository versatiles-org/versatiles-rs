#![allow(dead_code)]

use super::*;

#[derive(Debug, PartialEq)]
pub struct Feature<T> {
	pub id: Option<u64>,
	pub geometry: T,
	pub attributes: GeoAttributes,
}

pub type PointFeature = Feature<PointGeometry>;
pub type MultiPointFeature = Feature<MultiPointGeometry>;

pub type LinestringFeature = Feature<LinestringGeometry>;
pub type MultiLinestringFeature = Feature<MultiLinestringGeometry>;

pub type PolygonFeature = Feature<PolygonGeometry>;
pub type MultiPolygonFeature = Feature<MultiPolygonGeometry>;
