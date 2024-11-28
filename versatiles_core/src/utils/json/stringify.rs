use super::JsonValue;
use anyhow::Result;

pub fn json_as_string(json: &JsonValue) -> Result<String> {
	match json {
		JsonValue::Str(s) => Ok(format!("\"{}\"", escape_json_string(s))),
		JsonValue::Num(n) => Ok(n.to_string()),
		JsonValue::Boolean(b) => Ok(b.to_string()),
		JsonValue::Null => Ok(String::from("null")),
		JsonValue::Array(arr) => {
			let elements = arr
				.iter()
				.map(|item| item.as_string())
				.collect::<Result<Vec<String>>>()?;
			Ok(format!("[{}]", elements.join(",")))
		}
		JsonValue::Object(obj) => {
			let elements = obj
				.iter()
				.map(|(key, value)| {
					Ok(format!(
						"\"{}\":{}",
						escape_json_string(key),
						value.as_string()?
					))
				})
				.collect::<Result<Vec<String>>>()?;
			Ok(format!("{{{}}}", elements.join(",")))
		}
	}
}

fn escape_json_string(input: &str) -> String {
	input
		.chars()
		.map(|c| match c {
			'"' => "\\\"".to_string(),
			'\\' => "\\\\".to_string(),
			'\n' => "\\n".to_string(),
			'\r' => "\\r".to_string(),
			'\t' => "\\t".to_string(),
			'\u{08}' => "\\b".to_string(),
			'\u{0c}' => "\\f".to_string(),
			c if c.is_control() => format!("\\u{:04x}", c as u32),
			c => c.to_string(),
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::super::parse::parse_json_str;
	use super::json_as_string;
	use anyhow::Result;

	#[test]
	fn test_as_string_primitives() -> Result<()> {
		let json = parse_json_str("\"Hello, World!\"")?;
		assert_eq!(
			json_as_string(&json)?,
			"\"Hello, World!\"",
			"String with normal characters failed"
		);

		let json = parse_json_str("42")?;
		assert_eq!(json_as_string(&json)?, "42", "Number test failed");

		let json = parse_json_str("true")?;
		assert_eq!(json_as_string(&json)?, "true", "Boolean true test failed");

		let json = parse_json_str("null")?;
		assert_eq!(json_as_string(&json)?, "null", "Null test failed");
		Ok(())
	}

	#[test]
	fn test_as_string_special_characters() -> Result<()> {
		let json = parse_json_str("\"Line1\\nLine2\\rTab\\tBackslash\\\\\"")?;
		assert_eq!(
			json_as_string(&json)?,
			"\"Line1\\nLine2\\rTab\\tBackslash\\\\\"",
			"Special character escaping failed"
		);

		let json = parse_json_str("\"Hello \\\"World\\\"\"")?;
		assert_eq!(
			json_as_string(&json)?,
			"\"Hello \\\"World\\\"\"",
			"Escaped quotes test failed"
		);
		Ok(())
	}

	#[test]
	fn test_as_string_unicode() -> Result<()> {
		let json = parse_json_str("\"Unicode: 😊\"")?;
		assert_eq!(
			json_as_string(&json)?,
			"\"Unicode: 😊\"",
			"Unicode character test failed"
		);

		let json = parse_json_str("\"Emoji and text 🌟✨\"")?;
		assert_eq!(
			json_as_string(&json)?,
			"\"Emoji and text 🌟✨\"",
			"Emoji and text test failed"
		);
		Ok(())
	}

	#[test]
	fn test_as_string_array() -> Result<()> {
		let json = parse_json_str("[\"item1\", 123, false, null]")?;
		assert_eq!(
			json_as_string(&json)?,
			"[\"item1\",123,false,null]",
			"Mixed type array test failed"
		);

		let json = parse_json_str("[]")?;
		assert_eq!(json_as_string(&json)?, "[]", "Empty array test failed");
		Ok(())
	}

	#[test]
	fn test_as_string_object() -> Result<()> {
		let json = parse_json_str("{\"key1\": \"value1\", \"key2\": 42}")?;
		assert_eq!(
			json_as_string(&json)?,
			"{\"key1\":\"value1\",\"key2\":42}",
			"Simple object test failed"
		);

		let json = parse_json_str("{}")?;
		assert_eq!(json_as_string(&json)?, "{}", "Empty object test failed");
		Ok(())
	}

	#[test]
	fn test_as_string_nested() -> Result<()> {
		let json = parse_json_str(
			"{\"nested\": {\"array\": [\"value\", {\"inner_key\": 3.14}], \"boolean\": true}}",
		)?;
		assert_eq!(
			json_as_string(&json)?,
			"{\"nested\":{\"array\":[\"value\",{\"inner_key\":3.14}],\"boolean\":true}}",
			"Nested structure test failed"
		);
		Ok(())
	}

	#[test]
	fn test_as_string_complex_object() -> Result<()> {
		let json = parse_json_str(
			r#"
            {
                "string": "value",
                "number": 123.45,
                "boolean": false,
                "null_value": null,
                "array": [1, "two", true],
                "object": {
                    "key": "value",
                    "nested_array": [3, 4, 5]
                }
            }
            "#,
		)?;
		assert_eq!(
            json_as_string(&json)?,
            "{\"array\":[1,\"two\",true],\"boolean\":false,\"null_value\":null,\"number\":123.45,\"object\":{\"key\":\"value\",\"nested_array\":[3,4,5]},\"string\":\"value\"}",
            "Complex object test failed"
        );
		Ok(())
	}
}
