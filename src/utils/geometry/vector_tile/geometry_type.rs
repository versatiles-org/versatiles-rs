#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub enum GeomType {
	#[default]
	Unknown = 0,
	Point = 1,
	LineString = 2,
	Polygon = 3,
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
			1 => GeomType::Point,
			2 => GeomType::LineString,
			3 => GeomType::Polygon,
			_ => GeomType::Unknown,
		}
	}
}
