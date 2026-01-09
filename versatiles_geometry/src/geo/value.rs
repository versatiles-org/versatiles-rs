//! Typed property values for GeoJSON-like features.
//!
//! This module defines [`GeoValue`], a small, ordered sum type used for feature
//! properties in the `versatiles_geometry` crate. It supports construction from
//! primitive Rust types, lexicographic/total ordering for deterministic output,
//! parsing from strings, hashing/equality, and conversion to the crate’s `JsonValue`.

use anyhow::{Result, bail};
use lazy_static::lazy_static;
use regex::Regex;
use std::{
	cmp::Ordering,
	fmt::{Debug, Display},
	hash::Hash,
};
use versatiles_core::json::JsonValue;

/// A compact, typed representation of a property value used in GeoJSON-like features.
///
/// Variants cover the common scalar JSON types plus separate `Float`/`Double` and
/// signed/unsigned integer distinctions. `Ord` and `Hash` are implemented to allow
/// use as map values with deterministic orderings.
#[derive(Clone, PartialEq)]
pub enum GeoValue {
	/// Boolean value.
	Bool(bool),
	/// 64-bit floating-point number.
	Double(f64),
	/// 32-bit floating-point number.
	Float(f32),
	/// 64-bit signed integer.
	Int(i64),
	/// JSON null.
	Null,
	/// UTF‑8 string.
	String(String),
	/// 64-bit unsigned integer.
	UInt(u64),
}

/// Formats the value as `Variant(inner)` to mirror the enum structure for developers.
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

/// Converts a `&str` into `GeoValue::String`.
impl From<&str> for GeoValue {
	fn from(value: &str) -> Self {
		GeoValue::String(value.to_string())
	}
}

/// Converts a `&String` into `GeoValue::String`.
impl From<&String> for GeoValue {
	fn from(value: &String) -> Self {
		GeoValue::String(value.clone())
	}
}

/// Converts a `String` into `GeoValue::String`.
impl From<String> for GeoValue {
	fn from(value: String) -> Self {
		GeoValue::String(value)
	}
}

/// Converts a `u8` into `GeoValue::UInt`.
impl From<u8> for GeoValue {
	fn from(value: u8) -> Self {
		GeoValue::UInt(u64::from(value))
	}
}

/// Converts an `i32` into `GeoValue::Int` if negative, otherwise `GeoValue::UInt`.
impl From<i32> for GeoValue {
	fn from(value: i32) -> Self {
		if value < 0 {
			GeoValue::Int(i64::from(value))
		} else {
			GeoValue::UInt(value as u64)
		}
	}
}

/// Converts a `u32` into `GeoValue::UInt`.
impl From<u32> for GeoValue {
	fn from(value: u32) -> Self {
		GeoValue::UInt(u64::from(value))
	}
}

/// Converts a `usize` into `GeoValue::UInt`.
impl From<usize> for GeoValue {
	fn from(value: usize) -> Self {
		GeoValue::UInt(value as u64)
	}
}

/// Converts an `i64` into `GeoValue::Int`.
impl From<i64> for GeoValue {
	fn from(value: i64) -> Self {
		GeoValue::Int(value)
	}
}

/// Converts a `u64` into `GeoValue::UInt`.
impl From<u64> for GeoValue {
	fn from(value: u64) -> Self {
		GeoValue::UInt(value)
	}
}

/// Converts an `f32` into `GeoValue::Float`.
impl From<f32> for GeoValue {
	fn from(value: f32) -> Self {
		GeoValue::Float(value)
	}
}

/// Converts an `f64` into `GeoValue::Double`.
impl From<f64> for GeoValue {
	fn from(value: f64) -> Self {
		GeoValue::Double(value)
	}
}

/// Converts a `bool` into `GeoValue::Bool`.
impl From<bool> for GeoValue {
	fn from(value: bool) -> Self {
		GeoValue::Bool(value)
	}
}

/// Equality is defined per-variant and value; see also `Ord` for cross-variant ordering.
impl Eq for GeoValue {}

/// Hashes both the variant tag and the inner value to ensure stable hashing across variants.
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

/// Provides total ordering. Values are first compared within the same variant; otherwise
/// a fixed variant precedence is used (see `variant_order`).
impl PartialOrd for GeoValue {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

/// Total ordering used for deterministic sorting across mixed variants.
impl Ord for GeoValue {
	fn cmp(&self, other: &Self) -> Ordering {
		use GeoValue::{Bool, Double, Float, Int, String, UInt};
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

/// Displays the value as a plain string (e.g., numbers as decimals, booleans as `true`/`false`, `null`).
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
				GeoValue::Null => "null".to_string(),
				GeoValue::String(v) => v.to_string(),
				GeoValue::UInt(v) => v.to_string(),
			}
		)
	}
}

impl GeoValue {
	/// Internal: variant precedence used when ordering mixed types (`String` < `Float` < `Double` < `Int` < `UInt` < `Bool` < `Null`).
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

	/// Parses a string into a `GeoValue` by detecting booleans, integers, unsigned integers,
	/// and floating-point numbers; falls back to `String` (empty input yields empty string).
	///
	/// Numbers with leading zeros (except `0` itself or `0.x`) are treated as strings.
	/// Supports exponential notation (e.g., `1.5e10`, `1E-3`).
	#[must_use]
	pub fn parse_str(value: &str) -> Self {
		lazy_static! {
			// Double: requires decimal point and/or exponent, no leading zeros
			// Format: -?[0|1-9...](.[digits])([eE][+-]?digits) where . or e/E is required
			static ref REG_DOUBLE: Regex = Regex::new(
				r"^-?(?:0|[1-9]\d*)(?:(?:\.\d+)(?:[eE][+-]?\d+)?|[eE][+-]?\d+)$"
			).unwrap();
			// Signed integer (negative): -[0|1-9...]
			static ref REG_INT: Regex = Regex::new(r"^-(?:0|[1-9]\d*)$").unwrap();
			// Unsigned integer: 0 or [1-9...]
			static ref REG_UINT: Regex = Regex::new(r"^(?:0|[1-9]\d*)$").unwrap();
		}

		match value {
			"" => GeoValue::String(String::new()),
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

	/// Returns the value as `u64` if it is `Int` or `UInt`; otherwise returns an error.
	pub fn as_u64(&self) -> Result<u64> {
		match self {
			GeoValue::Int(v) => Ok(*v as u64),
			GeoValue::UInt(v) => Ok(*v),
			_ => bail!("value is not an integer"),
		}
	}

	/// Converts the `GeoValue` to the crate’s `JsonValue` representation.
	/// Note: integer types are converted to `Number` (as `f64`) to match JSON semantics.
	#[must_use]
	pub fn to_json(&self) -> JsonValue {
		match self {
			GeoValue::Bool(v) => JsonValue::from(*v),
			GeoValue::Double(v) => JsonValue::from(*v),
			GeoValue::Float(v) => JsonValue::from(f64::from(*v)),
			GeoValue::Int(v) => JsonValue::from(*v as f64),
			GeoValue::Null => JsonValue::Null,
			GeoValue::String(v) => JsonValue::from(v.clone()),
			GeoValue::UInt(v) => JsonValue::from(*v as f64),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	#[test]
	fn test_geo_value_ord() {
		// Test ordering within the same variant
		assert!(GeoValue::from("a") < GeoValue::from("b"));
		assert!(GeoValue::from(1.0f32) < GeoValue::from(2.0f32));
		assert!(GeoValue::from(1.0f64) < GeoValue::from(2.0f64));
		assert!(GeoValue::from(1) < GeoValue::from(2));
		assert!(GeoValue::from(-1) < GeoValue::from(0));
		assert!(GeoValue::from(1u64) < GeoValue::from(2u64));
		assert!(GeoValue::from(false) < GeoValue::from(true));

		// Test ordering between different variants
		assert!(GeoValue::from("a") < GeoValue::from(1.0f32));
		assert!(GeoValue::from(1.0f32) < GeoValue::from(1.0f64));
		assert!(GeoValue::from(1.0f64) < GeoValue::from(1));
		assert!(GeoValue::from(1u64) < GeoValue::from(false));
	}

	#[rstest]
	#[case(GeoValue::from("a"), GeoValue::from("b"))]
	#[case(GeoValue::from(1.0f32), GeoValue::from(2.0f32))]
	#[case(GeoValue::from(1.0f64), GeoValue::from(2.0f64))]
	#[case(GeoValue::from(1), GeoValue::from(2))]
	#[case(GeoValue::from(-1), GeoValue::from(2))]
	#[case(GeoValue::from(-2), GeoValue::from(-1))]
	#[case(GeoValue::from(1u64), GeoValue::from(2u64))]
	#[case(GeoValue::from(false), GeoValue::from(true))]
	#[case(GeoValue::from("a"), GeoValue::from(1.0f32))]
	#[case(GeoValue::from(1.0f32), GeoValue::from(1.0f64))]
	#[case(GeoValue::from(1.0f64), GeoValue::from(1))]
	#[case(GeoValue::from(1u64), GeoValue::from(false))]
	fn test_geo_value_partial_cmp(#[case] a: GeoValue, #[case] b: GeoValue) {
		// Test partial_cmp within the same variant
		assert_eq!(a.partial_cmp(&b), Some(Ordering::Less));
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

	#[rstest]
	// Booleans
	#[case(GeoValue::Bool(false), "false")]
	#[case(GeoValue::Bool(true), "true")]
	// Doubles with decimal point
	#[case(GeoValue::Double(-0.42), "-0.42")]
	#[case(GeoValue::Double(-0.42), "-0.420")]
	#[case(GeoValue::Double(-23.42), "-23.42")]
	#[case(GeoValue::Double(0.0), "0.0")]
	#[case(GeoValue::Double(0.42), "0.42")]
	#[case(GeoValue::Double(0.42), "0.420")]
	#[case(GeoValue::Double(23.42), "23.42")]
	// Exponential notation
	#[case(GeoValue::Double(-1.5e10), "-1.5e10")]
	#[case(GeoValue::Double(0.0), "0.0e0")]
	#[case(GeoValue::Double(0.0), "0e0")]
	#[case(GeoValue::Double(1.23e-4), "1.23e-4")]
	#[case(GeoValue::Double(1.23e4), "1.23e4")]
	#[case(GeoValue::Double(1.5e-10), "1.5e-10")]
	#[case(GeoValue::Double(1.5e10), "1.5e+10")]
	#[case(GeoValue::Double(1.5e10), "1.5e10")]
	#[case(GeoValue::Double(1.5e10), "1.5e10")]
	#[case(GeoValue::Double(1.5e10), "1.5E10")]
	#[case(GeoValue::Double(1e-3), "1e-3")]
	#[case(GeoValue::Double(1e10), "1e10")]
	#[case(GeoValue::Double(5e0), "5e0")]
	// Signed integers
	#[case(GeoValue::Int(-4), "-4")]
	#[case(GeoValue::Int(-42), "-42")]
	#[case(GeoValue::Int(0), "-0")]
	// Unsigned integers
	#[case(GeoValue::UInt(0), "0")]
	#[case(GeoValue::UInt(42), "42")]
	#[case(GeoValue::UInt(123456789), "123456789")]
	// Strings (invalid number formats)
	#[case(GeoValue::String(" 42".to_string()), " 42")]
	#[case(GeoValue::String("-.42".to_string()), "-.42")]
	#[case(GeoValue::String("-00.0".to_string()), "-00.0")]
	#[case(GeoValue::String("-042".to_string()), "-042")]
	#[case(GeoValue::String(".42".to_string()), ".42")]
	#[case(GeoValue::String(".420".to_string()), ".420")]
	#[case(GeoValue::String(String::new()), "")]
	#[case(GeoValue::String("+42".to_string()), "+42")]
	#[case(GeoValue::String("00.0".to_string()), "00.0")]
	#[case(GeoValue::String("00".to_string()), "00")]
	#[case(GeoValue::String("01e5".to_string()), "01e5")]
	#[case(GeoValue::String("042".to_string()), "042")]
	#[case(GeoValue::String("1.2.3".to_string()), "1.2.3")]
	#[case(GeoValue::String("123abc".to_string()), "123abc")]
	#[case(GeoValue::String("1e".to_string()), "1e")]
	#[case(GeoValue::String("1e1e1".to_string()), "1e1e1")]
	#[case(GeoValue::String("42 ".to_string()), "42 ")]
	#[case(GeoValue::String("e10".to_string()), "e10")]
	#[case(GeoValue::String("hello".to_string()), "hello")]
	fn test_parse_str(#[case] value: GeoValue, #[case] text: &str) {
		assert_eq!(GeoValue::parse_str(text), value);
	}

	#[rstest]
	#[case(GeoValue::Bool(true), "Bool(true)")]
	#[case(GeoValue::Bool(false), "Bool(false)")]
	#[case(GeoValue::Float(23.42), "Float(23.42)")]
	#[case(GeoValue::Double(23.42), "Double(23.42)")]
	#[case(GeoValue::Double(-23.42), "Double(-23.42)")]
	#[case(GeoValue::Int(-42), "Int(-42)")]
	#[case(GeoValue::UInt(42), "UInt(42)")]
	#[case(GeoValue::Null, "Null")]
	fn test_debug(#[case] value: GeoValue, #[case] text: &str) {
		assert_eq!(format!("{:?}", value), text);
	}

	#[rstest]
	#[case(GeoValue::Bool(true), JsonValue::Boolean(true))]
	#[case(GeoValue::Bool(false), JsonValue::Boolean(false))]
	#[case(GeoValue::Float(32.0), JsonValue::Number(32.0))]
	#[case(GeoValue::Double(23.42), JsonValue::Number(23.42))]
	#[case(GeoValue::Double(-23.42), JsonValue::Number(-23.42))]
	#[case(GeoValue::Int(-42),JsonValue::Number(-42.0))]
	#[case(GeoValue::UInt(42), JsonValue::Number(42.0))]
	#[case(GeoValue::Null, JsonValue::Null)]
	fn test_json(#[case] value: GeoValue, #[case] json: JsonValue) {
		assert_eq!(value.to_json(), json);
	}
}
