use crate::utils::JsonValue;
use anyhow::{bail, ensure, Result};
use std::collections::BTreeMap;

/// A map storing string keys and their associated typed JSON values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileJsonValues(BTreeMap<String, TileJsonValue>);

impl TileJsonValues {
	/// Inserts a key-value pair into the internal BTreeMap, converting the `JsonValue` into a `TileJsonValue`.
	///
	/// # Errors
	///
	/// Returns an error if the provided `JsonValue` cannot be converted into a `TileJsonValue`.
	pub fn insert(&mut self, key: &str, value: &JsonValue) -> Result<()> {
		// Convert JsonValue into a typed TileJsonValue, and insert into the map.
		self
			.0
			.insert(key.to_owned(), TileJsonValue::try_from(value)?);
		Ok(())
	}

	/// Gets a cloned `String` value for a given key, if that key exists and is stored as a string.
	pub fn get_string(&self, key: &str) -> Option<String> {
		self
			.0
			.get(key)
			.and_then(|v| v.get_str().map(|s| s.to_owned()))
	}

	/// Gets a `Byte` value for a given key, if that key exists and is stored as a byte.
	pub fn get_byte(&self, key: &str) -> Option<u8> {
		self.0.get(key).and_then(|v| v.get_byte())
	}

	/// Checks if the given key is either absent or references a list. Returns an error if it is present but not a list.
	pub fn check_optional_list(&self, key: &str) -> Result<()> {
		if let Some(value) = self.0.get(key) {
			if !value.is_list() {
				bail!("Item '{key}' is a '{}' and not a 'List'", value.get_type());
			}
		}
		Ok(())
	}

	/// Checks if the given key is either absent or references a string. Returns an error if it is present but not a string.
	pub fn check_optional_string(&self, key: &str) -> Result<()> {
		if let Some(value) = self.0.get(key) {
			if !value.is_string() {
				bail!("Item '{key}' is '{}' and not a 'String'", value.get_type());
			}
		}
		Ok(())
	}

	/// Checks if the given key is either absent or references a byte. Returns an error if it is present but not a byte.
	pub fn check_optional_byte(&self, key: &str) -> Result<()> {
		if let Some(value) = self.0.get(key) {
			if !value.is_byte() {
				bail!("Item '{key}' is '{}' and not a 'Byte'", value.get_type());
			}
		}
		Ok(())
	}

	/// Creates an iterator over the internal key-value pairs, where the value is returned as `JsonValue`.
	///
	/// The returned iterator yields `(String, JsonValue)` tuples.
	pub fn iter_json_values(&self) -> impl Iterator<Item = (String, JsonValue)> + '_ {
		self
			.0
			.iter()
			.map(|(k, v)| (k.to_owned(), v.as_json_value()))
	}
}

/// An enumeration representing allowed JSON value types in this module.
#[derive(Clone, Debug, PartialEq)]
pub enum TileJsonValue {
	/// A list of strings (originally from a JSON array).
	List(Vec<String>),
	/// A single string (originally from a JSON string).
	String(String),
	/// A single byte (stored as `u8`).
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

	/// Converts this `TileJsonValue` to a generic `JsonValue`.
	pub fn as_json_value(&self) -> JsonValue {
		match self {
			TileJsonValue::String(s) => JsonValue::from(s),
			TileJsonValue::List(l) => JsonValue::from(l),
			TileJsonValue::Byte(b) => JsonValue::from(*b),
		}
	}

	/// Returns a string describing the variant of this `TileJsonValue`.
	pub fn get_type(&self) -> &str {
		match self {
			TileJsonValue::String(_) => "String",
			TileJsonValue::List(_) => "List",
			TileJsonValue::Byte(_) => "Byte",
		}
	}

	/// Returns `true` if this value is a list.
	pub fn is_list(&self) -> bool {
		matches!(self, TileJsonValue::List(_))
	}

	/// Returns `true` if this value is a string.
	pub fn is_string(&self) -> bool {
		matches!(self, TileJsonValue::String(_))
	}

	/// Returns `true` if this value is a string.
	pub fn is_byte(&self) -> bool {
		matches!(self, TileJsonValue::Byte(_))
	}
}

impl TryFrom<&JsonValue> for TileJsonValue {
	type Error = anyhow::Error;

	/// Attempts to convert a reference to a `JsonValue` into a `TileJsonValue`.
	///
	/// # Errors
	///
	/// Returns an error if the conversion is not possible (e.g., invalid type).
	fn try_from(value: &JsonValue) -> Result<Self> {
		Ok(match value {
			JsonValue::String(s) => TileJsonValue::String(s.to_owned()),
			JsonValue::Array(a) => TileJsonValue::List(a.as_string_vec()?),
			JsonValue::Number(n) => {
				ensure!(n >= &0.0 && n <= &255.0, "Number out of byte range: {}", n);
				TileJsonValue::Byte(*n as u8)
			}
			_ => bail!("Invalid value type"),
		})
	}
}
