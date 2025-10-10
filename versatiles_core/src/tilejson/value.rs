use crate::json::JsonValue;
use anyhow::{Result, bail, ensure};
use std::collections::BTreeMap;

/// A map storing string keys and their associated typed JSON values.
///
/// By default, this map includes the key `"tilejson"` with a default value of
/// `"3.0.0"`, mirroring a typical `TileJSON` requirement.
#[derive(Clone, Debug, PartialEq)]
pub struct TileJsonValues(BTreeMap<String, TileJsonValue>);

impl TileJsonValues {
	/// Inserts a key-value pair into the internal `BTreeMap`,
	/// converting the [`JsonValue`] into a [`TileJsonValue`].
	///
	/// # Errors
	///
	/// Returns an error if the provided `JsonValue` cannot be converted into
	/// a `TileJsonValue` (e.g., out-of-range numeric value).
	pub fn insert(&mut self, key: &str, value: &JsonValue) -> Result<()> {
		self.0.insert(key.to_owned(), TileJsonValue::try_from(value)?);
		Ok(())
	}

	/// Returns a reference to the inner `str` value if this key exists as a string variant,
	/// otherwise returns `None`.
	///
	/// This method does **not** copy or clone data, returning a `&str` slice instead.
	pub fn get_str(&self, key: &str) -> Option<&str> {
		self.0.get(key).and_then(|v| v.get_str())
	}

	/// Returns a cloned `String` if this key exists as a string variant,
	/// otherwise returns `None`.
	///
	/// This method *does* allocate, returning an owned `String`.
	pub fn get_string(&self, key: &str) -> Option<String> {
		self.0.get(key).and_then(|v| v.get_str().map(ToOwned::to_owned))
	}

	/// Returns a `u8` if this key exists as a byte variant, otherwise returns `None`.
	pub fn get_byte(&self, key: &str) -> Option<u8> {
		self.0.get(key).and_then(TileJsonValue::get_byte)
	}

	/// Checks if the given `key` is either absent or references a list (`Vec<String>`).
	/// Returns an error if it is present but not a list.
	pub fn check_optional_list(&self, key: &str) -> Result<()> {
		if let Some(value) = self.0.get(key)
			&& !value.is_list()
		{
			bail!("Item '{key}' is a '{}' and not a 'List'", value.get_type());
		}
		Ok(())
	}

	/// Checks if the given `key` is either absent or references a string.
	/// Returns an error if it is present but not a string.
	pub fn check_optional_string(&self, key: &str) -> Result<()> {
		if let Some(value) = self.0.get(key)
			&& !value.is_string()
		{
			bail!("Item '{key}' is '{}' and not a 'String'", value.get_type());
		}
		Ok(())
	}

	/// Checks if the given `key` is either absent or references a byte (`u8`).
	/// Returns an error if it is present but not a byte.
	pub fn check_optional_byte(&self, key: &str) -> Result<()> {
		if let Some(value) = self.0.get(key)
			&& !value.is_byte()
		{
			bail!("Item '{key}' is '{}' and not a 'Byte'", value.get_type());
		}
		Ok(())
	}

	/// Returns an iterator over `(String, JsonValue)` pairs, where
	/// each `JsonValue` is the generic form of the stored [`TileJsonValue`].
	///
	/// Use this to transform `TileJsonValues` back into a generic JSON structure.
	pub fn iter_json_values(&self) -> impl Iterator<Item = (String, JsonValue)> + '_ {
		self.0.iter().map(|(k, v)| (k.clone(), v.as_json_value()))
	}

	/// Updates or inserts a byte (`u8`) for the given `key`.
	/// The provided `update` closure receives the current value (if any)
	/// and returns the new byte value to be stored.
	pub fn update_byte<F>(&mut self, key: &str, update: F)
	where
		F: FnOnce(Option<u8>) -> u8,
	{
		let new_val = update(self.0.get(key).and_then(TileJsonValue::get_byte));
		self.0.insert(key.to_owned(), TileJsonValue::Byte(new_val));
	}

	pub fn set<T>(&mut self, key: &str, value: T)
	where
		TileJsonValue: From<T>,
	{
		self.0.insert(key.to_owned(), TileJsonValue::from(value));
	}
}

impl From<u8> for TileJsonValue {
	fn from(value: u8) -> Self {
		TileJsonValue::Byte(value)
	}
}

impl Default for TileJsonValues {
	/// By default, we create a map with one key: `"tilejson"`,
	/// initialized to the string `"3.0.0"`.
	fn default() -> Self {
		let mut map = BTreeMap::new();
		map.insert("tilejson".to_string(), TileJsonValue::String("3.0.0".to_owned()));
		TileJsonValues(map)
	}
}

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
mod tests {
	use super::*;

	#[test]
	fn default_includes_tilejson() {
		let default_values = TileJsonValues::default();
		assert_eq!(default_values.get_string("tilejson"), Some("3.0.0".to_owned()));
	}

	#[test]
	fn insert_and_retrieve_string() -> Result<()> {
		let mut tv = TileJsonValues::default();
		tv.insert("name", &JsonValue::from("Test Layer"))?;
		assert_eq!(tv.get_string("name"), Some("Test Layer".to_owned()));
		Ok(())
	}

	#[test]
	fn insert_and_retrieve_byte() -> Result<()> {
		let mut tv = TileJsonValues::default();
		tv.insert("maxzoom", &JsonValue::from(12.9_f64))?;
		assert_eq!(tv.get_byte("maxzoom"), Some(12));
		Ok(())
	}

	#[test]
	fn insert_out_of_range_byte() {
		let mut tv = TileJsonValues::default();
		// 999.0 is out of byte range
		let result = tv.insert("zoom", &JsonValue::from(999_f64));
		assert!(result.is_err());
	}

	#[test]
	fn insert_and_retrieve_list() -> Result<()> {
		let mut tv = TileJsonValues::default();
		let json_array = JsonValue::from(vec!["field1", "field2"]);

		tv.insert("fields", &json_array)?;
		let val = tv.0.get("fields").unwrap();
		assert!(val.is_list());

		match val {
			TileJsonValue::List(list) => assert_eq!(list, &["field1", "field2"]),
			_ => panic!("Expected a list"),
		}
		Ok(())
	}

	#[test]
	fn check_optional_list() {
		let mut tv = TileJsonValues::default();
		tv.insert("mylist", &JsonValue::from(vec!["a", "b"])).unwrap();
		assert!(tv.check_optional_list("mylist").is_ok());

		// If we overwrite "mylist" with a string, it should fail
		tv.insert("mylist", &JsonValue::from("not a list")).unwrap();
		assert!(tv.check_optional_list("mylist").is_err());
	}

	#[test]
	fn check_optional_string() {
		let mut tv = TileJsonValues::default();
		tv.insert("desc", &JsonValue::from("description")).unwrap();
		assert!(tv.check_optional_string("desc").is_ok());

		// If we overwrite "desc" with a list, it should fail
		tv.insert("desc", &JsonValue::from(vec!["oops"])).unwrap();
		assert!(tv.check_optional_string("desc").is_err());
	}

	#[test]
	fn check_optional_byte() {
		let mut tv = TileJsonValues::default();
		tv.insert("opacity", &JsonValue::from(123_f64)).unwrap();
		assert!(tv.check_optional_byte("opacity").is_ok());

		// Overwrite with a string -> fails
		tv.insert("opacity", &JsonValue::from("should be a number")).unwrap();
		assert!(tv.check_optional_byte("opacity").is_err());
	}

	#[test]
	fn update_byte_test() {
		let mut tv = TileJsonValues::default();
		// No existing value => default to 0
		tv.update_byte("zoom", |maybe| maybe.unwrap_or(0).max(10));
		assert_eq!(tv.get_byte("zoom"), Some(10));

		// Existing value => modify existing
		tv.update_byte("zoom", |maybe| maybe.unwrap_or(0).max(20));
		assert_eq!(tv.get_byte("zoom"), Some(20));
	}

	#[test]
	fn iter_json_values_test() -> Result<()> {
		let mut tv = TileJsonValues::default();
		tv.insert("alpha", &JsonValue::from(1_f64))?;
		tv.insert("name", &JsonValue::from("Layer"))?;

		let json_values: BTreeMap<String, JsonValue> = tv.iter_json_values().collect();

		// "tilejson" is always present by default
		assert!(json_values.contains_key("tilejson"));
		assert_eq!(json_values["tilejson"], JsonValue::from("3.0.0"));
		assert_eq!(json_values["alpha"], JsonValue::from(1_f64));
		assert_eq!(json_values["name"], JsonValue::from("Layer"));

		Ok(())
	}
}
