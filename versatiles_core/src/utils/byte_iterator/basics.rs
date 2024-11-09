use super::iterator::ByteIterator;
use anyhow::{bail, Context, Error, Result};
use std::str::FromStr;

pub fn parse_tag(iter: &mut ByteIterator, text: &str) -> Result<()> {
	for c in text.bytes() {
		match iter.expect_next_byte()? {
			b if b == c => continue,
			_ => return Err(iter.format_error("unexpected character while parsing 'null'")),
		}
	}
	Ok(())
}

pub fn parse_quoted_json_string(iter: &mut ByteIterator) -> Result<String> {
	iter.skip_whitespace()?;
	if iter.expect_next_byte()? != b'"' {
		bail!(iter.format_error("expected '\"' while parsing a string"));
	}

	let mut bytes: Vec<u8> = Vec::new();
	loop {
		match iter.expect_next_byte()? {
			b'"' => break,
			b'\\' => match iter.expect_next_byte()? {
				b'"' => bytes.push(b'"'),
				b'\\' => bytes.push(b'\\'),
				b'/' => bytes.push(b'/'),
				b'b' => bytes.push(b'\x08'),
				b'f' => bytes.push(b'\x0C'),
				b'n' => bytes.push(b'\n'),
				b'r' => bytes.push(b'\r'),
				b't' => bytes.push(b'\t'),
				b'u' => {
					let mut hex = String::new();
					for _ in 0..4 {
						hex.push(iter.expect_next_byte()? as char);
					}
					let code_point = u16::from_str_radix(&hex, 16)
						.map_err(|_| iter.format_error("invalid unicode code point"))?;
					let s = String::from_utf16(&[code_point])
						.with_context(|| iter.format_error("invalid unicode code point"))?;
					bytes.append(&mut s.into_bytes());
				}
				c => bytes.push(c),
			},
			c => bytes.push(c),
		}
	}
	String::from_utf8(bytes).map_err(Error::from)
}

pub fn parse_number_as_string(iter: &mut ByteIterator) -> Result<String> {
	let mut number = String::new();
	while let Some(c) = iter.peek() {
		if c.is_ascii_digit() || *c == b'-' || *c == b'.' {
			number.push(*c as char);
			iter.advance();
		} else {
			break;
		}
	}
	Ok(number)
}

pub fn parse_number_as<R: FromStr>(iter: &mut ByteIterator) -> Result<R> {
	parse_number_as_string(iter)?
		.parse::<R>()
		.map_err(|_| iter.format_error("invalid number"))
}

pub fn parse_object_entries<R>(
	iter: &mut ByteIterator,
	mut parse_value: impl FnMut(String, &mut ByteIterator) -> Result<R>,
) -> Result<()> {
	iter.skip_whitespace()?;
	if iter.expect_next_byte()? != b'{' {
		bail!(iter.format_error("expected '{' while parsing an object"));
	}

	loop {
		iter.skip_whitespace()?;
		match iter.expect_peeked_byte()? {
			b'}' => {
				iter.advance();
				break;
			}
			b'"' => {
				let key = parse_quoted_json_string(iter)?;

				iter.skip_whitespace()?;
				match iter.expect_peeked_byte()? {
					b':' => iter.advance(),
					_ => return Err(iter.format_error("expected ':'")),
				};

				iter.skip_whitespace()?;
				parse_value(key, iter)?;

				iter.skip_whitespace()?;
				match iter.expect_peeked_byte()? {
					b',' => iter.advance(),
					b'}' => continue,
					_ => {
						return Err(iter.format_error("expected ',' or '}'"));
					}
				}
			}
			_ => {
				return Err(iter.format_error("parsing object, expected '\"' or '}'"));
			}
		}
	}
	Ok(())
}

pub fn parse_array_entries<R>(
	iter: &mut ByteIterator,
	mut parse_value: impl FnMut(&mut ByteIterator) -> Result<R>,
) -> Result<()> {
	iter.skip_whitespace()?;
	if iter.expect_next_byte()? != b'[' {
		bail!(iter.format_error("expected '[' while parsing an array"));
	}

	loop {
		iter.skip_whitespace()?;
		match iter.expect_peeked_byte()? {
			b']' => {
				iter.advance();
				break;
			}
			_ => {
				parse_value(iter)?;
				iter.skip_whitespace()?;
				match iter.expect_peeked_byte()? {
					b',' => {
						iter.advance();
						continue;
					}
					b']' => continue,
					_ => return Err(iter.format_error("parsing array, expected ',' or ']'")),
				}
			}
		}
	}
	Ok(())
}
