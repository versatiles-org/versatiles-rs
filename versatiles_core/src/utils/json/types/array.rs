use super::{AsNumber, JsonValue};
use crate::utils::json::stringify::stringify;
use anyhow::{anyhow, Result};
use std::fmt::Debug;

#[derive(Clone, Default, PartialEq)]
pub struct JsonArray(pub Vec<JsonValue>);

impl JsonArray {
	pub fn stringify(&self) -> String {
		let items = self.0.iter().map(stringify).collect::<Vec<_>>();
		format!("[{}]", items.join(","))
	}
	pub fn as_string_vec(&self) -> Result<Vec<String>> {
		self.0.iter().map(JsonValue::as_string).collect::<Result<Vec<_>>>()
	}
	pub fn as_number_vec<T>(&self) -> Result<Vec<T>>
	where
		T: AsNumber<T>,
	{
		self.0.iter().map(JsonValue::as_number).collect::<Result<Vec<T>>>()
	}
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

impl<T> From<T> for JsonValue
where
	JsonArray: From<T>,
{
	fn from(input: T) -> Self {
		JsonValue::Array(JsonArray::from(input))
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
		JsonArray(Vec::from_iter(input.into_iter().map(|v| JsonValue::from(*v))))
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

		assert_eq!(format!("{:?}", array), r#"[String("debug"), Number(42.0)]"#);
	}

	#[test]
	fn test_from_vec() {
		let json_array = JsonArray::from(vec![1, 2, 3]);
		assert_eq!(json_array.0.len(), 3);
		assert_eq!(json_array.0[0], JsonValue::from(1));
	}
}
