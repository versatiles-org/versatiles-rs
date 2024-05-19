use crate::utils::geometry::types::{Geometry, GeometryValue};

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub enum GeomType {
	#[default]
	Unknown = 0,
	MultiPoint = 1,
	MultiLineString = 2,
	MultiPolygon = 3,
}

impl GeomType {
	#[allow(dead_code)]
	pub fn as_u64(&self) -> u64 {
		*self as u64
	}
}

impl From<u64> for GeomType {
	fn from(value: u64) -> Self {
		match value {
			1 => GeomType::MultiPoint,
			2 => GeomType::MultiLineString,
			3 => GeomType::MultiPolygon,
			_ => GeomType::Unknown,
		}
	}
}

impl From<Geometry> for GeomType {
	fn from(geometry: Geometry) -> Self {
		use GeometryValue::*;
		match geometry.value {
			MultiPoint(_) => GeomType::MultiPoint,
			MultiLineString(_) => GeomType::MultiLineString,
			MultiPolygon(_) => GeomType::MultiPolygon,
			_ => panic!("only Multi* geometries are allowed"),
		}
	}
}
