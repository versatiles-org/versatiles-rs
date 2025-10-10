//! JSON number conversions for `JsonValue` and numeric types.
//!
//! This module provides `From` implementations to create `JsonValue::Number` from various
//! Rust numeric types, and the `AsNumber` trait to convert `JsonValue::Number` back to Rust types.

use super::JsonValue;

/// Create a JSON number value from a 64-bit floating point.
impl From<f64> for JsonValue {
	fn from(input: f64) -> Self {
		JsonValue::Number(input)
	}
}

/// Create a JSON number value from an 8-bit unsigned integer.
impl From<u8> for JsonValue {
	fn from(input: u8) -> Self {
		JsonValue::Number(f64::from(input))
	}
}

/// Create a JSON number value from a 32-bit signed integer.
impl From<i32> for JsonValue {
	fn from(input: i32) -> Self {
		JsonValue::Number(f64::from(input))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_from_f64() {
		let value: JsonValue = JsonValue::from(42.5_f64);
		if let JsonValue::Number(num) = value {
			assert_eq!(num, 42.5);
		} else {
			panic!("Expected JsonValue::Number");
		}
	}

	#[test]
	fn test_from_u8() {
		let value: JsonValue = JsonValue::from(42_u8);
		if let JsonValue::Number(num) = value {
			assert_eq!(num, 42.0);
		} else {
			panic!("Expected JsonValue::Number");
		}
	}

	#[test]
	fn test_from_i32() {
		let value: JsonValue = JsonValue::from(-42_i32);
		if let JsonValue::Number(num) = value {
			assert_eq!(num, -42.0);
		} else {
			panic!("Expected JsonValue::Number");
		}
	}
}
