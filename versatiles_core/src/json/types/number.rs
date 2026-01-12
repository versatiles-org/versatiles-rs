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

/// Implement `From<Number>` for `JsonValue` for types with lossless f64 conversion.
macro_rules! impl_from_number_lossless {
	($($t:ty),+ $(,)?) => {
		$(
			impl From<$t> for JsonValue {
				fn from(input: $t) -> Self {
					JsonValue::Number(f64::from(input))
				}
			}
		)+
	};
}

/// Implement `From<Number>` for `JsonValue` for types without lossless f64 conversion.
macro_rules! impl_from_number_lossy {
	($($t:ty),+ $(,)?) => {
		$(
			#[allow(clippy::cast_precision_loss)]
			impl From<$t> for JsonValue {
				fn from(input: $t) -> Self {
					JsonValue::Number(input as f64)
				}
			}
		)+
	};
}

impl_from_number_lossless!(f32, u8, u16, u32, i8, i16, i32);
impl_from_number_lossy!(u64, u128, usize, i64, i128, isize);

#[cfg(test)]
mod tests {
	use super::*;

	/// Generate per-type tests that assert `From<T> for JsonValue` maps to `Number(v as f64)`.
	/// Only include values that are within or equal to the exact-integer range of f64 where relevant.
	macro_rules! gen_from_number_tests {
		($($name:ident : $t:ty => [$($v:expr),+ $(,)?];)+) => {
			$(
				#[test]
				#[allow(clippy::cast_lossless)]
				fn $name() {
					let vals: &[$t] = &[$($v),+];
					for &v in vals {
						let j: JsonValue = JsonValue::from(v);
						match j {
							JsonValue::Number(n) => assert_eq!(n, v as f64, "failed for value {:?} ({})", v, stringify!($t)),
							_ => panic!("expected JsonValue::Number for type {}", stringify!($t)),
						}
					}
				}
			)+
		};
	}

	// 2^53 - 1 is the largest integer exactly representable in f64
	const F64_SAFE_INT_MAX: u64 = 9_007_199_254_740_991; // 2^53 - 1

	gen_from_number_tests! {
		from_f32: f32 => [0.0, -1.5, 3.5, 42.0];
		from_f64: f64 => [0.0, -1.5, 3.5, 42.0];

		from_u8:  u8  => [0, 1, 255];
		from_u16: u16 => [0, 65535];
		from_u32: u32 => [0, 1, 1_000_000_000];
		// keep u64 within f64 exact-int range to avoid precision-based false negatives
		from_u64: u64 => [0, 1, F64_SAFE_INT_MAX];
		// same for u128: use values within the safe range
		from_u128: u128 => [0, 1, 1_000_000_000_000u128];
		from_usize: usize => [0, 1, 123_456];

		from_i8:  i8  => [-128, -1, 0, 1, 127];
		from_i16: i16 => [-32768, -1, 0, 32767];
		from_i32: i32 => [-1_000_000_000, 0, 1_000_000_000];
		from_i64: i64 => [-4_000_000_000, 0, 1_234_567_890_123];
		from_i128: i128 => [-1_234_567_890_123_i128, 0i128, 1_234_567_890_123_i128];
		from_isize: isize => [-123_456, 0, 123_456];
	}

	/// Sanity check: very large integers outside the f64 exact range will be rounded when cast.
	/// We don't assert exact equality against the original integer here — this just documents
	/// the behavior and ensures the conversion path doesn't panic.
	#[test]
	fn large_ints_outside_safe_range_do_not_panic() {
		let big_u64: u64 = F64_SAFE_INT_MAX + 1; // not exactly representable as f64
		let big_i128: i128 = 10_000_000_000_000_000_000_000i128; // well beyond 2^53

		let j1: JsonValue = JsonValue::from(big_u64);
		let j2: JsonValue = JsonValue::from(big_i128);

		match j1 {
			JsonValue::Number(n) => assert!(n.is_finite()),
			_ => panic!("expected Number"),
		}
		match j2 {
			JsonValue::Number(n) => assert!(n.is_finite()),
			_ => panic!("expected Number"),
		}
	}
}
