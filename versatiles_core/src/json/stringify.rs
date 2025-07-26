use super::JsonValue;

pub fn stringify(json: &JsonValue) -> String {
	match json {
		JsonValue::String(s) => format!("\"{}\"", escape_json_string(s)),
		JsonValue::Number(n) => n.to_string(),
		JsonValue::Boolean(b) => b.to_string(),
		JsonValue::Null => String::from("null"),
		JsonValue::Array(arr) => arr.stringify(),
		JsonValue::Object(obj) => obj.stringify(),
	}
}

pub fn stringify_pretty_single_line(json: &JsonValue) -> String {
	match json {
		JsonValue::Array(arr) => arr.stringify_pretty_single_line(),
		JsonValue::Object(obj) => obj.stringify_pretty_single_line(),
		_ => stringify(json),
	}
}

pub fn stringify_pretty_multi_line(json: &JsonValue, max_width: usize, depth: usize, indention: usize) -> String {
	match json {
		JsonValue::Array(arr) => {
			let single_line = arr.stringify_pretty_single_line();
			if single_line.len() + indention <= max_width {
				return single_line;
			}
			arr.stringify_pretty_multi_line(max_width, depth)
		}
		JsonValue::Object(obj) => {
			let single_line = obj.stringify_pretty_single_line();
			if single_line.len() + indention <= max_width {
				return single_line;
			}
			obj.stringify_pretty_multi_line(max_width, depth)
		}
		_ => stringify(json),
	}
}

pub fn escape_json_string(input: &str) -> String {
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
	use super::stringify;
	use anyhow::Result;

	#[test]
	fn test_as_string_primitives() -> Result<()> {
		let json = parse_json_str("\"Hello, World!\"")?;
		assert_eq!(
			stringify(&json),
			"\"Hello, World!\"",
			"String with normal characters failed"
		);

		let json = parse_json_str("42")?;
		assert_eq!(stringify(&json), "42", "Number test failed");

		let json = parse_json_str("true")?;
		assert_eq!(stringify(&json), "true", "Boolean true test failed");

		let json = parse_json_str("null")?;
		assert_eq!(stringify(&json), "null", "Null test failed");
		Ok(())
	}

	#[test]
	fn test_as_string_special_characters() -> Result<()> {
		let json = parse_json_str("\"Line1\\nLine2\\rTab\\tBackslash\\\\\"")?;
		assert_eq!(
			stringify(&json),
			"\"Line1\\nLine2\\rTab\\tBackslash\\\\\"",
			"Special character escaping failed"
		);

		let json = parse_json_str("\"Hello \\\"World\\\"\"")?;
		assert_eq!(
			stringify(&json),
			"\"Hello \\\"World\\\"\"",
			"Escaped quotes test failed"
		);
		Ok(())
	}

	#[test]
	fn test_as_string_unicode() -> Result<()> {
		let json = parse_json_str("\"Unicode: 😊\"")?;
		assert_eq!(stringify(&json), "\"Unicode: 😊\"", "Unicode character test failed");

		let json = parse_json_str("\"Emoji and text 🌟✨\"")?;
		assert_eq!(
			stringify(&json),
			"\"Emoji and text 🌟✨\"",
			"Emoji and text test failed"
		);
		Ok(())
	}

	#[test]
	fn test_as_string_array() -> Result<()> {
		let json = parse_json_str("[\"item1\", 123, false, null]")?;
		assert_eq!(
			stringify(&json),
			"[\"item1\",123,false,null]",
			"Mixed type array test failed"
		);

		let json = parse_json_str("[]")?;
		assert_eq!(stringify(&json), "[]", "Empty array test failed");
		Ok(())
	}

	#[test]
	fn test_as_string_object() -> Result<()> {
		let json = parse_json_str("{\"key1\": \"value1\", \"key2\": 42}")?;
		assert_eq!(
			stringify(&json),
			"{\"key1\":\"value1\",\"key2\":42}",
			"Simple object test failed"
		);

		let json = parse_json_str("{}")?;
		assert_eq!(stringify(&json), "{}", "Empty object test failed");
		Ok(())
	}

	#[test]
	fn test_as_string_nested() -> Result<()> {
		let json = parse_json_str("{\"nested\": {\"array\": [\"value\", {\"inner_key\": 3.14}], \"boolean\": true}}")?;
		assert_eq!(
			stringify(&json),
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
			stringify(&json),
			"{\"array\":[1,\"two\",true],\"boolean\":false,\"null_value\":null,\"number\":123.45,\"object\":{\"key\":\"value\",\"nested_array\":[3,4,5]},\"string\":\"value\"}",
			"Complex object test failed"
		);
		Ok(())
	}

	#[test]
	fn test_escape_json_string_control() {
		let input = "Control:\x01\x02";
		let escaped = super::escape_json_string(input);
		assert_eq!(escaped, "Control:\\u0001\\u0002");
	}

	#[test]
	fn test_stringify_pretty_single_line_primitives() -> Result<()> {
		let json = parse_json_str("123")?;
		assert_eq!(super::stringify_pretty_single_line(&json), "123");
		let json = parse_json_str("\"abc\"")?;
		assert_eq!(super::stringify_pretty_single_line(&json), "\"abc\"");
		Ok(())
	}

	#[test]
	fn test_pretty_single_line_array() -> Result<()> {
		let json = parse_json_str("[1,2,3]")?;
		assert_eq!(super::stringify_pretty_single_line(&json), "[ 1, 2, 3 ]");
		Ok(())
	}

	#[test]
	fn test_stringify_pretty_multi_line_array() -> Result<()> {
		// Force multi-line by using a small max_width
		let json = parse_json_str("[\"alpha\",\"beta\",\"gamma\"]")?;
		let result = super::stringify_pretty_multi_line(&json, 5, 0, 0);
		let expected = "[\n  \"alpha\",\n  \"beta\",\n  \"gamma\"\n]";
		assert_eq!(result, expected);
		Ok(())
	}

	#[test]
	fn test_stringify_pretty_multi_line_object() -> Result<()> {
		// Force multi-line by using a small max_width
		let json = parse_json_str("{\"a\":1,\"bb\":2}")?;
		let result = super::stringify_pretty_multi_line(&json, 5, 0, 0);
		let expected = "{\n  \"a\": 1,\n  \"bb\": 2\n}";
		assert_eq!(result, expected);
		Ok(())
	}
}
