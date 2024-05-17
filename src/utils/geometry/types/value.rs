use std::fmt::Debug;

#[derive(Clone, PartialEq)]
pub enum GeoValue {
	GeoString(String),
	GeoF32(f32),
	GeoF64(f64),
	GeoI64(i64),
	GeoU64(u64),
	GeoBool(bool),
}

impl Debug for GeoValue {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::GeoString(v) => f.debug_tuple("String").field(v).finish(),
			Self::GeoF32(v) => f.debug_tuple("F32").field(v).finish(),
			Self::GeoF64(v) => f.debug_tuple("F64").field(v).finish(),
			Self::GeoI64(v) => f.debug_tuple("I64").field(v).finish(),
			Self::GeoU64(v) => f.debug_tuple("U64").field(v).finish(),
			Self::GeoBool(v) => f.debug_tuple("Bool").field(v).finish(),
		}
	}
}
