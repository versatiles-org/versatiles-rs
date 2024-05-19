use std::{fmt::Debug, hash::Hash};

#[derive(Clone, PartialEq)]
pub enum GeoValue {
	String(String),
	Float(f32),
	Double(f64),
	Int(i64),
	UInt(u64),
	Bool(bool),
}

impl Debug for GeoValue {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::String(v) => f.debug_tuple("String").field(v).finish(),
			Self::Float(v) => f.debug_tuple("F32").field(v).finish(),
			Self::Double(v) => f.debug_tuple("F64").field(v).finish(),
			Self::Int(v) => f.debug_tuple("I64").field(v).finish(),
			Self::UInt(v) => f.debug_tuple("U64").field(v).finish(),
			Self::Bool(v) => f.debug_tuple("Bool").field(v).finish(),
		}
	}
}

impl Eq for GeoValue {}
impl Hash for GeoValue {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
		core::mem::discriminant(self).hash(state);
	}
}
