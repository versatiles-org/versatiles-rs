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
	Number(f64),
	Object(JsonObject),
	String(String),
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
			Number(_) => "number",
			Object(_) => "object",
			String(_) => "string",
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
			JsonValue::String(text) => Ok(text.to_owned()),
			_ => bail!("expected a string, found a {}", self.type_as_str()),
		}
	}
	pub fn as_str(&self) -> Result<&str> {
		match self {
			JsonValue::String(text) => Ok(text),
			_ => bail!("expected a string, found a {}", self.type_as_str()),
		}
	}
	pub fn as_number<T>(&self) -> Result<T>
	where
		T: AsNumber<T>,
	{
		if let JsonValue::Number(val) = self {
			Ok(<T as AsNumber<T>>::convert(*val))
		} else {
			bail!("expected a number, found a {}", self.type_as_str())
		}
	}
}

impl From<&str> for JsonValue {
	fn from(input: &str) -> Self {
		JsonValue::String(input.to_string())
	}
}

impl From<String> for JsonValue {
	fn from(input: String) -> Self {
		JsonValue::String(input)
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
	use crate::types::Blob;

	#[test]
	fn test_from_str() {
		let input = "hello";
		let result: JsonValue = input.into();
		assert_eq!(result, JsonValue::String("hello".to_string()));
	}

	#[test]
	fn test_from_string() {
		let result: JsonValue = String::from("hello").into();
		assert_eq!(result, JsonValue::String("hello".to_string()));
	}

	#[test]
	fn test_from_bool() {
		assert_eq!(JsonValue::from(true), JsonValue::Boolean(true));
		assert_eq!(JsonValue::from(false), JsonValue::Boolean(false));
	}

	#[test]
	fn test_from_f64() {
		let result: JsonValue = 23.42.into();
		assert_eq!(result, JsonValue::Number(23.42));
	}

	#[test]
	fn test_from_i32() {
		let result: JsonValue = 42.into();
		assert_eq!(result, JsonValue::Number(42.0));
	}

	#[test]
	fn test_from_vec_of_json_values() {
		let result = JsonValue::from(vec![
			JsonValue::from("value1"),
			JsonValue::from(true),
			JsonValue::from(23.42),
		]);
		assert_eq!(
			result,
			JsonValue::Array(JsonArray(vec![
				JsonValue::String("value1".to_string()),
				JsonValue::Boolean(true),
				JsonValue::Number(23.42),
			]))
		);
	}

	#[test]
	fn test_from_vec_of_str() {
		let result = JsonValue::from(vec!["value1", "value2", "value3"]);
		assert_eq!(
			result,
			JsonValue::Array(JsonArray(vec![
				JsonValue::String("value1".to_string()),
				JsonValue::String("value2".to_string()),
				JsonValue::String("value3".to_string()),
			]))
		);
	}

	#[test]
	fn test_type_as_str() {
		assert_eq!(
			JsonValue::String("value".to_string()).type_as_str(),
			"string"
		);
		assert_eq!(JsonValue::Number(42.0).type_as_str(), "number");
		assert_eq!(JsonValue::Boolean(true).type_as_str(), "boolean");
		assert_eq!(JsonValue::Null.type_as_str(), "null");
		assert_eq!(JsonValue::Array(JsonArray(vec![])).type_as_str(), "array");
		assert_eq!(
			JsonValue::Object(JsonObject::default()).type_as_str(),
			"object"
		);
	}

	#[test]
	fn test_stringify() {
		assert_eq!(
			JsonValue::Array(JsonArray(vec![
				JsonValue::String("value".to_string()),
				JsonValue::Number(42.0)
			]))
			.stringify(),
			r#"["value",42]"#
		);

		assert_eq!(
			JsonValue::Object(JsonObject::from(vec![("key", "value")])).stringify(),
			r#"{"key":"value"}"#
		);
	}

	#[test]
	fn test_new_array_and_object() {
		assert_eq!(JsonValue::new_array(), JsonValue::Array(JsonArray(vec![])));
		assert_eq!(
			JsonValue::new_object(),
			JsonValue::Object(JsonObject::default())
		);
	}

	#[test]
	fn test_as_array_to_array() {
		let value = JsonValue::Array(JsonArray(vec![]));

		assert!(value.as_array().is_ok());
		assert!(value.to_array().is_ok());

		let non_array = JsonValue::String("not an array".to_string());
		assert!(non_array.as_array().is_err());
		assert!(non_array.to_array().is_err());
	}

	#[test]
	fn test_as_object_to_object() {
		let value = JsonValue::Object(JsonObject::default());

		assert!(value.as_object().is_ok());
		assert!(value.to_object().is_ok());

		let non_object = JsonValue::String("not an object".to_string());
		assert!(non_object.as_object().is_err());
		assert!(non_object.to_object().is_err());
	}

	#[test]
	fn test_as_string_as_str() {
		let value = JsonValue::String("value".to_string());

		assert_eq!(value.as_string().unwrap(), "value");
		assert_eq!(value.as_str().unwrap(), "value");

		let non_string = JsonValue::Number(42.0);
		assert!(non_string.as_string().is_err());
		assert!(non_string.as_str().is_err());
	}

	#[test]
	fn test_as_number() {
		let value = JsonValue::Number(42.0);

		assert_eq!(value.as_number::<f64>().unwrap(), 42.0);
		assert_eq!(value.as_number::<i32>().unwrap(), 42);

		let non_number = JsonValue::String("not a number".to_string());
		assert!(non_number.as_number::<f64>().is_err());
	}

	#[test]
	fn test_parse_str() {
		let json = r#"{"key":"value","number":42}"#;
		let parsed = JsonValue::parse_str(json).unwrap();

		assert_eq!(
			parsed,
			JsonValue::from(vec![
				("key", JsonValue::from("value")),
				("number", JsonValue::from(42.0))
			])
		);

		let invalid_json = r#"{"key":}"#;
		assert!(JsonValue::parse_str(invalid_json).is_err());
	}

	#[test]
	fn test_parse_blob() {
		let blob = Blob::from(r#"{"key":"value","number":42}"#);
		let parsed = JsonValue::parse_blob(&blob).unwrap();

		assert_eq!(
			parsed,
			JsonValue::from(vec![
				("key", JsonValue::from("value")),
				("number", JsonValue::from(42.0))
			])
		);

		let invalid_blob = Blob::from(r#"{"key":}"#);
		assert!(JsonValue::parse_blob(&invalid_blob).is_err());
	}
}
