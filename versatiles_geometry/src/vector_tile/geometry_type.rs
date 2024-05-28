use crate::basic::Geometry;

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

impl From<&Geometry> for GeomType {
	fn from(geometry: &Geometry) -> Self {
		use Geometry::*;
		match geometry {
			MultiPoint(_) => GeomType::MultiPoint,
			MultiLineString(_) => GeomType::MultiLineString,
			MultiPolygon(_) => GeomType::MultiPolygon,
			_ => panic!("only Multi* geometries are allowed"),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_as_u64() {
		assert_eq!(GeomType::Unknown.as_u64(), 0);
		assert_eq!(GeomType::MultiPoint.as_u64(), 1);
		assert_eq!(GeomType::MultiLineString.as_u64(), 2);
		assert_eq!(GeomType::MultiPolygon.as_u64(), 3);
	}

	#[test]
	fn test_from_u64() {
		assert_eq!(GeomType::from(0), GeomType::Unknown);
		assert_eq!(GeomType::from(1), GeomType::MultiPoint);
		assert_eq!(GeomType::from(2), GeomType::MultiLineString);
		assert_eq!(GeomType::from(3), GeomType::MultiPolygon);
		assert_eq!(GeomType::from(99), GeomType::Unknown);
	}

	#[test]
	fn test_from_geometry() {
		let multi_point = Geometry::new_multi_point(Geometry::parse1(vec![[1.0, 2.0], [3.0, 4.0]]));
		let multi_line_string = Geometry::new_multi_line_string(Geometry::parse2(vec![
			vec![[1.0, 2.0], [3.0, 4.0]],
			vec![[5.0, 6.0], [7.0, 8.0]],
		]));
		let multi_polygon = Geometry::new_multi_polygon(Geometry::parse3(vec![
			vec![vec![[0.0, 0.0], [5.0, 0.0], [2.5, 4.0], [0.0, 0.0]]],
			vec![vec![
				[6.0, 0.0],
				[9.0, 0.0],
				[9.0, 4.0],
				[6.0, 4.0],
				[6.0, 0.0],
			]],
		]));

		assert_eq!(GeomType::from(&multi_point), GeomType::MultiPoint);
		assert_eq!(
			GeomType::from(&multi_line_string),
			GeomType::MultiLineString
		);
		assert_eq!(GeomType::from(&multi_polygon), GeomType::MultiPolygon);
	}
}
