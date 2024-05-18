#![allow(dead_code)]

use std::fmt::Debug;

use super::*;

#[derive(Debug, PartialEq)]
pub struct AbstractFeature<T> {
	pub id: Option<u64>,
	pub geometry: T,
	pub properties: GeoProperties,
}

pub type PointFeature = AbstractFeature<PointGeometry>;
pub type MultiPointFeature = AbstractFeature<MultiPointGeometry>;

pub type LinestringFeature = AbstractFeature<LinestringGeometry>;
pub type MultiLinestringFeature = AbstractFeature<MultiLinestringGeometry>;

pub type PolygonFeature = AbstractFeature<PolygonGeometry>;
pub type MultiPolygonFeature = AbstractFeature<MultiPolygonGeometry>;

#[derive(PartialEq)]
pub enum Feature {
	Point(PointFeature),
	Linestring(LinestringFeature),
	Polygon(PolygonFeature),

	MultiPoint(MultiPointFeature),
	MultiLinestring(MultiLinestringFeature),
	MultiPolygon(MultiPolygonFeature),
}

impl Debug for Feature {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Point(a) => a.fmt(f),
			Self::Linestring(a) => a.fmt(f),
			Self::Polygon(a) => a.fmt(f),
			Self::MultiPoint(a) => a.fmt(f),
			Self::MultiLinestring(a) => a.fmt(f),
			Self::MultiPolygon(a) => a.fmt(f),
		}
	}
}

pub enum MultiFeature {
	Point(MultiPointFeature),
	Linestring(MultiLinestringFeature),
	Polygon(MultiPolygonFeature),
}

impl MultiFeature {
	pub fn new(id: Option<u64>, geometry: MultiGeometry, properties: GeoProperties) -> Self {
		use MultiFeature as F;
		use MultiGeometry as G;
		match geometry {
			G::Point(geometry) => F::Point(MultiPointFeature {
				id,
				geometry,
				properties,
			}),
			G::Linestring(geometry) => F::Linestring(MultiLinestringFeature {
				id,
				geometry,
				properties,
			}),
			G::Polygon(geometry) => F::Polygon(MultiPolygonFeature {
				id,
				geometry,
				properties,
			}),
		}
	}
	pub fn into_feature(self) -> Feature {
		match self {
			MultiFeature::Point(f) => Feature::MultiPoint(f),
			MultiFeature::Linestring(f) => Feature::MultiLinestring(f),
			MultiFeature::Polygon(f) => Feature::MultiPolygon(f),
		}
	}
}
