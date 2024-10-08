use super::JsonValue;
use crate::utils::{
	parse_array_entries, parse_number_as, parse_object_entries, parse_quoted_json_string, parse_tag,
	ByteIterator,
};
use anyhow::Result;
use std::{collections::BTreeMap, str};

pub fn parse_json(json: &str) -> Result<JsonValue> {
	let mut iter = ByteIterator::new(json.bytes(), true)?;
	parse_json_value(&mut iter)
}

pub fn parse_json_value(iter: &mut ByteIterator) -> Result<JsonValue> {
	iter.skip_whitespace()?;
	match iter.get_peek_byte()? {
		b'[' => parse_json_array(iter),
		b'{' => parse_json_object(iter),
		b'"' => parse_json_string(iter),
		d if d.is_ascii_digit() || d == b'.' || d == b'-' => parse_json_number(iter),
		b't' => parse_true(iter),
		b'f' => parse_false(iter),
		b'n' => parse_null(iter),
		c => Err(iter.build_error(&format!("unexpected character '{}'", c as char))),
	}
}

fn parse_json_array(iter: &mut ByteIterator) -> Result<JsonValue> {
	let mut array = Vec::new();
	parse_array_entries(iter, |iter2| {
		array.push(parse_json_value(iter2)?);
		Ok(())
	})?;
	Ok(JsonValue::Array(array))
}

fn parse_json_object(iter: &mut ByteIterator) -> Result<JsonValue> {
	let mut list: Vec<(String, JsonValue)> = Vec::new();
	parse_object_entries(iter, |key, iter2| {
		let value = parse_json_value(iter2)?;
		list.push((key, value));
		Ok(())
	})?;
	Ok(JsonValue::Object(BTreeMap::from_iter(list)))
}

fn parse_json_string(iter: &mut ByteIterator) -> Result<JsonValue> {
	parse_quoted_json_string(iter).map(JsonValue::Str)
}

fn parse_json_number(iter: &mut ByteIterator) -> Result<JsonValue> {
	parse_number_as::<f64>(iter).map(JsonValue::Num)
}

fn parse_true(iter: &mut ByteIterator) -> Result<JsonValue> {
	parse_tag(iter, "true").map(|_| JsonValue::Boolean(true))
}

fn parse_false(iter: &mut ByteIterator) -> Result<JsonValue> {
	parse_tag(iter, "false").map(|_| JsonValue::Boolean(false))
}

fn parse_null(iter: &mut ByteIterator) -> Result<JsonValue> {
	parse_tag(iter, "null").map(|_| JsonValue::Null)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::utils::JsonValue;
	use std::collections::BTreeMap;

	fn v<T>(input: T) -> JsonValue
	where
		JsonValue: From<T>,
	{
		JsonValue::from(input)
	}

	#[test]
	fn simple() {
		let data = r##"{"users":{"user1":{"city":"Nantes","country":"France"},"user2":{"city":"Bruxelles","country":"Belgium"},"user3":{"city":"Paris","country":"France","age":30}},"countries":["France","Belgium"]}"##;
		let json = parse_json(data).unwrap();
		assert_eq!(
			json,
			v(vec![
				(
					"users",
					v(vec![
						("user1", v(vec![("city", "Nantes"), ("country", "France")])),
						(
							"user2",
							v(vec![("city", "Bruxelles"), ("country", "Belgium")])
						),
						(
							"user3",
							v(vec![
								("city", v("Paris")),
								("country", v("France")),
								("age", v(30))
							])
						)
					])
				),
				("countries", v(vec!["France", "Belgium"]))
			])
		);
	}

	#[test]
	fn error() {
		let data = r##"{"city":"Nantes","country","France"}"##;
		let json = parse_json(data);
		assert_eq!(
			json.unwrap_err().to_string(),
			"expected ':' at pos 27: ntes\",\"country\","
		);
	}

	#[test]
	fn test_whitespaces() -> Result<()> {
		let result = v(vec![(
			"a",
			v(vec![
				v(vec![
					("b", JsonValue::from(7)),
					("c", JsonValue::from(true)),
				]),
				v(vec![
					("d", JsonValue::from(false)),
					("e", JsonValue::Null),
					("f", JsonValue::from("g")),
				]),
			]),
		)]);

		let data =
			r##"_{_"a"_:_[_{_"b"_:_7_,_"c"_:_true_}_,_{_"d"_:_false_,_"e"_:_null_,_"f"_:_"g"_}_]_}_"##;

		assert_eq!(parse_json(&data.replace('_', ""))?, result);
		assert_eq!(parse_json(&data.replace('_', " "))?, result);
		assert_eq!(parse_json(&data.replace('_', "\t"))?, result);
		assert_eq!(parse_json(&data.replace('_', "\n"))?, result);
		assert_eq!(parse_json(&data.replace('_', "\r"))?, result);

		Ok(())
	}

	#[test]
	fn test_empty_object() {
		let json = parse_json("{}").unwrap();
		assert_eq!(json, JsonValue::Object(BTreeMap::new()));
	}

	#[test]
	fn test_empty_array() {
		let json = parse_json("[]").unwrap();
		assert_eq!(json, JsonValue::Array(vec![]));
	}

	#[test]
	fn test_nested_array() {
		let json = parse_json("[1, [2, 3], 4]").unwrap();
		assert_eq!(json, v(vec![v(1.0), v(vec![v(2.0), v(3.0)]), v(4.0)]));
	}

	#[test]
	fn test_nested_object() {
		let json = parse_json(r##"{"a": {"b": {"c": "d"}}}"##).unwrap();
		assert_eq!(json, v(vec![("a", v(vec![("b", v(vec![("c", v("d"))]))]))]));
	}

	#[test]
	fn test_null_value() {
		let json = parse_json(r##"{"key": null}"##).unwrap();
		assert_eq!(json, v(vec![("key", JsonValue::Null)]));
	}

	#[test]
	fn test_boolean_value() {
		let json = parse_json(r##"{"key1": true, "key2": false}"##).unwrap();
		assert_eq!(json, v(vec![("key1", v(true)), ("key2", v(false))]));
	}

	#[test]
	fn test_number_value() {
		let json = parse_json(r##"{"integer": 42, "float": 3.14}"##).unwrap();
		assert_eq!(json, v(vec![("integer", v(42.0)), ("float", v(3.14))]));
	}

	#[test]
	fn test_string_value() {
		let json = parse_json(r##"{"key": "value"}"##).unwrap();
		assert_eq!(json, v(vec![("key", v("value"))]));
	}

	#[test]
	fn test_invalid_json_missing_colon() {
		let json = parse_json(r##"{"key" "value"}"##);
		assert_eq!(
			json.unwrap_err().to_string(),
			"expected ':' at pos 8: {\"key\" \""
		);
	}

	#[test]
	fn test_invalid_json_unclosed_brace() {
		let json = parse_json(r##"{"key": "value""##);
		assert_eq!(
			json.unwrap_err().to_string(),
			"unexpected end at pos 16: {\"key\": \"value\"<EOF>"
		);
	}

	#[test]
	fn test_invalid_json_unclosed_bracket() {
		let json = parse_json(r##"["key", "value""##);
		assert_eq!(
			json.unwrap_err().to_string(),
			"unexpected end at pos 16: [\"key\", \"value\"<EOF>"
		);
	}
}
