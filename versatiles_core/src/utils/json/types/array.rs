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
		self
			.0
			.iter()
			.map(JsonValue::as_string)
			.collect::<Result<Vec<_>>>()
	}
	pub fn as_number_vec<T>(&self) -> Result<Vec<T>>
	where
		T: AsNumber<T>,
	{
		self
			.0
			.iter()
			.map(JsonValue::as_number)
			.collect::<Result<Vec<T>>>()
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

impl<T> From<Vec<T>> for JsonValue
where
	JsonValue: From<T>,
{
	fn from(input: Vec<T>) -> Self {
		JsonValue::Array(JsonArray(Vec::from_iter(
			input.into_iter().map(JsonValue::from),
		)))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::utils::JsonValue;

	#[test]
	fn test_from_vec_of_json_values() {
		let result: JsonValue = vec![
			JsonValue::from("value1"),
			JsonValue::from(true),
			JsonValue::from(23.42),
		]
		.into();
		assert_eq!(
			result,
			JsonValue::Array(JsonArray(vec![
				JsonValue::Str("value1".to_string()),
				JsonValue::Boolean(true),
				JsonValue::Num(23.42),
			]))
		);
	}

	#[test]
	fn test_from_vec_of_str() {
		let result: JsonValue = vec!["value1", "value2", "value3"].into();
		assert_eq!(
			result,
			JsonValue::Array(JsonArray(vec![
				JsonValue::Str("value1".to_string()),
				JsonValue::Str("value2".to_string()),
				JsonValue::Str("value3".to_string()),
			]))
		);
	}
}
