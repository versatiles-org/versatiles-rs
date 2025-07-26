use super::JsonValue;

impl From<f64> for JsonValue {
	fn from(input: f64) -> Self {
		JsonValue::Number(input)
	}
}

impl From<u8> for JsonValue {
	fn from(input: u8) -> Self {
		JsonValue::Number(input as f64)
	}
}

impl From<i32> for JsonValue {
	fn from(input: i32) -> Self {
		JsonValue::Number(input as f64)
	}
}

pub trait AsNumber<T> {
	fn convert(value: f64) -> T;
}

impl AsNumber<f64> for f64 {
	fn convert(value: f64) -> f64 {
		value
	}
}

impl AsNumber<f32> for f32 {
	fn convert(value: f64) -> f32 {
		value as f32
	}
}

impl AsNumber<u8> for u8 {
	fn convert(value: f64) -> u8 {
		value as u8
	}
}

impl AsNumber<u16> for u16 {
	fn convert(value: f64) -> u16 {
		value as u16
	}
}

impl AsNumber<u32> for u32 {
	fn convert(value: f64) -> u32 {
		value as u32
	}
}

impl AsNumber<u64> for u64 {
	fn convert(value: f64) -> u64 {
		value as u64
	}
}

impl AsNumber<i8> for i8 {
	fn convert(value: f64) -> i8 {
		value as i8
	}
}

impl AsNumber<i16> for i16 {
	fn convert(value: f64) -> i16 {
		value as i16
	}
}

impl AsNumber<i32> for i32 {
	fn convert(value: f64) -> i32 {
		value as i32
	}
}

impl AsNumber<i64> for i64 {
	fn convert(value: f64) -> i64 {
		value as i64
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

	#[test]
	fn test_as_number_f64() {
		let value = f64::convert(42.5_f64);
		assert_eq!(value, 42.5);
	}

	#[test]
	fn test_as_number_f32() {
		let value = f32::convert(42.5_f64);
		assert_eq!(value, 42.5_f32);
	}

	#[test]
	fn test_as_number_u8() {
		let value = u8::convert(42.5_f64);
		assert_eq!(value, 42_u8); // Should truncate to integer
	}

	#[test]
	fn test_as_number_u16() {
		let value = u16::convert(65535.0_f64);
		assert_eq!(value, 65535_u16);
	}

	#[test]
	fn test_as_number_u32() {
		let value = u32::convert(123456789.0_f64);
		assert_eq!(value, 123456789_u32);
	}

	#[test]
	fn test_as_number_i8() {
		let value = i8::convert(-128.0_f64);
		assert_eq!(value, -128_i8);
	}

	#[test]
	fn test_as_number_i32() {
		let value = i32::convert(-123456789.0_f64);
		assert_eq!(value, -123456789_i32);
	}

	#[test]
	fn test_as_number_precision_loss() {
		// Ensure precision loss is acceptable for conversions
		let value = u32::convert(123456789.999_f64);
		assert_eq!(value, 123456789_u32);
	}

	#[test]
	fn test_as_number_u64() {
		let value = u64::convert(123456789012345.0_f64);
		assert_eq!(value, 123456789012345_u64);
	}

	#[test]
	fn test_as_number_i16() {
		let value = i16::convert(-32768.0_f64);
		assert_eq!(value, -32768_i16);
	}

	#[test]
	fn test_as_number_i64() {
		let value = i64::convert(-1234567890123.0_f64);
		assert_eq!(value, -1234567890123_i64);
	}
}
