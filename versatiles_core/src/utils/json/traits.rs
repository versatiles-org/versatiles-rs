use super::JsonValue;
use crate::utils::io::CharIterator;
use anyhow::{ensure, Result};

pub trait StreamParser {
	fn iter(&self) -> CharIterator;

	fn error(&self, msg: &str) -> Result<JsonValue> {
		let iter = self.iter();
		iter.build_error(msg).map(|()| JsonValue::Null)
	}

	fn parse_json(&mut self) -> Result<JsonValue> {
		let mut iter = self.iter();
		iter.skip_whitespace()?;
		match iter.get_peek_char()? {
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
		let mut iter = self.iter();
		ensure!(iter.get_next_char()? == '[');

		let mut array = Vec::new();
		loop {
			iter.skip_whitespace()?;
			match iter.get_peek_char()? {
				']' => {
					iter.skip_char();
					break;
				}
				_ => {
					array.push(self.parse_json()?);
					iter.skip_whitespace()?;
					match iter.get_peek_char()? {
						',' => {
							iter.skip_char();
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
		let mut iter = self.iter();
		ensure!(iter.get_next_char()? == '{');

		let mut list: Vec<(String, JsonValue)> = Vec::new();
		loop {
			iter.skip_whitespace()?;
			match iter.get_peek_char()? {
				'}' => {
					iter.skip_char();
					break;
				}
				'"' => {
					let key = self.parse_string()?;

					iter.skip_whitespace()?;
					match iter.get_peek_char()? {
						':' => iter.skip_char(),
						_ => {
							self.error("expected ':'")?;
						}
					};

					iter.skip_whitespace()?;
					let value = self.parse_json()?;
					list.push((key, value));

					iter.skip_whitespace()?;
					match iter.get_peek_char()? {
						',' => iter.skip_char(),
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
		let mut iter = self.iter();
		ensure!(iter.get_next_char()? == '{');

		loop {
			iter.skip_whitespace()?;
			match iter.get_peek_char()? {
				'}' => {
					iter.skip_char();
					break;
				}
				'"' => {
					let key = self.parse_string()?;

					iter.skip_whitespace()?;
					match iter.get_peek_char()? {
						':' => iter.skip_char(),
						_ => {
							self.error("expected ':'")?;
						}
					};

					iter.skip_whitespace()?;
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
		let mut iter = self.iter();
		ensure!(iter.get_next_char()? == '"');

		let mut string = String::new();
		loop {
			match iter.get_next_char()? {
				'"' => break,
				'\\' => match iter.get_next_char()? {
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
							hex.push(iter.get_next_char()?);
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
		let mut iter = self.iter();
		let mut number = String::new();
		while let Some(c) = iter.peek_char() {
			if c.is_ascii_digit() || *c == '-' || *c == '.' {
				number.push(*c);
				iter.skip_char();
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
		let mut iter = self.iter();
		let true_str = ['t', 'r', 'u', 'e'];
		for &c in &true_str {
			match iter.get_next_char()? {
				b if b == c => continue,
				_ => {
					self.error("unexpected character while parsing 'true'")?;
				}
			}
		}
		Ok(JsonValue::Boolean(true))
	}

	fn parse_false(&mut self) -> Result<JsonValue> {
		let mut iter = self.iter();
		let false_str = ['f', 'a', 'l', 's', 'e'];
		for &c in &false_str {
			match iter.get_next_char()? {
				b if b == c => continue,
				_ => {
					self.error("unexpected character while parsing 'false'")?;
				}
			}
		}
		Ok(JsonValue::Boolean(false))
	}

	fn parse_null(&mut self) -> Result<JsonValue> {
		let mut iter = self.iter();
		let null_str = ['n', 'u', 'l', 'l'];
		for &c in &null_str {
			match iter.get_next_char()? {
				b if b == c => continue,
				_ => {
					self.error("unexpected character while parsing 'null'")?;
				}
			}
		}
		Ok(JsonValue::Null)
	}
}
