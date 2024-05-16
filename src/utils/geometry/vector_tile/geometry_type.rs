#[derive(Copy, Clone, Debug, PartialEq)]
pub enum GeomType {
	Unknown = 0,
	Point = 1,
	Linestring = 2,
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
			2 => GeomType::Linestring,
			3 => GeomType::Polygon,
			_ => GeomType::Unknown,
		}
	}
}
