//! JSON object type and utilities for serializing, deserializing, and converting JSON to Rust types.
use crate::json::*;
use anyhow::Result;
use std::{
	collections::BTreeMap,
	fmt::{Debug, Display},
};

/// A JSON object backed by a `BTreeMap<String, JsonValue>`.
///
/// Provides methods to assign, get, set, and serialize JSON object data.
#[derive(Clone, Default, PartialEq)]
pub struct JsonObject(pub BTreeMap<String, JsonValue>);

impl JsonObject {
	/// Create a new, empty `JsonObject`.
	#[must_use]
	pub fn new() -> Self {
		Self(BTreeMap::new())
	}

	/// Merge entries from another `JsonObject` into this one, overwriting existing keys.
	pub fn assign(&mut self, object: JsonObject) -> Result<()> {
		for entry in object.0 {
			self.0.insert(entry.0, entry.1);
		}
		Ok(())
	}

	/// Get a reference to the raw `JsonValue` for the specified key, if present.
	#[must_use]
	pub fn get(&self, key: &str) -> Option<&JsonValue> {
		self.0.get(key)
	}

	/// Retrieve a string value for the specified key, returning `None` if missing or not a string.
	pub fn get_string(&self, key: &str) -> Result<Option<String>> {
		self.get(key).map(JsonValue::to_string).transpose()
	}

	pub fn get_object(&self, key: &str) -> Result<Option<&JsonObject>> {
		self.get(key).map(JsonValue::as_object).transpose()
	}

	/// Retrieve a numeric value of type `T` for the specified key, returning `None` if missing or not numeric.
	pub fn get_number(&self, key: &str) -> Result<Option<f64>> {
		self.get(key).map(JsonValue::as_number).transpose()
	}

	/// Retrieve a `JsonArray` reference for the specified key, if present and an array.
	pub fn get_array(&self, key: &str) -> Result<Option<&JsonArray>> {
		self.get(key).map(JsonValue::as_array).transpose()
	}

	/// Retrieve a `Vec<String>` from the array at the specified key, if present and all elements are strings.
	pub fn get_string_vec(&self, key: &str) -> Result<Option<Vec<String>>> {
		self
			.get_array(key)?
			.map(super::array::JsonArray::as_string_vec)
			.transpose()
	}

	/// Retrieve a `Vec<T>` from the array at the specified key, if present and all elements are numeric.
	pub fn get_number_vec(&self, key: &str) -> Result<Option<Vec<f64>>> {
		self
			.get_array(key)?
			.map(super::array::JsonArray::as_number_vec)
			.transpose()
	}

	/// Retrieve a fixed-size array `[T; N]` from the array at the specified key, if present and all elements are numeric.
	pub fn get_number_array<const N: usize>(&self, key: &str) -> Result<Option<[f64; N]>> {
		self
			.get_array(key)?
			.map(super::array::JsonArray::as_number_array::<N>)
			.transpose()
	}

	/// Set the specified key to the given value, converting it into a `JsonValue`.
	pub fn set<T: Clone>(&mut self, key: &str, value: T)
	where
		JsonValue: From<T>,
	{
		self.0.insert(key.to_owned(), JsonValue::from(value));
	}

	/// Set the specified key only if the provided `Option` is `Some`, converting it into a `JsonValue`.
	pub fn set_optional<T>(&mut self, key: &str, value: &Option<T>)
	where
		JsonValue: From<T>,
		T: Clone,
	{
		if let Some(v) = value {
			self.0.insert(key.to_owned(), JsonValue::from(v.clone()));
		}
	}

	/// Serialize this `JsonObject` into a compact JSON string without extra whitespace.
	#[must_use]
	pub fn stringify(&self) -> String {
		let items = self
			.0
			.iter()
			.map(|(key, value)| format!("\"{}\":{}", escape_json_string(key), stringify(value)))
			.collect::<Vec<_>>();
		format!("{{{}}}", items.join(","))
	}

	/// Serialize this `JsonObject` into a single-line, pretty-printed JSON string with spaces.
	#[must_use]
	pub fn stringify_pretty_single_line(&self) -> String {
		let items = self
			.0
			.iter()
			.map(|(key, value)| {
				format!(
					"\"{}\": {}",
					escape_json_string(key),
					stringify_pretty_single_line(value)
				)
			})
			.collect::<Vec<_>>();
		format!("{{ {} }}", items.join(", "))
	}

	/// Serialize this `JsonObject` into a multi-line, pretty-printed JSON string with indentation.
	///
	/// `max_width` controls when to wrap lines, and `depth` sets the base indentation level.
	#[must_use]
	pub fn stringify_pretty_multi_line(&self, max_width: usize, depth: usize) -> String {
		let indent = "  ".repeat(depth);
		let items = self
			.0
			.iter()
			.map(|(key, value)| {
				let key_string = format!("{}  \"{}\": ", indent, escape_json_string(key));
				format!(
					"{key_string}{}",
					stringify_pretty_multi_line(value, max_width, depth + 1, key_string.len())
				)
			})
			.collect::<Vec<_>>();
		format!("{{\n{}\n{}}}", items.join(",\n"), indent)
	}

	/// Parse a JSON string into a `JsonObject`, returning an error on invalid JSON or non-object root.
	pub fn parse_str(json: &str) -> Result<JsonObject> {
		JsonValue::parse_str(json)?.into_object()
	}

	/// Return an iterator over key-value pairs in this `JsonObject` in insertion order.
	pub fn iter(&self) -> impl Iterator<Item = (&String, &JsonValue)> {
		self.0.iter()
	}
}

impl Debug for JsonObject {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{:?}", self.0)
	}
}

impl Display for JsonObject {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.stringify())
	}
}

/// Convert a `Vec<(&str, T)>` into a `JsonValue::Object` by converting into a `JsonObject`.
impl<T> From<Vec<(&str, T)>> for JsonValue
where
	JsonValue: From<T>,
{
	fn from(input: Vec<(&str, T)>) -> Self {
		JsonValue::Object(JsonObject::from(input))
	}
}

/// Convert a `Vec<(&str, T)>` into a `JsonObject`, consuming the vector of key-value pairs.
impl<T> From<Vec<(&str, T)>> for JsonObject
where
	JsonValue: From<T>,
{
	fn from(input: Vec<(&str, T)>) -> Self {
		JsonObject(
			input
				.into_iter()
				.map(|(key, value)| (key.to_string(), JsonValue::from(value)))
				.collect(),
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_object_assign() {
		let mut obj1 = JsonObject::from(vec![("key1", "value1")]);
		let obj2 = JsonObject::from(vec![("key2", "value2"), ("key3", "value3")]);
		obj1.assign(obj2).unwrap();

		assert_eq!(
			obj1,
			JsonObject::from(vec![("key1", "value1"), ("key2", "value2"), ("key3", "value3"),])
		);
	}

	#[test]
	fn test_object_set_key_value() {
		let mut obj = JsonObject::default();
		obj.set("key", JsonValue::from("value"));

		assert_eq!(
			obj,
			JsonObject(BTreeMap::from_iter(vec![("key".to_string(), JsonValue::from("value"))]))
		);
	}

	#[test]
	fn test_get() {
		let obj = JsonObject::from(vec![("key", "value")]);

		assert_eq!(obj.get("key"), Some(&JsonValue::from("value")));
	}

	#[test]
	fn test_get_string() {
		let obj = JsonObject::from(vec![("key", "value")]);
		let value = obj.get_string("key").unwrap();

		assert_eq!(value, Some("value".to_string()));

		let missing = obj.get_string("missing").unwrap();
		assert_eq!(missing, None);
	}

	#[test]
	fn test_get_number() {
		let obj = JsonObject::from(vec![("key", 42)]);
		let value = obj.get_number("key").unwrap();
		assert_eq!(value, Some(42.0));

		let missing = obj.get_number("missing").unwrap();
		assert_eq!(missing, None);
	}

	#[test]
	fn test_get_array() {
		let array = JsonArray(vec![JsonValue::from("item1"), JsonValue::from("item2")]);
		let obj = JsonObject::from(vec![("key", JsonValue::Array(array.clone()))]);

		let value = obj.get_array("key").unwrap();
		assert_eq!(value, Some(&array));
	}

	#[test]
	fn test_get_string_vec() {
		let array = JsonArray(vec![JsonValue::from("item1"), JsonValue::from("item2")]);
		let obj = JsonObject::from(vec![("key", JsonValue::Array(array))]);

		let value = obj.get_string_vec("key").unwrap();
		assert_eq!(value, Some(vec!["item1".to_string(), "item2".to_string()]));
	}

	#[test]
	fn test_get_number_vec() {
		let array = JsonArray::from(vec![1, 2, 3]);
		let obj = JsonObject::from(vec![("key", JsonValue::Array(array))]);
		assert_eq!(obj.get_number_vec("key").unwrap(), Some(vec![1.0, 2.0, 3.0]));
	}

	#[test]
	fn test_get_number_array() {
		let array = JsonArray::from(vec![1, 2, 3]);
		let obj = JsonObject::from(vec![("key", JsonValue::Array(array))]);

		let value = obj.get_number_array("key").unwrap();
		assert_eq!(value, Some([1.0, 2.0, 3.0]));
	}

	#[test]
	fn test_set_and_set_optional() {
		let mut obj = JsonObject::default();
		obj.set("key1", 42);
		obj.set_optional("key2", &Some(84));
		obj.set_optional::<i32>("key3", &None);

		assert_eq!(
			obj,
			JsonObject(BTreeMap::from_iter(vec![
				("key1".to_string(), JsonValue::from(42)),
				("key2".to_string(), JsonValue::from(84)),
			]))
		);
	}

	#[test]
	fn test_stringify() {
		let obj = JsonObject::from(vec![
			("key1", JsonValue::from("value1")),
			("key2", JsonValue::from(42)),
			("key3", JsonValue::from(vec![1, 2])),
		]);

		let json_string = obj.stringify();
		let expected = r#"{"key1":"value1","key2":42,"key3":[1,2]}"#;

		assert_eq!(json_string, expected);
	}

	#[test]
	fn test_parse_str() {
		let json = r#"{"key1":"value1","key2":42,"key3":[1,2]}"#;
		let parsed = JsonObject::parse_str(json).unwrap();

		let expected = JsonObject::from(vec![
			("key1", JsonValue::from("value1")),
			("key2", JsonValue::from(42)),
			("key3", JsonValue::from(vec![1, 2])),
		]);

		assert_eq!(parsed, expected);
	}

	#[test]
	fn test_stringify_pretty_single_line() {
		let obj = JsonObject::from(vec![("key1", JsonValue::from("value1")), ("key2", JsonValue::from(2))]);
		let s = obj.stringify_pretty_single_line();
		assert_eq!(s, "{ \"key1\": \"value1\", \"key2\": 2 }");
	}

	#[test]
	fn test_stringify_pretty_multi_line() {
		let obj = JsonObject::from(vec![("a", JsonValue::from(1)), ("b", JsonValue::from(2))]);
		let s = obj.stringify_pretty_multi_line(80, 0);
		let expected = "{\n  \"a\": 1,\n  \"b\": 2\n}";
		assert_eq!(s, expected);
	}

	#[test]
	fn test_iter_and_order() {
		let obj = JsonObject::from(vec![("x", "y"), ("z", "w")]);
		let pairs: Vec<(&String, &JsonValue)> = obj.iter().collect();
		let keys: Vec<&String> = pairs.iter().map(|(k, _)| *k).collect();
		assert_eq!(keys, vec![&"x".to_string(), &"z".to_string()]);
	}

	#[test]
	fn test_debug_fmt() {
		let obj = JsonObject::from(vec![("k", 1)]);
		let expected_map: std::collections::BTreeMap<_, _> =
			vec![("k".to_string(), JsonValue::from(1))].into_iter().collect();
		assert_eq!(format!("{obj:?}"), format!("{expected_map:?}"));
	}

	#[test]
	fn test_from_vec_for_jsonvalue() {
		let input = vec![("foo", 3), ("bar", 4)];
		let jv: JsonValue = input.clone().into();
		if let JsonValue::Object(obj) = jv {
			assert_eq!(obj.get_number("foo").unwrap(), Some(3.0));
			assert_eq!(obj.get_number("bar").unwrap(), Some(4.0));
		} else {
			panic!("Expected JsonValue::Object variant");
		}
	}

	#[test]
	fn test_get_missing_variants() {
		let obj = JsonObject::default();
		assert_eq!(obj.get_array("missing").unwrap(), None);
		assert_eq!(obj.get_string_vec("missing").unwrap(), None);
		assert_eq!(obj.get_number_vec("missing").unwrap(), None);
		assert_eq!(obj.get_number_array::<3>("missing").unwrap(), None);
	}
}
