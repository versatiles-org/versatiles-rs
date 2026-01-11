use crate::{json::JsonValue, utils::float_to_int};
use anyhow::{Result, bail};

/// An enumeration representing allowed JSON value types in this module.
#[derive(Clone, Debug, PartialEq)]
pub enum TileJsonValue {
	/// A list of strings (originally from a JSON array).
	List(Vec<String>),
	/// A single string (originally from a JSON string).
	String(String),
	/// A single integer (stored as `i64`).
	Integer(i64),
}

impl TileJsonValue {
	/// Returns `Some(&str)` if the value is a string, or `None` otherwise.
	pub fn get_str(&self) -> Option<&str> {
		match self {
			TileJsonValue::String(s) => Some(s),
			_ => None,
		}
	}

	/// Returns `Some(i64)` if the value is a byte, or `None` otherwise.
	pub fn get_integer(&self) -> Option<i64> {
		match self {
			TileJsonValue::Integer(b) => Some(*b),
			_ => None,
		}
	}

	/// Converts this `TileJsonValue` into a generic [`JsonValue`].
	pub fn as_json_value(&self) -> JsonValue {
		match self {
			TileJsonValue::Integer(b) => JsonValue::from(*b),
			TileJsonValue::List(l) => JsonValue::from(l),
			TileJsonValue::String(s) => JsonValue::from(s),
		}
	}

	/// Returns a string describing which variant this `TileJsonValue` is (`"List"`, `"String"`, or `"Integer"`).
	pub fn get_type(&self) -> &str {
		match self {
			TileJsonValue::Integer(_) => "Integer",
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

	/// Returns `true` if this value is a `TileJsonValue::Integer`.
	pub fn is_integer(&self) -> bool {
		matches!(self, TileJsonValue::Integer(_))
	}
}

impl From<u8> for TileJsonValue {
	fn from(value: u8) -> Self {
		TileJsonValue::Integer(value as i64)
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
			JsonValue::Number(n) => Ok(TileJsonValue::Integer(float_to_int(*n)?)),
			_ => bail!("Invalid value type: only string, array, or integer allowed"),
		}
	}
}

#[cfg(test)]
mod tests {}
