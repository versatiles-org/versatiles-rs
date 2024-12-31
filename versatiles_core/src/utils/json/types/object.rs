use super::{AsNumber, JsonArray, JsonValue};
use crate::utils::json::stringify::{escape_json_string, stringify};
use anyhow::Result;
use std::{collections::BTreeMap, fmt::Debug};

#[derive(Clone, Default, PartialEq)]
pub struct JsonObject(pub BTreeMap<String, JsonValue>);

impl JsonObject {
	pub fn object_assign(&mut self, object: JsonObject) -> Result<()> {
		for entry in object.0.into_iter() {
			self.0.insert(entry.0, entry.1);
		}
		Ok(())
	}
	pub fn object_set_key_value(&mut self, key: String, value: JsonValue) -> Result<()> {
		self.0.insert(key, value);
		Ok(())
	}
	pub fn object_get_value(&self, key: &str) -> Result<Option<&JsonValue>> {
		Ok(self.0.get(key))
	}
	pub fn get(&self, key: &str) -> Result<Option<&JsonValue>> {
		Ok(self.0.get(key))
	}
	pub fn get_string(&self, key: &str) -> Result<Option<String>> {
		self.get(key)?.map(JsonValue::as_string).transpose()
	}
	pub fn get_number<T>(&self, key: &str) -> Result<Option<T>>
	where
		T: AsNumber<T>,
	{
		self.get(key)?.map(JsonValue::as_number).transpose()
	}
	pub fn get_array(&self, key: &str) -> Result<Option<&JsonArray>> {
		self.get(key)?.map(JsonValue::as_array).transpose()
	}
	pub fn get_string_vec(&self, key: &str) -> Result<Option<Vec<String>>> {
		self
			.get_array(key)?
			.map(|array| array.as_string_vec())
			.transpose()
	}
	pub fn get_number_vec<T>(&self, key: &str) -> Result<Option<Vec<T>>>
	where
		T: AsNumber<T>,
	{
		self
			.get_array(key)?
			.map(|array| array.as_number_vec::<T>())
			.transpose()
	}
	pub fn get_number_array<T, const N: usize>(&self, key: &str) -> Result<Option<[T; N]>>
	where
		T: AsNumber<T>,
	{
		self
			.get_array(key)?
			.map(|array| array.as_number_array::<T, N>())
			.transpose()
	}

	pub fn set<T: Clone>(&mut self, key: &str, value: &T)
	where
		JsonValue: From<T>,
	{
		self
			.0
			.insert(key.to_owned(), JsonValue::from(value.clone()));
	}

	pub fn set_optional<T>(&mut self, key: &str, value: &Option<T>)
	where
		JsonValue: From<T>,
		T: Clone,
	{
		if let Some(v) = value {
			self.0.insert(key.to_owned(), JsonValue::from(v.clone()));
		}
	}

	pub fn stringify(&self) -> String {
		let items = self
			.0
			.iter()
			.map(|(key, value)| format!("\"{}\":{}", escape_json_string(key), stringify(value)))
			.collect::<Vec<_>>();
		format!("{{{}}}", items.join(","))
	}

	pub fn parse_str(json: &str) -> Result<JsonObject> {
		JsonValue::parse_str(json)?.to_object()
	}
}

impl Debug for JsonObject {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{:?}", self.0)
	}
}

impl<T> From<Vec<(&str, T)>> for JsonValue
where
	JsonValue: From<T>,
{
	fn from(input: Vec<(&str, T)>) -> Self {
		JsonValue::Object(JsonObject::from(input))
	}
}

impl<T> From<Vec<(&str, T)>> for JsonObject
where
	JsonValue: From<T>,
{
	fn from(input: Vec<(&str, T)>) -> Self {
		JsonObject(BTreeMap::from_iter(
			input
				.into_iter()
				.map(|(key, value)| (key.to_string(), JsonValue::from(value))),
		))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn test_from_vec_of_tuples() {
		let result: JsonValue = vec![("key1", "value1"), ("key2", "value2")].into();
		assert_eq!(
			result,
			JsonValue::Object(JsonObject(BTreeMap::from_iter(vec![
				("key1".to_string(), JsonValue::Str("value1".to_string())),
				("key2".to_string(), JsonValue::Str("value2".to_string())),
			])))
		);
	}
}
