use super::{AsNumber, JsonArray, JsonObject};
use crate::{
	types::Blob,
	utils::{json::stringify::stringify, parse_json_str},
};
use anyhow::{bail, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
	Array(JsonArray),
	Boolean(bool),
	Null,
	Num(f64),
	Object(JsonObject),
	Str(String),
}

impl JsonValue {
	pub fn parse_str(json: &str) -> Result<JsonValue> {
		parse_json_str(json)
	}
	pub fn parse_blob(blob: &Blob) -> Result<JsonValue> {
		parse_json_str(blob.as_str())
	}

	pub fn type_as_str(&self) -> &str {
		use JsonValue::*;
		match self {
			Array(_) => "array",
			Boolean(_) => "boolean",
			Null => "null",
			Num(_) => "number",
			Object(_) => "object",
			Str(_) => "string",
		}
	}
	pub fn stringify(&self) -> String {
		stringify(self)
	}

	pub fn new_array() -> JsonValue {
		JsonValue::Array(JsonArray::default())
	}
	pub fn new_object() -> JsonValue {
		JsonValue::Object(JsonObject::default())
	}

	pub fn as_array(&self) -> Result<&JsonArray> {
		if let JsonValue::Array(array) = self {
			Ok(array)
		} else {
			bail!("self must be a JSON array")
		}
	}
	pub fn to_array(self) -> Result<JsonArray> {
		if let JsonValue::Array(array) = self {
			Ok(array)
		} else {
			bail!("self must be a JSON array")
		}
	}
	pub fn as_object(&self) -> Result<&JsonObject> {
		if let JsonValue::Object(object) = self {
			Ok(object)
		} else {
			bail!("self must be a JSON object")
		}
	}
	pub fn to_object(self) -> Result<JsonObject> {
		if let JsonValue::Object(object) = self {
			Ok(object)
		} else {
			bail!("self must be a JSON object")
		}
	}
	pub fn as_string(&self) -> Result<String> {
		match self {
			JsonValue::Str(text) => Ok(text.to_owned()),
			_ => bail!("expected a string, found a {}", self.type_as_str()),
		}
	}
	pub fn as_str(&self) -> Result<&str> {
		match self {
			JsonValue::Str(text) => Ok(text),
			_ => bail!("expected a string, found a {}", self.type_as_str()),
		}
	}
	pub fn as_number<T>(&self) -> Result<T>
	where
		T: AsNumber<T>,
	{
		if let JsonValue::Num(val) = self {
			Ok(<T as AsNumber<T>>::convert(*val))
		} else {
			bail!("expected a number, found a {}", self.type_as_str())
		}
	}
}

impl From<&str> for JsonValue {
	fn from(input: &str) -> Self {
		JsonValue::Str(input.to_string())
	}
}

impl From<String> for JsonValue {
	fn from(input: String) -> Self {
		JsonValue::Str(input)
	}
}

impl From<bool> for JsonValue {
	fn from(input: bool) -> Self {
		JsonValue::Boolean(input)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_from_str() {
		let input = "hello";
		let result: JsonValue = input.into();
		assert_eq!(result, JsonValue::Str("hello".to_string()));
	}

	#[test]
	fn test_from_string() {
		let result: JsonValue = String::from("hello").into();
		assert_eq!(result, JsonValue::Str("hello".to_string()));
	}

	#[test]
	fn test_from_bool() {
		let result: JsonValue = true.into();
		assert_eq!(result, JsonValue::Boolean(true));

		let result: JsonValue = false.into();
		assert_eq!(result, JsonValue::Boolean(false));
	}

	#[test]
	fn test_from_f64() {
		let result: JsonValue = 23.42.into();
		assert_eq!(result, JsonValue::Num(23.42));
	}

	#[test]
	fn test_from_i32() {
		let result: JsonValue = 42.into();
		assert_eq!(result, JsonValue::Num(42.0));
	}
}
