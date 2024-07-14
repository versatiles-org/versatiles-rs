use lazy_static::lazy_static;
use regex::{Regex, RegexBuilder};
use std::{
	cmp::Ordering,
	fmt::{Debug, Display},
	hash::Hash,
};

#[derive(Clone, PartialEq)]
pub enum GeoValue {
	Bool(bool),
	Double(f64),
	Float(f32),
	Int(i64),
	Null,
	String(String),
	UInt(u64),
}

impl Debug for GeoValue {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::String(v) => f.debug_tuple("String").field(v).finish(),
			Self::Float(v) => f.debug_tuple("Float").field(v).finish(),
			Self::Double(v) => f.debug_tuple("Double").field(v).finish(),
			Self::Int(v) => f.debug_tuple("Int").field(v).finish(),
			Self::UInt(v) => f.debug_tuple("UInt").field(v).finish(),
			Self::Bool(v) => f.debug_tuple("Bool").field(v).finish(),
			Self::Null => f.debug_tuple("Null").finish(),
		}
	}
}

impl From<&str> for GeoValue {
	fn from(value: &str) -> Self {
		GeoValue::String(value.to_string())
	}
}

impl From<&String> for GeoValue {
	fn from(value: &String) -> Self {
		GeoValue::String(value.clone())
	}
}

impl From<String> for GeoValue {
	fn from(value: String) -> Self {
		GeoValue::String(value)
	}
}

impl From<u8> for GeoValue {
	fn from(value: u8) -> Self {
		GeoValue::UInt(value as u64)
	}
}

impl From<i32> for GeoValue {
	fn from(value: i32) -> Self {
		if value < 0 {
			GeoValue::Int(value as i64)
		} else {
			GeoValue::UInt(value as u64)
		}
	}
}

impl From<u32> for GeoValue {
	fn from(value: u32) -> Self {
		GeoValue::UInt(value as u64)
	}
}

impl From<i64> for GeoValue {
	fn from(value: i64) -> Self {
		GeoValue::Int(value)
	}
}

impl From<u64> for GeoValue {
	fn from(value: u64) -> Self {
		GeoValue::UInt(value)
	}
}

impl From<f32> for GeoValue {
	fn from(value: f32) -> Self {
		GeoValue::Float(value)
	}
}

impl From<f64> for GeoValue {
	fn from(value: f64) -> Self {
		GeoValue::Double(value)
	}
}

impl From<bool> for GeoValue {
	fn from(value: bool) -> Self {
		GeoValue::Bool(value)
	}
}

impl Eq for GeoValue {}

impl Hash for GeoValue {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
		core::mem::discriminant(self).hash(state);
		match self {
			GeoValue::Bool(v) => v.hash(state),
			GeoValue::Double(v) => v.to_bits().hash(state),
			GeoValue::Float(v) => v.to_bits().hash(state),
			GeoValue::Int(v) => v.hash(state),
			GeoValue::Null => (),
			GeoValue::String(v) => v.hash(state),
			GeoValue::UInt(v) => v.hash(state),
		}
	}
}

impl PartialOrd for GeoValue {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for GeoValue {
	fn cmp(&self, other: &Self) -> Ordering {
		use GeoValue::*;
		match (self, other) {
			(String(a), String(b)) => a.cmp(b),
			(Float(a), Float(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
			(Double(a), Double(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
			(Int(a), Int(b)) => a.cmp(b),
			(UInt(a), UInt(b)) => a.cmp(b),
			(Bool(a), Bool(b)) => a.cmp(b),
			_ => self.variant_order().cmp(&other.variant_order()),
		}
	}
}
impl Display for GeoValue {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"{}",
			match self {
				GeoValue::Bool(v) => v.to_string(),
				GeoValue::Double(v) => v.to_string(),
				GeoValue::Float(v) => v.to_string(),
				GeoValue::Int(v) => v.to_string(),
				GeoValue::Null => String::from("null"),
				GeoValue::String(v) => v.to_string(),
				GeoValue::UInt(v) => v.to_string(),
			}
		)
	}
}

impl GeoValue {
	fn variant_order(&self) -> u8 {
		match self {
			GeoValue::String(_) => 0,
			GeoValue::Float(_) => 1,
			GeoValue::Double(_) => 2,
			GeoValue::Int(_) => 3,
			GeoValue::UInt(_) => 4,
			GeoValue::Bool(_) => 5,
			GeoValue::Null => 6,
		}
	}
	pub fn parse_str(value: &str) -> Self {
		lazy_static! {
			static ref REG_DOUBLE: Regex = RegexBuilder::new(r"^\d*\.\d+$").build().unwrap();
			static ref REG_INT: Regex = RegexBuilder::new(r"^\-\d+$").build().unwrap();
			static ref REG_UINT: Regex = RegexBuilder::new(r"^\d+$").build().unwrap();
		}

		match value {
			"" => GeoValue::String("".to_string()),
			"true" => GeoValue::Bool(true),
			"false" => GeoValue::Bool(false),
			_ => {
				if REG_DOUBLE.is_match(value) {
					GeoValue::Double(value.parse::<f64>().unwrap())
				} else if REG_INT.is_match(value) {
					GeoValue::Int(value.parse::<i64>().unwrap())
				} else if REG_UINT.is_match(value) {
					GeoValue::UInt(value.parse::<u64>().unwrap())
				} else {
					GeoValue::String(value.to_string())
				}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_geo_value_ord() {
		// Test ordering within the same variant
		assert!(GeoValue::from("a") < GeoValue::from("b"));
		assert!(GeoValue::from(1.0f32) < GeoValue::from(2.0f32));
		assert!(GeoValue::from(1.0f64) < GeoValue::from(2.0f64));
		assert!(GeoValue::from(1) < GeoValue::from(2));
		assert!(GeoValue::from(1u64) < GeoValue::from(2u64));
		assert!(GeoValue::from(false) < GeoValue::from(true));

		// Test ordering between different variants
		assert!(GeoValue::from("a") < GeoValue::from(1.0f32));
		assert!(GeoValue::from(1.0f32) < GeoValue::from(1.0f64));
		assert!(GeoValue::from(1.0f64) < GeoValue::from(1));
		assert!(GeoValue::from(1u64) < GeoValue::from(false));
	}

	#[test]
	fn test_geo_value_partial_cmp() {
		// Test partial_cmp within the same variant
		assert_eq!(
			GeoValue::from("a").partial_cmp(&GeoValue::from("b")),
			Some(Ordering::Less)
		);
		assert_eq!(
			GeoValue::from(1.0f32).partial_cmp(&GeoValue::from(2.0f32)),
			Some(Ordering::Less)
		);
		assert_eq!(
			GeoValue::from(1.0f64).partial_cmp(&GeoValue::from(2.0f64)),
			Some(Ordering::Less)
		);
		assert_eq!(
			GeoValue::from(1).partial_cmp(&GeoValue::from(2)),
			Some(Ordering::Less)
		);
		assert_eq!(
			GeoValue::from(1u64).partial_cmp(&GeoValue::from(2u64)),
			Some(Ordering::Less)
		);
		assert_eq!(
			GeoValue::from(false).partial_cmp(&GeoValue::from(true)),
			Some(Ordering::Less)
		);

		// Test partial_cmp between different variants
		assert_eq!(
			GeoValue::from("a").partial_cmp(&GeoValue::from(1.0f32)),
			Some(Ordering::Less)
		);
		assert_eq!(
			GeoValue::from(1.0f32).partial_cmp(&GeoValue::from(1.0f64)),
			Some(Ordering::Less)
		);
		assert_eq!(
			GeoValue::from(1.0f64).partial_cmp(&GeoValue::from(1)),
			Some(Ordering::Less)
		);
		assert_eq!(
			GeoValue::from(1u64).partial_cmp(&GeoValue::from(false)),
			Some(Ordering::Less)
		);
	}

	#[test]
	fn test_geo_value_eq() {
		// Test equality within the same variant
		assert_eq!(GeoValue::from("a"), GeoValue::from("a"));
		assert_eq!(GeoValue::from(1.0f32), GeoValue::from(1.0f32));
		assert_eq!(GeoValue::from(1.0f64), GeoValue::from(1.0f64));
		assert_eq!(GeoValue::from(1), GeoValue::from(1));
		assert_eq!(GeoValue::from(1u64), GeoValue::from(1u64));
		assert_eq!(GeoValue::from(false), GeoValue::from(false));

		// Test inequality within the same variant
		assert_ne!(GeoValue::from("a"), GeoValue::from("b"));
		assert_ne!(GeoValue::from(1.0f32), GeoValue::from(2.0f32));
		assert_ne!(GeoValue::from(1.0f64), GeoValue::from(2.0f64));
		assert_ne!(GeoValue::from(1), GeoValue::from(2));
		assert_ne!(GeoValue::from(1u64), GeoValue::from(2u64));
		assert_ne!(GeoValue::from(false), GeoValue::from(true));
	}
}
