use geo_types::Geometry;

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

impl From<&Geometry<f64>> for GeomType {
	fn from(geometry: &Geometry<f64>) -> Self {
		match geometry {
			Geometry::Point(_) | Geometry::MultiPoint(_) => GeomType::MultiPoint,
			Geometry::Line(_) | Geometry::LineString(_) | Geometry::MultiLineString(_) => GeomType::MultiLineString,
			Geometry::Polygon(_) | Geometry::Rect(_) | Geometry::Triangle(_) | Geometry::MultiPolygon(_) => {
				GeomType::MultiPolygon
			}
			Geometry::GeometryCollection(_) => GeomType::Unknown,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use geo_types::{LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};

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
		let multi_point = Geometry::MultiPoint(MultiPoint(vec![Point::new(1.0, 2.0), Point::new(3.0, 4.0)]));
		let multi_line_string = Geometry::MultiLineString(MultiLineString(vec![
			LineString::from(vec![[1.0, 2.0], [3.0, 4.0]]),
			LineString::from(vec![[5.0, 6.0], [7.0, 8.0]]),
		]));
		let multi_polygon = Geometry::MultiPolygon(MultiPolygon(vec![
			Polygon::new(
				LineString::from(vec![[0.0, 0.0], [10.0, 0.0], [5.0, 8.0], [0.0, 0.0]]),
				vec![],
			),
			Polygon::new(
				LineString::from(vec![[12.0, 0.0], [18.0, 0.0], [18.0, 8.0], [12.0, 8.0], [12.0, 0.0]]),
				vec![],
			),
		]));

		assert_eq!(GeomType::from(&multi_point), GeomType::MultiPoint);
		assert_eq!(GeomType::from(&multi_line_string), GeomType::MultiLineString);
		assert_eq!(GeomType::from(&multi_polygon), GeomType::MultiPolygon);
	}
}
