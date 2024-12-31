use super::JsonValue;

impl From<f64> for JsonValue {
	fn from(input: f64) -> Self {
		JsonValue::Num(input)
	}
}

impl From<u8> for JsonValue {
	fn from(input: u8) -> Self {
		JsonValue::Num(input as f64)
	}
}

impl From<i32> for JsonValue {
	fn from(input: i32) -> Self {
		JsonValue::Num(input as f64)
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
