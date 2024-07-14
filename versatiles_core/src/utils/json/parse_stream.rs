use super::JsonValue;
use crate::utils::io::CharIterator;
use anyhow::{ensure, Result};
use std::{
	collections::BTreeMap,
	str::{self, Chars},
};

pub struct JsonParser<'a> {
	chars: CharIterator<'a>,
}

#[allow(dead_code)]
impl<'a> JsonParser<'a> {
	pub fn new(chars: Chars<'a>, debug: bool) -> Result<Self> {
		let parser = JsonParser {
			chars: CharIterator::new(chars, debug)?,
		};
		Ok(parser)
	}

	fn error(&self, msg: &str) -> Result<JsonValue> {
		self.chars.build_error(msg).map(|()| JsonValue::Null)
	}

	pub fn parse_json(&mut self) -> Result<JsonValue> {
		self.chars.skip_whitespace()?;
		match self.chars.get_peek_char()? {
			'[' => self.parse_array(),
			'{' => self.parse_object(),
			'"' => Ok(JsonValue::Str(self.parse_string()?)),
			d if d.is_ascii_digit() || d == '.' || d == '-' => self.parse_number(),
			't' => self.parse_true(),
			'f' => self.parse_false(),
			'n' => self.parse_null(),
			c => self.error(&format!("unexpected character '{c}'")),
		}
	}

	fn parse_array(&mut self) -> Result<JsonValue> {
		ensure!(self.chars.get_next_char()? == '[');

		let mut array = Vec::new();
		loop {
			self.chars.skip_whitespace()?;
			match self.chars.get_peek_char()? {
				']' => {
					self.chars.skip_char();
					break;
				}
				_ => {
					array.push(self.parse_json()?);
					self.chars.skip_whitespace()?;
					match self.chars.get_peek_char()? {
						',' => {
							self.chars.skip_char();
							continue;
						}
						']' => continue,
						_ => {
							self.error("parsing array, expected ',' or ']'")?;
						}
					}
				}
			}
		}
		Ok(JsonValue::Array(array))
	}

	fn parse_object(&mut self) -> Result<JsonValue> {
		ensure!(self.chars.get_next_char()? == '{');

		let mut list: Vec<(String, JsonValue)> = Vec::new();
		loop {
			self.chars.skip_whitespace()?;
			match self.chars.get_peek_char()? {
				'}' => {
					self.chars.skip_char();
					break;
				}
				'"' => {
					let key = self.parse_string()?;

					self.chars.skip_whitespace()?;
					match self.chars.get_peek_char()? {
						':' => self.chars.skip_char(),
						_ => {
							self.error("expected ':'")?;
						}
					};

					self.chars.skip_whitespace()?;
					let value = self.parse_json()?;
					list.push((key, value));

					self.chars.skip_whitespace()?;
					match self.chars.get_peek_char()? {
						',' => self.chars.skip_char(),
						'}' => continue,
						_ => {
							self.error("expected ',' or '}'")?;
						}
					}
				}
				_ => {
					self.error("parsing object, expected '\"' or '}'")?;
				}
			}
		}
		Ok(JsonValue::Object(BTreeMap::from_iter(list)))
	}

	fn parse_object_entries(&mut self, callback: impl Fn(String, JsonValue)) -> Result<()> {
		ensure!(self.chars.get_next_char()? == '{');

		loop {
			self.chars.skip_whitespace()?;
			match self.chars.get_peek_char()? {
				'}' => {
					self.chars.skip_char();
					break;
				}
				'"' => {
					let key = self.parse_string()?;

					self.chars.skip_whitespace()?;
					match self.chars.get_peek_char()? {
						':' => self.chars.skip_char(),
						_ => {
							self.error("expected ':'")?;
						}
					};

					self.chars.skip_whitespace()?;
					let value = self.parse_json()?;
					callback(key, value);
				}
				_ => {
					self.error("parsing object, expected '\"' or '}'")?;
				}
			}
		}
		Ok(())
	}

	fn parse_string(&mut self) -> Result<String> {
		ensure!(self.chars.get_next_char()? == '"');

		let mut string = String::new();
		loop {
			match self.chars.get_next_char()? {
				'"' => break,
				'\\' => match self.chars.get_next_char()? {
					'"' => string.push('"'),
					'\\' => string.push('\\'),
					'/' => string.push('/'),
					'b' => string.push('\x08'),
					'f' => string.push('\x0C'),
					'n' => string.push('\n'),
					'r' => string.push('\r'),
					't' => string.push('\t'),
					'u' => {
						let mut hex = String::new();
						for _ in 0..4 {
							hex.push(self.chars.get_next_char()?);
						}
						let code_point = u16::from_str_radix(&hex, 16)
							.map_err(|_| self.error("invalid unicode code point").unwrap_err())?;
						string.push(
							char::from_u32(code_point as u32)
								.ok_or_else(|| self.error("invalid unicode code point").unwrap_err())?,
						);
					}
					c => string.push(c),
				},
				c => string.push(c),
			}
		}
		Ok(string)
	}

	fn parse_number(&mut self) -> Result<JsonValue> {
		let mut number = String::new();
		while let Some(c) = self.chars.peek_char() {
			if c.is_ascii_digit() || *c == '-' || *c == '.' {
				number.push(*c);
				self.chars.skip_char();
			} else {
				break;
			}
		}
		if let Ok(n) = number.parse::<f64>() {
			Ok(JsonValue::Num(n))
		} else {
			self.error("invalid number")
		}
	}

	fn parse_true(&mut self) -> Result<JsonValue> {
		let true_str = ['t', 'r', 'u', 'e'];
		for &c in &true_str {
			match self.chars.get_next_char()? {
				b if b == c => continue,
				_ => {
					self.error("unexpected character while parsing 'true'")?;
				}
			}
		}
		Ok(JsonValue::Boolean(true))
	}

	fn parse_false(&mut self) -> Result<JsonValue> {
		let false_str = ['f', 'a', 'l', 's', 'e'];
		for &c in &false_str {
			match self.chars.get_next_char()? {
				b if b == c => continue,
				_ => {
					self.error("unexpected character while parsing 'false'")?;
				}
			}
		}
		Ok(JsonValue::Boolean(false))
	}

	fn parse_null(&mut self) -> Result<JsonValue> {
		let null_str = ['n', 'u', 'l', 'l'];
		for &c in &null_str {
			match self.chars.get_next_char()? {
				b if b == c => continue,
				_ => {
					self.error("unexpected character while parsing 'null'")?;
				}
			}
		}
		Ok(JsonValue::Null)
	}
}

#[allow(dead_code)]
pub fn parse_json(json: &str) -> Result<JsonValue> {
	JsonParser::new(json.chars(), true)?.parse_json()
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
