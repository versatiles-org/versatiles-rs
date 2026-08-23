//! JSON parsing utilities for converting text or byte iterators into `JsonValue`.
use std::{collections::BTreeMap, io::Cursor};

use anyhow::Result;
use versatiles_derive::context;

use crate::{
	byte_iterator::{
		ByteIterator, parse_array_entries, parse_number_as, parse_object_entries, parse_quoted_json_string, parse_tag,
	},
	json::{JsonArray, JsonObject, JsonValue},
};

/// How deeply arrays and objects may nest before parsing gives up.
///
/// The parser is recursive descent, so nesting depth is stack depth: without a
/// bound, `[[[[…]]]]` from a container's metadata overflows the stack and aborts
/// the process, which no caller can catch. 128 is the same limit `serde_json`
/// applies by default, and far beyond anything a real document uses.
const MAX_NESTING_DEPTH: usize = 128;

/// Parse a JSON string into a `JsonValue`.
///
/// # Errors
/// Returns an error if the JSON is invalid, with context indicating the input string.
#[context("while parsing JSON string '{}'", json)]
pub fn parse_json_str(json: &str) -> Result<JsonValue> {
	let mut iter = ByteIterator::from_reader(Cursor::new(json), true);
	parse_json_iter(&mut iter)
}

/// Parse JSON data from a `ByteIterator` into a `JsonValue`.
///
/// Consumes leading whitespace and dispatches to the appropriate parser based on the next character.
///
/// # Errors
/// Returns an error if an unexpected character is encountered or parsing fails.
#[context("while parsing JSON data")]
pub fn parse_json_iter(iter: &mut ByteIterator) -> Result<JsonValue> {
	parse_json_at_depth(iter, 0)
}

/// Parse a value that sits `depth` levels inside the document.
fn parse_json_at_depth(iter: &mut ByteIterator, depth: usize) -> Result<JsonValue> {
	if depth >= MAX_NESTING_DEPTH {
		return Err(iter.format_error(&format!("JSON nested deeper than {MAX_NESTING_DEPTH} levels")));
	}

	iter.skip_whitespace();
	match iter.expect_peeked_byte()? {
		b'[' => parse_array_entries(iter, |iter2| parse_json_at_depth(iter2, depth + 1))
			.map(|i| JsonValue::Array(JsonArray(i))),
		b'{' => parse_json_object(iter, depth),
		b'"' => parse_quoted_json_string(iter).map(JsonValue::String),
		d if d.is_ascii_digit() || d == b'.' || d == b'-' => parse_number_as::<f64>(iter).map(JsonValue::Number),
		b't' => parse_tag(iter, "true").map(|()| JsonValue::Boolean(true)),
		b'f' => parse_tag(iter, "false").map(|()| JsonValue::Boolean(false)),
		b'n' => parse_tag(iter, "null").map(|()| JsonValue::Null),
		c => Err(iter.format_error(&format!("unexpected character '{}'", c as char))),
	}
}

/// Parse a JSON object from a `ByteIterator`, constructing a `JsonValue::Object`.
///
/// Reads key-value pairs into a `BTreeMap`, preserving insertion order.
///
/// # Errors
/// Returns an error if object syntax is invalid.
#[context("while parsing JSON object")]
fn parse_json_object(iter: &mut ByteIterator, depth: usize) -> Result<JsonValue> {
	let mut list: Vec<(String, JsonValue)> = Vec::new();
	parse_object_entries(iter, |key, iter2| {
		list.push((key, parse_json_at_depth(iter2, depth + 1)?));
		Ok(())
	})?;
	Ok(JsonValue::Object(JsonObject(BTreeMap::from_iter(list))))
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Recursive descent means nesting depth is stack depth. Without a bound,
	/// `[[[[…]]]]` in a container's metadata overflowed the stack and aborted the
	/// process — 10,000 levels was enough, and no caller can catch that.
	#[test]
	fn nesting_is_bounded() -> Result<()> {
		// Ordinary depth still parses, both shapes, right up to the limit.
		assert!(parse_json_str(&format!("{}{}", "[".repeat(100), "]".repeat(100))).is_ok());
		assert!(parse_json_str(&format!("{}1{}", "{\"a\":".repeat(100), "}".repeat(100))).is_ok());
		assert!(
			parse_json_str(&format!(
				"{}{}",
				"[".repeat(MAX_NESTING_DEPTH),
				"]".repeat(MAX_NESTING_DEPTH)
			))
			.is_ok(),
			"exactly {MAX_NESTING_DEPTH} levels must still parse"
		);

		// One level past it is an error, not a crash.
		for depth in [MAX_NESTING_DEPTH + 1, 10_000, 1_000_000] {
			let error = parse_json_str(&format!("{}{}", "[".repeat(depth), "]".repeat(depth))).unwrap_err();
			assert!(
				format!("{error:#}").contains("nested deeper"),
				"depth {depth} gave: {error:#}"
			);
		}

		Ok(())
	}
	/// JSON spells a character outside the BMP as a surrogate pair, and a writer
	/// that escapes everything above ASCII produces them for emoji and the CJK
	/// extensions alike. Decoding the halves separately rejected all of them.
	#[test]
	fn escaped_characters_decode_including_surrogate_pairs() -> Result<()> {
		assert_eq!(parse_json_str(r#""\u0041""#)?, JsonValue::from("A"));
		assert_eq!(parse_json_str(r#""\u00e4""#)?, JsonValue::from("ä"));
		assert_eq!(parse_json_str(r#""\ud83d\ude00""#)?, JsonValue::from("😀"));
		assert_eq!(parse_json_str(r#""\ud840\udc0b""#)?, JsonValue::from("\u{2000b}"));

		// A high surrogate with nothing after it, and a lone low surrogate, are
		// both malformed rather than silently mangled.
		assert!(parse_json_str(r#""\ud83d""#).is_err());
		assert!(parse_json_str(r#""\ud83dx""#).is_err());
		assert!(parse_json_str(r#""\ude00""#).is_err());

		Ok(())
	}

	fn v<T>(input: T) -> JsonValue
	where
		JsonValue: From<T>,
	{
		JsonValue::from(input)
	}

	#[test]
	fn simple() {
		let data = r#"{"users":{"user1":{"city":"Nantes","country":"France"},"user2":{"city":"Bruxelles","country":"Belgium"},"user3":{"city":"Paris","country":"France","age":30}},"countries":["France","Belgium"]}"#;
		let json = parse_json_str(data).unwrap();
		assert_eq!(
			json,
			v(vec![
				(
					"users",
					v(vec![
						("user1", v(vec![("city", "Nantes"), ("country", "France")])),
						("user2", v(vec![("city", "Bruxelles"), ("country", "Belgium")])),
						(
							"user3",
							v(vec![("city", v("Paris")), ("country", v("France")), ("age", v(30))])
						)
					])
				),
				("countries", v(vec!["France", "Belgium"]))
			])
		);
	}

	#[test]
	fn error() {
		let data = r#"{"city":"Nantes","country","France"}"#;
		let json = parse_json_str(data);
		assert_eq!(
			json.unwrap_err().chain().next_back().unwrap().to_string(),
			"expected ':' at position 27: tes\",\"country\","
		);
	}

	#[test]
	fn test_whitespaces() -> Result<()> {
		let result = v(vec![(
			"a",
			v(vec![
				v(vec![("b", JsonValue::from(7)), ("c", JsonValue::from(true))]),
				v(vec![
					("d", JsonValue::from(false)),
					("e", JsonValue::Null),
					("f", JsonValue::from("g")),
				]),
			]),
		)]);

		let data = r#"_{_"a"_:_[_{_"b"_:_7_,_"c"_:_true_}_,_{_"d"_:_false_,_"e"_:_null_,_"f"_:_"g"_}_]_}_"#;

		assert_eq!(parse_json_str(&data.replace('_', ""))?, result);
		assert_eq!(parse_json_str(&data.replace('_', " "))?, result);
		assert_eq!(parse_json_str(&data.replace('_', "\t"))?, result);
		assert_eq!(parse_json_str(&data.replace('_', "\n"))?, result);
		assert_eq!(parse_json_str(&data.replace('_', "\r"))?, result);

		Ok(())
	}

	#[test]
	fn test_empty_object() {
		let json = parse_json_str("{}").unwrap();
		assert_eq!(json, JsonValue::new_object());
	}

	#[test]
	fn test_empty_array() {
		let json = parse_json_str("[]").unwrap();
		assert_eq!(json, JsonValue::new_array());
	}

	#[test]
	fn test_nested_array() {
		let json = parse_json_str("[1, [2, 3], 4]").unwrap();
		assert_eq!(json, v(vec![v(1.0), v(vec![v(2.0), v(3.0)]), v(4.0)]));
	}

	#[test]
	fn test_nested_object() {
		let json = parse_json_str(r#"{"a": {"b": {"c": "d"}}}"#).unwrap();
		assert_eq!(json, v(vec![("a", v(vec![("b", v(vec![("c", v("d"))]))]))]));
	}

	#[test]
	fn test_null_value() {
		let json = parse_json_str(r#"{"key": null}"#).unwrap();
		assert_eq!(json, v(vec![("key", JsonValue::Null)]));
	}

	#[test]
	fn test_boolean_value() {
		let json = parse_json_str(r#"{"key1": true, "key2": false}"#).unwrap();
		assert_eq!(json, v(vec![("key1", v(true)), ("key2", v(false))]));
	}

	#[test]
	fn test_number_value() {
		let json = parse_json_str(r#"{"integer": 42, "float": 23.42}"#).unwrap();
		assert_eq!(json, v(vec![("integer", v(42.0)), ("float", v(23.42))]));
	}

	#[test]
	fn test_string_value() {
		let json = parse_json_str(r#"{"key": "value"}"#).unwrap();
		assert_eq!(json, v(vec![("key", v("value"))]));
	}

	#[test]
	fn test_invalid_json_missing_colon() {
		let json = parse_json_str(r#"{"key" "value"}"#);
		assert_eq!(
			json.unwrap_err().chain().next_back().unwrap().to_string(),
			"expected ':' at position 8: {\"key\" \""
		);
	}

	#[test]
	fn test_invalid_json_unclosed_brace() {
		let json = parse_json_str(r#"{"key": "value""#);
		assert_eq!(
			json.unwrap_err().chain().next_back().unwrap().to_string(),
			"unexpected end at position 15: {\"key\": \"value\"<EOF>"
		);
	}

	#[test]
	fn test_invalid_json_unclosed_bracket() {
		let json = parse_json_str(r#"["key", "value""#);
		assert_eq!(
			json.unwrap_err().chain().next_back().unwrap().to_string(),
			"unexpected end at position 15: [\"key\", \"value\"<EOF>"
		);
	}
}
