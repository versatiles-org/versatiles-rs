use super::JsonValue;
use anyhow::{bail, ensure, Result};
use std::{
	collections::BTreeMap,
	str::{self, Chars},
};

struct JsonParser<'a> {
	inner_chars: Chars<'a>,
	next_char: Option<char>,
	pos: u64,
}

#[allow(dead_code)]
impl<'a> JsonParser<'a> {
	fn new(inner_chars: Chars<'a>) -> Result<Self> {
		let mut parser = JsonParser {
			inner_chars,
			next_char: None,
			pos: 0,
		};
		parser.skip();
		Ok(parser)
	}

	fn error(&self, msg: &str) -> Result<JsonValue> {
		bail!("{msg} at pos {}", self.pos);
	}

	fn peek(&self) -> &Option<char> {
		&self.next_char
	}

	fn skip(&mut self) -> () {
		self.next_char = self.inner_chars.next();
		self.pos += 1;
	}

	fn next(&mut self) -> Option<char> {
		let next_char = self.next_char;
		self.skip();
		next_char
	}

	fn skip_whitespace(&mut self) -> Result<()> {
		while let Some(b) = self.peek() {
			if !b.is_ascii_whitespace() {
				break;
			}
			self.next();
		}
		Ok(())
	}

	pub fn parse_json(&mut self) -> Result<JsonValue> {
		self.skip_whitespace()?;
		match self.peek() {
			Some('[') => self.parse_array(),
			Some('{') => self.parse_object(),
			Some('"') => Ok(JsonValue::Str(self.parse_string()?)),
			Some(d) if d.is_ascii_digit() || *d == '.' || *d == '-' => self.parse_number(),
			Some('t') => self.parse_true(),
			Some('f') => self.parse_false(),
			Some('n') => self.parse_null(),
			Some(c) => self.error(&format!("unexpected character '{c}'")),
			None => self.error("unexpected end of file"),
		}
	}

	fn parse_array(&mut self) -> Result<JsonValue> {
		ensure!(self.next().unwrap() == '[');
		let mut array = Vec::new();
		loop {
			self.skip_whitespace()?;
			match self.peek() {
				Some(']') => {
					self.skip();
					break;
				}
				Some(_) => {
					array.push(self.parse_json()?);
					self.skip_whitespace()?;
					match self.peek() {
						Some(',') => {
							self.skip();
							continue;
						}
						Some(']') => continue,
						Some(_) => {
							self.error("expected ',' or ']'")?;
						}
						None => {
							self.error("unexpected end of file")?;
						}
					}
				}
				None => {
					self.error("unexpected end of file")?;
				}
			}
		}
		Ok(JsonValue::Array(array))
	}

	fn parse_object(&mut self) -> Result<JsonValue> {
		ensure!(self.next().unwrap() == '{');
		let mut object = BTreeMap::new();
		loop {
			self.skip_whitespace()?;
			match self.peek() {
				Some('}') => {
					self.next();
					break;
				}
				Some(_) => {
					let key = self.parse_string()?;

					self.skip_whitespace()?;
					match self.peek() {
						Some(':') => self.skip(),
						_ => {
							self.error("expected ':'")?;
						}
					};

					self.skip_whitespace()?;
					let value = self.parse_json()?;
					object.insert(key, value);

					self.skip_whitespace()?;
					match self.peek() {
						Some(',') => self.skip(),
						Some('}') => continue,
						_ => {
							self.error("expected ',' or '}'")?;
						}
					}
				}
				None => {
					self.error("unexpected end of file")?;
				}
			}
		}
		Ok(JsonValue::Object(object))
	}

	fn parse_string(&mut self) -> Result<String> {
		ensure!(self.next().unwrap() == '"');
		let mut string = String::new();
		loop {
			match self.next() {
				Some('"') => break,
				Some('\\') => match self.next() {
					Some('"') => string.push('"'),
					Some('\\') => string.push('\\'),
					Some('/') => string.push('/'),
					Some('b') => string.push('\x08'),
					Some('f') => string.push('\x0C'),
					Some('n') => string.push('\n'),
					Some('r') => string.push('\r'),
					Some('t') => string.push('\t'),
					Some('u') => {
						let mut hex = String::new();
						for _ in 0..4 {
							if let Some(c) = self.next() {
								hex.push(c);
							} else {
								self.error("invalid unicode code point")?;
							}
						}
						let code_point = u16::from_str_radix(&hex, 16)
							.map_err(|_| self.error("invalid unicode code point").unwrap_err())?;
						string.push(
							char::from_u32(code_point as u32)
								.ok_or_else(|| self.error("invalid unicode code point").unwrap_err())?,
						);
					}
					_ => {
						self.error("unexpected character")?;
					}
				},
				Some(c) => string.push(c),
				None => {
					self.error("unexpected end of file")?;
				}
			}
		}
		Ok(string)
	}

	fn parse_number(&mut self) -> Result<JsonValue> {
		let mut number = String::new();
		while let Some(c) = self.peek() {
			if c.is_ascii_digit() || *c == '-' || *c == '.' {
				number.push(*c);
				self.next();
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
			match self.next() {
				Some(b) if b == c => continue,
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
			match self.next() {
				Some(b) if b == c => continue,
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
			match self.next() {
				Some(b) if b == c => continue,
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
	JsonParser::new(json.chars())?.parse_json()
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
		let data = r##"{"users":{"user1":{"city":"Nantes","country","France"},"user2":{"city":"Bruxelles","country":"Belgium"},"user3":{"city":"Paris","country":"France","age":30}},"countries":["France","Belgium"]}"##;
		let json = parse_json(data);
		assert_eq!(json.unwrap_err().to_string(), "expected ':' at pos 45");
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
		assert_eq!(json.unwrap_err().to_string(), "expected ':' at pos 8");
	}

	#[test]
	fn test_invalid_json_unclosed_brace() {
		let json = parse_json(r##"{"key": "value""##);
		assert_eq!(
			json.unwrap_err().to_string(),
			"expected ',' or '}' at pos 16"
		);
	}

	#[test]
	fn test_invalid_json_unclosed_bracket() {
		let json = parse_json(r##"["key", "value""##);
		assert_eq!(
			json.unwrap_err().to_string(),
			"unexpected end of file at pos 16"
		);
	}
}
