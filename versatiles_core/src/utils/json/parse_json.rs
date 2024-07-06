use super::JsonValue;
use anyhow::{bail, Result};
use nom::{
	branch::alt,
	bytes::complete::{escaped, tag, take_while},
	character::complete::{alphanumeric1 as alphanumeric, char, one_of},
	combinator::{cut, map, value},
	error::{context, convert_error, ContextError, ParseError, VerboseError},
	multi::separated_list0,
	number::complete::double,
	sequence::{preceded, separated_pair, terminated},
	Err, IResult, Parser,
};
use std::collections::HashMap;
use std::str;

pub fn parse_json<'a>(input: &'a str) -> Result<JsonValue> {
	let result = json_value::<VerboseError<&'a str>>(input);
	return match result {
		Ok(r) => Ok(r.1),
		Err(Err::Error(e)) | Err(Err::Failure(e)) => bail!(convert_error(input, e)),
		Err(e) => bail!(e.to_string()),
	};

	fn json_value<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
		i: &'a str,
	) -> IResult<&'a str, JsonValue, E> {
		preceded(
			sp,
			alt((
				map(hash, JsonValue::Object),
				map(array, JsonValue::Array),
				map(string, |s| JsonValue::Str(String::from(s))),
				map(double, JsonValue::Num),
				map(boolean, JsonValue::Boolean),
				map(null, |_| JsonValue::Null),
			)),
		)
		.parse(i)
	}

	fn hash<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
		i: &'a str,
	) -> IResult<&'a str, HashMap<String, JsonValue>, E> {
		context(
			"object",
			preceded(
				char('{'),
				cut(terminated(
					map(
						separated_list0(preceded(sp, char(',')), key_value),
						|tuple_vec| {
							tuple_vec
								.into_iter()
								.map(|(k, v)| (String::from(k), v))
								.collect()
						},
					),
					preceded(sp, char('}')),
				)),
			),
		)
		.parse(i)
	}

	fn key_value<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
		i: &'a str,
	) -> IResult<&'a str, (&'a str, JsonValue), E> {
		separated_pair(
			preceded(sp, string),
			cut(preceded(sp, char(':'))),
			json_value,
		)
		.parse(i)
	}

	fn array<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
		i: &'a str,
	) -> IResult<&'a str, Vec<JsonValue>, E> {
		context(
			"array",
			preceded(
				char('['),
				cut(terminated(
					separated_list0(preceded(sp, char(',')), json_value),
					preceded(sp, char(']')),
				)),
			),
		)
		.parse(i)
	}

	fn string<'a, E: ParseError<&'a str> + ContextError<&'a str>>(
		i: &'a str,
	) -> IResult<&'a str, &'a str, E> {
		context(
			"string",
			preceded(char('\"'), cut(terminated(parse_str, char('\"')))),
		)
		.parse(i)
	}

	fn sp<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, &'a str, E> {
		let chars = " \t\r\n";

		// nom combinators like `take_while` return a function. That function is the
		// parser,to which we can pass the input
		take_while(move |c| chars.contains(c))(i)
	}

	fn parse_str<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, &'a str, E> {
		escaped(alphanumeric, '\\', one_of("\"n\\"))(i)
	}

	fn boolean<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, bool, E> {
		// This is a parser that returns `true` if it sees the string "true", and
		// an error otherwise
		let parse_true = value(true, tag("true"));

		// This is a parser that returns `false` if it sees the string "false", and
		// an error otherwise
		let parse_false = value(false, tag("false"));

		// `alt` combines the two parsers. It returns the result of the first
		// successful parser, or an error
		alt((parse_true, parse_false)).parse(input)
	}

	fn null<'a, E: ParseError<&'a str>>(input: &'a str) -> IResult<&'a str, (), E> {
		value((), tag("null")).parse(input)
	}
}

#[cfg(test)]
mod test {
	use std::collections::HashMap;

	use super::parse_json;
	use crate::utils::JsonValue;

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
		let data = r##"{"users":{"user1":{"city":"Nantes","country","France"},"user2":{"city":"Bruxelles","country":"Belgium"},"user3":{"city":"Paris","country":"France","age":30}},"countries":["France","Belgium"]}"##;
		let json = parse_json(data);
		assert_eq!(
			json.unwrap_err().to_string(),
			r##"0: at line 1:
{"users":{"user1":{"city":"Nantes","country","France"},"user2":{"city":"Bruxelles","country":"Belgium"},"user3":{"city":"Paris","country":"France","age":30}},"countries":["France","Belgium"]}
                                            ^
expected ':', found ,

1: at line 1, in object:
{"users":{"user1":{"city":"Nantes","country","France"},"user2":{"city":"Bruxelles","country":"Belgium"},"user3":{"city":"Paris","country":"France","age":30}},"countries":["France","Belgium"]}
                  ^

2: at line 1, in object:
{"users":{"user1":{"city":"Nantes","country","France"},"user2":{"city":"Bruxelles","country":"Belgium"},"user3":{"city":"Paris","country":"France","age":30}},"countries":["France","Belgium"]}
         ^

3: at line 1, in object:
{"users":{"user1":{"city":"Nantes","country","France"},"user2":{"city":"Bruxelles","country":"Belgium"},"user3":{"city":"Paris","country":"France","age":30}},"countries":["France","Belgium"]}
^

"##
		);
	}

	#[test]
	fn test_empty_object() {
		let json = parse_json("{}").unwrap();
		assert_eq!(json, JsonValue::Object(HashMap::new()));
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
			r##"0: at line 1:
{"key" "value"}
       ^
expected ':', found "

1: at line 1, in object:
{"key" "value"}
^

"##
		);
	}

	#[test]
	fn test_invalid_json_unclosed_brace() {
		let json = parse_json(r##"{"key": "value""##);
		assert_eq!(
			json.unwrap_err().to_string(),
			r##"0: at line 1:
{"key": "value"
               ^
expected '}', got end of input

1: at line 1, in object:
{"key": "value"
^

"##
		);
	}

	#[test]
	fn test_invalid_json_unclosed_bracket() {
		let json = parse_json(r##"["key", "value""##);
		assert_eq!(
			json.unwrap_err().to_string(),
			r##"0: at line 1:
["key", "value"
               ^
expected ']', got end of input

1: at line 1, in array:
["key", "value"
^

"##
		);
	}
}
