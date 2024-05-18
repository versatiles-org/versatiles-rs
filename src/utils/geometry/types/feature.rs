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

pub type LineStringFeature = AbstractFeature<LineStringGeometry>;
pub type MultiLineStringFeature = AbstractFeature<MultiLineStringGeometry>;

pub type PolygonFeature = AbstractFeature<PolygonGeometry>;
pub type MultiPolygonFeature = AbstractFeature<MultiPolygonGeometry>;

#[derive(PartialEq)]
pub enum Feature {
	Point(PointFeature),
	LineString(LineStringFeature),
	Polygon(PolygonFeature),

	MultiPoint(MultiPointFeature),
	MultiLineString(MultiLineStringFeature),
	MultiPolygon(MultiPolygonFeature),
}

impl Debug for Feature {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Point(a) => a.fmt(f),
			Self::LineString(a) => a.fmt(f),
			Self::Polygon(a) => a.fmt(f),
			Self::MultiPoint(a) => a.fmt(f),
			Self::MultiLineString(a) => a.fmt(f),
			Self::MultiPolygon(a) => a.fmt(f),
		}
	}
}

pub enum MultiFeature {
	Point(MultiPointFeature),
	LineString(MultiLineStringFeature),
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
			G::LineString(geometry) => F::LineString(MultiLineStringFeature {
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
			MultiFeature::LineString(f) => Feature::MultiLineString(f),
			MultiFeature::Polygon(f) => Feature::MultiPolygon(f),
		}
	}
}
