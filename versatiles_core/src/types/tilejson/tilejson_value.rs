use crate::json::JsonValue;
use anyhow::{Result, bail, ensure};

/// An enumeration representing allowed JSON value types in this module.
#[derive(Clone, Debug, PartialEq)]
pub enum TileJsonValue {
	/// A list of strings (originally from a JSON array).
	List(Vec<String>),
	/// A single string (originally from a JSON string).
	String(String),
	/// A single byte (stored as `u8`). Must be in `[0, 255]`.
	Byte(u8),
}

impl TileJsonValue {
	/// Returns `Some(&str)` if the value is a string, or `None` otherwise.
	pub fn get_str(&self) -> Option<&str> {
		match self {
			TileJsonValue::String(s) => Some(s),
			_ => None,
		}
	}

	/// Returns `Some(u8)` if the value is a byte, or `None` otherwise.
	pub fn get_byte(&self) -> Option<u8> {
		match self {
			TileJsonValue::Byte(b) => Some(*b),
			_ => None,
		}
	}

	/// Converts this `TileJsonValue` into a generic [`JsonValue`].
	pub fn as_json_value(&self) -> JsonValue {
		match self {
			TileJsonValue::Byte(b) => JsonValue::from(*b),
			TileJsonValue::List(l) => JsonValue::from(l),
			TileJsonValue::String(s) => JsonValue::from(s),
		}
	}

	/// Returns a string describing which variant this `TileJsonValue` is (`"List"`, `"String"`, or `"Byte"`).
	pub fn get_type(&self) -> &str {
		match self {
			TileJsonValue::Byte(_) => "Byte",
			TileJsonValue::List(_) => "List",
			TileJsonValue::String(_) => "String",
		}
	}

	/// Returns `true` if this value is a `TileJsonValue::List`.
	pub fn is_list(&self) -> bool {
		matches!(self, TileJsonValue::List(_))
	}

	/// Returns `true` if this value is a `TileJsonValue::String`.
	pub fn is_string(&self) -> bool {
		matches!(self, TileJsonValue::String(_))
	}

	/// Returns `true` if this value is a `TileJsonValue::Byte`.
	pub fn is_byte(&self) -> bool {
		matches!(self, TileJsonValue::Byte(_))
	}
}

impl From<u8> for TileJsonValue {
	fn from(value: u8) -> Self {
		TileJsonValue::Byte(value)
	}
}

impl TryFrom<&JsonValue> for TileJsonValue {
	type Error = anyhow::Error;

	/// Attempts to convert a reference to a [`JsonValue`] into a [`TileJsonValue`].
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The `JsonValue` is out of range for a byte (`u8`).
	/// - The `JsonValue` is some other type not supported by [`TileJsonValue`].
	fn try_from(value: &JsonValue) -> Result<Self> {
		match value {
			JsonValue::String(s) => Ok(TileJsonValue::String(s.to_owned())),
			JsonValue::Array(a) => Ok(TileJsonValue::List(a.as_string_vec()?)),
			JsonValue::Number(n) => {
				ensure!((0.0..=255.0).contains(n), "Number out of byte range: {n}");
				Ok(TileJsonValue::Byte(*n as u8))
			}
			_ => bail!("Invalid value type: only string, array, or byte allowed"),
		}
	}
}

#[cfg(test)]
mod tests {}
