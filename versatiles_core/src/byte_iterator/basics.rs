//! Small parsing helpers built on top of [`ByteIterator`](super::iterator::ByteIterator).
//!
//! These functions implement a tiny, allocation‑light subset of JSON/JSON‑like parsing:
//! - `parse_tag` for matching fixed ASCII tags
//! - `parse_quoted_json_string` for JSON string literals with escapes (`\" \\ \/ \b \f \n \r \t \uXXXX`)
//! - `parse_number_as_string` and `parse_number_as<T>` for JSON number syntax
//! - `parse_object_entries` and `parse_array_entries` to iterate over object/array contents
//!
//! All functions use the [`#[context]`](versatiles_derive::context) attribute to emit
//! helpful error messages annotated with where and what was being parsed.
//!
//! # Notes
//! - These are **minimal** helpers intended for internal configs/headers; they don’t aim to be
//!   a full JSON implementation.
//! - Parsing functions consume only as much as needed and leave the iterator positioned at the
//!   next token (e.g., after a closing `]` or `}`).

use super::iterator::ByteIterator;
use anyhow::{Error, Result, bail};
use std::str::FromStr;
use versatiles_derive::context;

/// Match a fixed ASCII tag at the current iterator position.
///
/// Advances the iterator byte‑by‑byte and returns an error on the first mismatch.
///
/// # Errors
/// Returns an error if the upcoming bytes do not exactly match `tag` or if the
/// underlying reader is exhausted prematurely.
///
/// # Example
/// ```
/// # use std::io::Cursor;
/// # use versatiles_core::byte_iterator::{ByteIterator, parse_tag};
/// let mut it = ByteIterator::from_reader(Cursor::new("null"), true);
/// parse_tag(&mut it, "null").unwrap();
/// ```
#[context("while parsing tag '{}'", tag)]
pub fn parse_tag(iter: &mut ByteIterator, tag: &str) -> Result<()> {
	for c in tag.bytes() {
		match iter.expect_next_byte()? {
			b if b == c => {}
			_ => return Err(iter.format_error(&format!("unexpected character while parsing tag '{tag}'"))),
		}
	}
	Ok(())
}

/// Parse a JSON quoted string literal and return it as `String`.
///
/// Supports standard JSON escapes (`\" \\ \/ \b \f \n \r \t`) and `\uXXXX` (BMP) escapes.
/// Leaves the iterator positioned **after** the closing quote.
///
/// # Errors
/// - Missing opening or closing quotes
/// - Invalid escape sequence or malformed `\uXXXX` hex
/// - Unterminated string or unexpected end of input
///
/// # Example
/// ```
/// # use std::io::Cursor;
/// # use versatiles_core::byte_iterator::{ByteIterator, parse_quoted_json_string};
/// let mut it = ByteIterator::from_reader(Cursor::new("\"he\\nllo\""), true);
/// assert_eq!(parse_quoted_json_string(&mut it).unwrap(), "he\nllo");
/// ```
#[context("while parsing a quoted JSON string")]
pub fn parse_quoted_json_string(iter: &mut ByteIterator) -> Result<String> {
	iter.skip_whitespace();
	if iter.expect_next_byte()? != b'"' {
		bail!(iter.format_error("expected '\"' while parsing a string"));
	}

	let mut bytes = Vec::with_capacity(32); // Pre-allocate based on expected JSON string sizes
	let mut hex = [0u8; 4];

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
					for i in &mut hex {
						*i = iter.expect_next_byte()?;
					}
					let code_point = u16::from_str_radix(std::str::from_utf8(&hex).unwrap(), 16)
						.map_err(|_| iter.format_error("invalid unicode code point"))?;
					bytes.extend_from_slice(
						&String::from_utf16(&[code_point])
							.map_err(|_| iter.format_error("invalid unicode code point"))?
							.into_bytes(),
					);
				}
				c => bytes.push(c),
			},
			c => bytes.push(c),
		}
	}
	String::from_utf8(bytes).map_err(Error::from)
}

/// Parse a JSON number and return its textual representation.
///
/// Accepts the JSON number grammar: optional sign, integer, optional fraction,
/// and optional exponent (`e`/`E` with optional sign). Validates that fraction
/// and exponent parts contain at least one digit.
///
/// Leaves the iterator at the first non‑number byte.
///
/// # Errors
/// Returns an error if required digits are missing or if invalid constructs
/// (e.g., multiple dots or an exponent without digits) are encountered.
///
/// # Example
/// ```
/// # use std::io::Cursor;
/// # use versatiles_core::byte_iterator::{ByteIterator, parse_number_as_string};
/// let mut it = ByteIterator::from_reader(Cursor::new("-12.3e+4,"), true);
/// assert_eq!(parse_number_as_string(&mut it).unwrap(), "-12.3e+4");
/// ```
#[context("while parsing a number")]
pub fn parse_number_as_string(iter: &mut ByteIterator) -> Result<String> {
	let mut number = Vec::with_capacity(16);

	// Optional sign
	if let Some(b'+' | b'-') = iter.peek() {
		number.push(iter.expect_next_byte()?);
	}

	// Integer part
	let mut has_digits = false;
	while let Some(b'0'..=b'9') = iter.peek() {
		has_digits = true;
		number.push(iter.expect_next_byte()?);
	}
	if !has_digits {
		return Err(iter.format_error("expected digits in number"));
	}

	// Fractional part
	if let Some(b'.') = iter.peek() {
		number.push(iter.expect_next_byte()?);
		let mut fractional_digits = false;
		while let Some(b'0'..=b'9') = iter.peek() {
			fractional_digits = true;
			number.push(iter.expect_next_byte()?);
		}
		if !fractional_digits {
			return Err(iter.format_error("expected digits after decimal point"));
		}

		// Reject multiple decimal points
		if let Some(b'.') = iter.peek() {
			return Err(iter.format_error("unexpected '.' in number"));
		}
	}

	// Exponent part
	if let Some(b'e' | b'E') = iter.peek() {
		number.push(iter.expect_next_byte()?);
		if let Some(b'+' | b'-') = iter.peek() {
			number.push(iter.expect_next_byte()?);
		}
		let mut exponent_digits = false;
		while let Some(b'0'..=b'9') = iter.peek() {
			exponent_digits = true;
			number.push(iter.expect_next_byte()?);
		}
		if !exponent_digits {
			return Err(iter.format_error("expected digits after exponent"));
		}
	}

	String::from_utf8(number).map_err(Error::from)
}

/// Parse a JSON number and convert it to a concrete type `R`.
///
/// This is a convenience on top of [`parse_number_as_string`], parsing the returned
/// string via `R: FromStr`.
///
/// # Errors
/// Returns an error if number parsing fails or if `R::from_str` returns an error.
///
/// # Example
/// ```
/// # use std::io::Cursor;
/// # use versatiles_core::byte_iterator::ByteIterator;
/// # use versatiles_core::byte_iterator::parse_number_as;
/// let mut it = ByteIterator::from_reader(Cursor::new("42"), true);
/// let n: i32 = parse_number_as(&mut it).unwrap();
/// assert_eq!(n, 42);
/// ```
pub fn parse_number_as<R: FromStr>(iter: &mut ByteIterator) -> Result<R> {
	parse_number_as_string(iter)?
		.parse::<R>()
		.map_err(|_| iter.format_error("invalid number"))
}

/// Iterate over JSON object entries, invoking `parse_value` for each key.
///
/// Expects a `{ ... }` structure with keys as **quoted strings** and a colon `:`
/// between key and value. After each value, either `,` continues to the next entry
/// or `}` terminates the object.
///
/// The provided closure receives the parsed key and a mutable reference to the
/// iterator positioned at the start of the value and is responsible for parsing
/// the value itself.
///
/// # Errors
/// Returns an error on malformed objects (missing quotes/colon/comma/brace) or
/// if `parse_value` returns an error.
///
/// # Example
/// ```
/// # use std::io::Cursor;
/// # use versatiles_core::byte_iterator::{ByteIterator, parse_object_entries, parse_quoted_json_string};
/// let mut it = ByteIterator::from_reader(Cursor::new("{\"k\":\"v\"}"), true);
/// let mut got = None;
/// parse_object_entries(&mut it, |k, it| { got = Some((k, parse_quoted_json_string(it)?)); Ok(()) }).unwrap();
/// assert_eq!(got, Some(("k".into(), "v".into())));
/// ```
#[context("while parsing object entries")]
pub fn parse_object_entries<R>(
	iter: &mut ByteIterator,
	mut parse_value: impl FnMut(String, &mut ByteIterator) -> Result<R>,
) -> Result<()> {
	iter.skip_whitespace();
	if iter.expect_next_byte()? != b'{' {
		bail!(iter.format_error("expected '{' while parsing an object"));
	}

	loop {
		iter.skip_whitespace();
		match iter.expect_peeked_byte()? {
			b'}' => {
				iter.advance();
				break;
			}
			b'"' => {
				let key = parse_quoted_json_string(iter)?;

				iter.skip_whitespace();
				if iter.expect_next_byte()? != b':' {
					return Err(iter.format_error("expected ':'"));
				}

				iter.skip_whitespace();
				parse_value(key, iter)?;

				iter.skip_whitespace();
				match iter.expect_next_byte()? {
					b',' => continue,
					b'}' => break,
					_ => return Err(iter.format_error("expected ',' or '}'")),
				}
			}
			_ => return Err(iter.format_error("parsing object, expected '\"' or '}'")),
		}
	}
	Ok(())
}

/// Iterate over JSON array entries, collecting the results from `parse_value`.
///
/// Expects a `[ ... ]` structure and allows optional whitespace between tokens.
/// Returns an empty `Vec` for `[]`, otherwise collects each parsed element
/// separated by commas.
///
/// # Errors
/// Returns an error on malformed arrays (missing brackets/commas) or if `parse_value`
/// returns an error.
///
/// # Example
/// ```
/// # use std::io::Cursor;
/// # use versatiles_core::byte_iterator::{ByteIterator, parse_array_entries, parse_number_as};
/// let mut it = ByteIterator::from_reader(Cursor::new("[1,2,3]"), true);
/// let nums: Vec<i32> = parse_array_entries(&mut it, parse_number_as).unwrap();
/// assert_eq!(nums, vec![1,2,3]);
/// ```
#[context("while parsing array entries")]
pub fn parse_array_entries<R>(
	iter: &mut ByteIterator,
	mut parse_value: impl FnMut(&mut ByteIterator) -> Result<R>,
) -> Result<Vec<R>> {
	iter.skip_whitespace();
	if iter.expect_next_byte()? != b'[' {
		bail!(iter.format_error("expected '[' while parsing an array"));
	}

	let mut result = Vec::new();

	// Check if the array is empty
	iter.skip_whitespace();
	if let Some(b']') = iter.peek() {
		iter.advance(); // Consume the closing bracket
		return Ok(result); // Return empty Vec
	}

	// Parse the first array element
	result.push(parse_value(iter)?);

	// Continue parsing additional elements, if any
	loop {
		iter.skip_whitespace();
		match iter.expect_next_byte()? {
			b']' => break,
			b',' => {
				iter.skip_whitespace();
				result.push(parse_value(iter)?);
			}
			_ => return Err(iter.format_error("parsing array, expected ',' or ']'")),
		}
	}

	Ok(result)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Cursor;

	fn get_reader(s: &str) -> ByteIterator<'_> {
		ByteIterator::from_reader(Cursor::new(s), true)
	}

	#[test]
	fn test_parse_tag() {
		fn parse(text: &str, tag: &str) -> bool {
			let mut iter = get_reader(text);
			parse_tag(&mut iter, tag).is_ok()
		}
		assert!(parse("null", "null"));
		assert!(!parse("nuul", "null"));
		assert!(parse("something", "some"));
	}

	#[test]
	fn test_parse_quoted_json_string() {
		fn parse(text: &str) -> Result<String> {
			let mut iter = get_reader(text);
			parse_quoted_json_string(&mut iter)
		}

		// Basic cases
		assert_eq!(parse(" \"hello\" ").unwrap(), "hello");
		assert_eq!(parse(" \"he\\nllo\" ").unwrap(), "he\nllo");
		assert_eq!(parse(" \"he\\u0041llo\" ").unwrap(), "heAllo");

		// Edge cases with various escapes
		assert_eq!(parse(" \"he\\b\\f\\n\\r\\tllo\" ").unwrap(), "he\x08\x0C\n\r\tllo");
		assert_eq!(parse(" \"hello \\\"world\\\"\" ").unwrap(), "hello \"world\"");

		// Invalid Unicode escape
		assert!(parse(" \"he\\u004Gllo\" ").is_err()); // Invalid hex
		assert!(parse(" \"he\\uD834\\uDD1E\" ").is_err()); // Surrogate pairs in non-UTF-16

		// Unescaped special characters (error cases)
		assert!(parse(" \"unterminated string ").is_err());
	}

	#[test]
	fn test_parse_number_as_string() -> Result<()> {
		fn parse(text: &str) -> Result<String> {
			let mut iter = get_reader(text);
			parse_number_as_string(&mut iter)
		}

		// Valid JSON number formats
		assert_eq!(parse("123")?, "123");
		assert_eq!(parse("-123")?, "-123");
		assert_eq!(parse("0.456")?, "0.456");
		assert_eq!(parse("-0.456")?, "-0.456");
		assert_eq!(parse("3e4")?, "3e4");
		assert_eq!(parse("123e10")?, "123e10");
		assert_eq!(parse("123E-10")?, "123E-10");
		assert_eq!(parse("-123.45E+6")?, "-123.45E+6");
		assert_eq!(parse("0")?, "0"); // Leading zero allowed if it's the only digit
		assert_eq!(parse("123.0e+3")?, "123.0e+3");

		// Valid numbers with spaces after (to test boundary stopping)
		assert_eq!(parse("123 ")?, "123");
		assert_eq!(parse("123.45 abc")?, "123.45");
		assert_eq!(parse("-123.45e+6xyz")?, "-123.45e+6");

		// Edge cases for leading zeros
		assert_eq!(parse("0.001")?, "0.001");
		assert_eq!(parse("01.23")?, "01.23"); // Leading zero followed by digits is invalid

		// Invalid formats
		assert_eq!(parse("123abc")?, "123"); // Extra characters after number

		// Invalid formats
		assert!(parse("123..45").is_err()); // Double decimal
		assert!(parse("1.2.3").is_err()); // Double decimal
		assert!(parse("123e").is_err()); // Exponent without digits
		assert!(parse("123e+").is_err()); // Exponent without digits
		assert!(parse("e123").is_err()); // Starts with exponent
		assert!(parse("-").is_err()); // Only a sign, no digits
		assert!(parse("123.").is_err()); // Decimal point with no following digits
		assert!(parse("0e").is_err()); // Exponent without following digits
		assert!(parse("-0.").is_err()); // Negative decimal without following digits
		Ok(())
	}

	#[test]
	fn test_parse_number_as() -> Result<()> {
		fn parse<T: FromStr>(text: &str) -> Result<T> {
			let mut iter = get_reader(text);
			let v = parse_number_as::<T>(&mut iter);
			if iter.peek().is_some() {
				return Err(iter.format_error("expected end of input after number"));
			}
			v
		}

		// Integer parsing
		assert_eq!(parse::<i32>("-123")?, -123);
		assert!(parse::<i32>("abc").is_err());
		assert!(parse::<i32>("12.34").is_err());
		assert!(parse::<i32>("1-2").is_err());

		// Floating point parsing
		assert_eq!(parse::<f64>("12.34")?, 12.34);
		assert_eq!(parse::<f64>("-0.123E3")?, -123.0);
		assert_eq!(parse::<f64>("2e10")?, 2e10);
		assert_eq!(parse::<f64>("+2e10")?, 2e10);
		assert_eq!(parse::<f64>("-2e10")?, -2e10);
		assert_eq!(parse::<f64>("2e+10")?, 2e10);
		assert_eq!(parse::<f64>("2e-10")?, 2e-10);
		assert!(parse::<f64>("abc").is_err());
		assert!(parse::<f64>("12.34.56").is_err());
		assert!(parse::<f64>("1-2").is_err());
		Ok(())
	}

	#[test]
	fn test_parse_object_entries() {
		let mut iter = get_reader("{\"key1\":\"value1\",\"key2\":\"value2\"}");

		let mut map = std::collections::HashMap::new();
		parse_object_entries(&mut iter, |key, iter| {
			let value = parse_quoted_json_string(iter)?;
			map.insert(key, value);
			Ok(())
		})
		.unwrap();

		assert_eq!(map.get("key1"), Some(&"value1".to_string()));
		assert_eq!(map.get("key2"), Some(&"value2".to_string()));
	}

	#[test]
	fn test_parse_object_entries_with_errors() {
		let mut iter = get_reader("{\"key1\":\"value1\", \"key2\": 123}");

		let result = parse_object_entries(&mut iter, |key, iter| {
			if key == "key1" {
				assert_eq!(parse_quoted_json_string(iter).unwrap(), "value1");
			} else {
				assert_eq!(parse_number_as::<i32>(iter).unwrap(), 123);
			}
			Ok(())
		});

		assert!(result.is_ok());
	}

	#[test]
	fn test_parse_array_entries() {
		let mut iter = get_reader("[\"val1\", \"val2\", \"val3\"]");
		let result = parse_array_entries(&mut iter, parse_quoted_json_string).unwrap();
		assert_eq!(result, vec!["val1", "val2", "val3"]);
	}

	#[test]
	fn test_parse_array_entries_with_numbers() {
		let mut iter = get_reader("[1, 2, 3]");
		let result = parse_array_entries(&mut iter, parse_number_as::<i32>).unwrap();
		assert_eq!(result, vec![1, 2, 3]);
	}
}
