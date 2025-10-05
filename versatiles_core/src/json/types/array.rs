//! JSON array type and utilities for serializing, deserializing, and converting to Rust types.
use crate::json::*;
use anyhow::{Result, anyhow};
use std::fmt::Debug;

#[derive(Clone, Default, PartialEq)]
/// A JSON array, backed by a `Vec<JsonValue>`.
///
/// Provides methods for stringifying the array in compact or pretty formats,
/// and converting elements to Rust types (e.g., strings, numbers).
pub struct JsonArray(pub Vec<JsonValue>);

impl JsonArray {
	/// Serialize the JSON array to a compact string without extra whitespace.
	///
	/// # Examples
	///
	/// ```rust
	/// use versatiles_core::json::{JsonArray, JsonValue};
	/// let arr = JsonArray(vec![JsonValue::from(1), JsonValue::from(2)]);
	/// assert_eq!(arr.stringify(), "[1,2]");
	/// ```
	pub fn stringify(&self) -> String {
		let items = self.0.iter().map(stringify).collect::<Vec<_>>();
		format!("[{}]", items.join(","))
	}

	/// Serialize the array to a single-line, pretty-printed string with spaces.
	///
	/// E.g., `[ 1, 2, 3 ]`.
	pub fn stringify_pretty_single_line(&self) -> String {
		let items = self.0.iter().map(stringify_pretty_single_line).collect::<Vec<_>>();
		format!("[ {} ]", items.join(", "))
	}

	/// Serialize the array to a multi-line, pretty-printed string.
	///
	/// `max_width` controls when to break lines, and `depth` sets the indentation level.
	pub fn stringify_pretty_multi_line(&self, max_width: usize, depth: usize) -> String {
		let indent = "  ".repeat(depth);
		let items = self
			.0
			.iter()
			.map(|value| {
				format!(
					"{indent}  {}",
					stringify_pretty_multi_line(value, max_width, depth + 1, depth * 2 + 2)
				)
			})
			.collect::<Vec<_>>();
		format!("[\n{}\n{}]", items.join(",\n"), indent)
	}

	/// Convert all elements to Rust `String`s, returning an error if any element is not a string.
	pub fn as_string_vec(&self) -> Result<Vec<String>> {
		self.0.iter().map(JsonValue::as_string).collect::<Result<Vec<_>>>()
	}

	/// Convert all elements to numbers of type `T`, returning an error if any element is not numeric.
	pub fn as_number_vec<T>(&self) -> Result<Vec<T>>
	where
		T: AsNumber<T>,
	{
		self.0.iter().map(JsonValue::as_number).collect::<Result<Vec<T>>>()
	}

	/// Get a reference to the underlying `Vec<JsonValue>`.
	pub fn as_vec(&self) -> &Vec<JsonValue> {
		&self.0
	}

	/// Convert elements to a fixed-size array of numbers, returning an error on mismatch or non-numeric elements.
	pub fn as_number_array<T, const N: usize>(&self) -> Result<[T; N]>
	where
		T: AsNumber<T>,
	{
		self
			.as_number_vec::<T>()?
			.try_into()
			.map_err(|e: Vec<T>| anyhow!("vector length mismatch {} != {}", e.len(), N))
	}
}

impl Debug for JsonArray {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{:?}", self.0)
	}
}

impl<T> From<Vec<T>> for JsonArray
where
	JsonValue: From<T>,
{
	fn from(input: Vec<T>) -> Self {
		JsonArray(Vec::from_iter(input.into_iter().map(JsonValue::from)))
	}
}

impl<T> From<&Vec<T>> for JsonArray
where
	JsonValue: From<T>,
	T: Clone,
{
	fn from(input: &Vec<T>) -> Self {
		JsonArray(Vec::from_iter(input.iter().map(|v| JsonValue::from(v.clone()))))
	}
}

impl<T, const N: usize> From<&[T; N]> for JsonArray
where
	JsonValue: From<T>,
	T: Copy,
{
	fn from(input: &[T; N]) -> Self {
		JsonArray(Vec::from_iter(input.iter().map(|v| JsonValue::from(*v))))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_stringify() {
		let array = JsonArray(vec![
			JsonValue::from("hello"),
			JsonValue::from(42.0),
			JsonValue::from(true),
		]);

		assert_eq!(array.stringify(), r#"["hello",42,true]"#);
	}

	#[test]
	fn test_as_string_vec() -> Result<()> {
		let array = JsonArray::from(vec!["hello", "world"]);

		assert_eq!(array.as_string_vec()?, vec!["hello", "world"]);

		// Test with a non-string element
		assert_eq!(
			JsonArray::from(vec![1, 2]).as_string_vec().unwrap_err().to_string(),
			"expected a string, found a number"
		);

		Ok(())
	}

	#[test]
	fn test_as_number_vec() -> Result<()> {
		let array = JsonArray::from(vec![1.2, 3.4, 5.6]);

		assert_eq!(array.as_number_vec::<f64>()?, vec![1.2, 3.4, 5.6]);
		assert_eq!(array.as_number_vec::<u8>()?, vec![1, 3, 5]);
		assert_eq!(array.as_number_vec::<i32>()?, vec![1, 3, 5]);

		// Test with a non-number element
		assert_eq!(
			JsonArray::from(vec!["a"])
				.as_number_vec::<f64>()
				.unwrap_err()
				.to_string(),
			"expected a number, found a string"
		);

		Ok(())
	}

	#[test]
	fn test_as_number_array() -> Result<()> {
		let array = JsonArray::from(vec![1.2, 3.4, 5.6]);

		let number_array: [f64; 3] = array.as_number_array()?;
		assert_eq!(number_array, [1.2, 3.4, 5.6]);

		let number_array: [u8; 3] = array.as_number_array()?;
		assert_eq!(number_array, [1, 3, 5]);

		// Test with incorrect length
		assert_eq!(
			array.as_number_array::<f64, 2>().unwrap_err().to_string(),
			"vector length mismatch 3 != 2"
		);

		// Test with a non-number element
		assert_eq!(
			JsonArray::from(vec!["a"])
				.as_number_array::<f64, 1>()
				.unwrap_err()
				.to_string(),
			"expected a number, found a string"
		);

		Ok(())
	}

	#[test]
	fn test_debug_impl() {
		let array = JsonArray(vec![JsonValue::from("debug"), JsonValue::from(42.0)]);

		assert_eq!(format!("{array:?}"), r#"[String("debug"), Number(42.0)]"#);
	}

	#[test]
	fn test_from_vec() {
		let json_array = JsonArray::from(vec![1, 2, 3]);
		assert_eq!(json_array.0.len(), 3);
		assert_eq!(json_array.0[0], JsonValue::from(1));
	}

	#[test]
	fn test_stringify_pretty_single_line() {
		let array = JsonArray(vec![JsonValue::from("hello"), JsonValue::from(42.0)]);
		assert_eq!(array.stringify_pretty_single_line(), "[ \"hello\", 42 ]");
	}

	#[test]
	fn test_stringify_pretty_multi_line() {
		let array = JsonArray(vec![JsonValue::from("a"), JsonValue::from("b")]);
		let expected = "[\n  \"a\",\n  \"b\"\n]";
		assert_eq!(array.stringify_pretty_multi_line(80, 0), expected);
	}

	#[test]
	fn test_as_vec() {
		let array = JsonArray(vec![JsonValue::from(true)]);
		// as_vec should return a reference to the internal vector
		assert_eq!(array.as_vec(), &array.0);
	}

	#[test]
	fn test_from_ref_vec() {
		let v = vec![1, 2, 3];
		let arr = JsonArray::from(&v);
		assert_eq!(arr.0, vec![JsonValue::from(1), JsonValue::from(2), JsonValue::from(3),]);
	}

	#[test]
	fn test_from_array_ref() {
		let slice = [4, 5, 6];
		let arr = JsonArray::from(&slice);
		assert_eq!(arr.0, vec![JsonValue::from(4), JsonValue::from(5), JsonValue::from(6),]);
	}
}
