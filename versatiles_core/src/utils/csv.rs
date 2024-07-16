use super::ByteIterator;
use anyhow::{anyhow, bail, Error, Result};
use std::io::BufRead;

fn parse_quoted_csv_string(iter: &mut ByteIterator) -> Result<String> {
	if iter.get_next_byte()? != b'"' {
		bail!(iter.build_error("expected '\"' while parsing a string"));
	}

	let mut bytes: Vec<u8> = Vec::new();
	loop {
		match iter.next_byte() {
			Some(b'"') => match iter.peek_byte() {
				Some(b'"') => {
					bytes.push(b'"');
					iter.skip_byte();
				}
				_ => return String::from_utf8(bytes).map_err(Error::from),
			},
			Some(c) => bytes.push(c),
			None => bail!("unexpected end of file"),
		}
	}
}

fn parse_simple_csv_string(iter: &mut ByteIterator, separator: u8) -> Result<String> {
	if iter.get_peek_byte()? == b'"' {
		bail!(iter.build_error("unexpected '\"' while parsing a string"));
	}

	let mut bytes: Vec<u8> = Vec::new();
	loop {
		match iter.peek_byte() {
			Some(s) if s == &separator => return String::from_utf8(bytes).map_err(Error::from),
			Some(b'\r') | Some(b'\n') | None => return String::from_utf8(bytes).map_err(Error::from),
			Some(c) => {
				bytes.push(*c);
				iter.skip_byte();
			}
		}
	}
}

fn read_csv_fields<'a>(
	reader: impl BufRead + Send + 'a,
	separator: u8,
) -> Result<impl Iterator<Item = Result<(usize, Vec<String>)>> + Send + 'a> {
	let mut iter = ByteIterator::from_reader(reader, true)?;

	let lines = std::iter::from_fn(move || -> Option<Result<(usize, Vec<String>)>> {
		if iter.peek_byte().is_none() {
			return None;
		}

		let mut fields = Vec::new();

		loop {
			let value = match iter.peek_byte() {
				Some(b'"') => match parse_quoted_csv_string(&mut iter) {
					Ok(v) => v,
					Err(e) => return Some(Err(e)),
				},
				Some(_) => match parse_simple_csv_string(&mut iter, separator) {
					Ok(v) => v,
					Err(e) => return Some(Err(e)),
				},
				None => String::new(),
			};
			fields.push(value);
			loop {
				match iter.next_byte() {
					Some(b'\r') => continue,
					Some(b'\n') => {
						if (fields.len() == 1) && (fields.first().unwrap().is_empty()) {
							fields.clear();
							break;
						}
						return Some(Ok((iter.byte_pos(), fields)));
					}
					None => {
						if (fields.len() == 1) && (fields.first().unwrap().is_empty()) {
							return None;
						}
						return Some(Ok((iter.byte_pos(), fields)));
					}
					Some(e) if e == separator => break,
					Some(_) => panic!(),
				}
			}
		}
	});

	Ok(lines)
}

pub fn read_csv_iter<'a>(
	reader: impl BufRead + Send + 'a,
	separator: u8,
) -> Result<impl Iterator<Item = Result<(usize, Vec<(String, String)>)>> + Send + 'a> {
	let mut iter = read_csv_fields(reader, separator)?;

	let header = iter.next().ok_or(anyhow!("can not find a header"))??.1;

	Ok(iter.map(move |entry| {
		entry.and_then(|(byte_pos, fields)| {
			if fields.len() != header.len() {
				bail!("At byte {byte_pos}: header and line have different number of fields")
			}
			Ok((byte_pos, header.clone().into_iter().zip(fields).collect()))
		})
	}))
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Cursor;

	#[test]
	fn test_parse_simple_csv_string() {
		fn test(input: &str, part1: &str, part2: &str) {
			let mut reader = ByteIterator::new(input.bytes(), true).unwrap();
			let value = parse_simple_csv_string(&mut reader, b',').unwrap();
			assert_eq!(value, part1);
			assert_eq!(reader.into_string(), part2);
		}

		test("name,age", "name", ",age");
		test("name\nage", "name", "\nage");
		test("\nage", "", "\nage");
		test(",age", "", ",age");
	}

	#[test]
	fn test_parse_quoted_csv_string() {
		fn test(input: &str, part1: &str, part2: &str) {
			let mut reader = ByteIterator::new(input.bytes(), true).unwrap();
			let value = parse_quoted_csv_string(&mut reader).unwrap();
			assert_eq!(value, part1);
			assert_eq!(reader.into_string(), part2);
		}

		test("\"name\"rest", "name", "rest");
		test("\"na\"\"me\"rest", "na\"me", "rest");
		test("\"na\nme\"rest", "na\nme", "rest");
		test("\"na,me\"rest", "na,me", "rest");
	}

	fn check(iter: impl Iterator<Item = Result<(usize, Vec<String>)>>) -> Vec<Vec<String>> {
		iter.map(|e| e.unwrap().1).collect()
	}

	#[test]
	fn test_read_csv_fields_basic() {
		let mut reader = Cursor::new("name,age\nJohn Doe,30\r\nJane Doe,29");
		let iter = read_csv_fields(&mut reader, b',').unwrap();

		assert_eq!(
			check(iter),
			vec![
				vec!["name", "age"],
				vec!["John Doe", "30"],
				vec!["Jane Doe", "29"]
			]
		);
	}

	#[test]
	fn test_read_csv_fields_with_quotes() {
		let mut reader = Cursor::new("name,age\n\"John, A. Doe\",30\r\n\"Jane Doe\",29");
		let iter = read_csv_fields(&mut reader, b',').unwrap();

		assert_eq!(
			check(iter),
			vec![
				vec!["name", "age"],
				vec!["John, A. Doe", "30"],
				vec!["Jane Doe", "29"]
			]
		);
	}

	#[test]
	fn test_read_csv_fields_with_escaped_quotes() {
		let mut reader = Cursor::new("name,age\n\"John \"\"The Man\"\" Doe\",30\n\"Jane Doe\",29");
		let iter = read_csv_fields(&mut reader, b',').unwrap();

		assert_eq!(
			check(iter),
			vec![
				vec!["name", "age"],
				vec!["John \"The Man\" Doe", "30"],
				vec!["Jane Doe", "29"]
			]
		);
	}

	#[test]
	fn test_read_csv_fields_empty_lines() {
		let mut reader = Cursor::new("name,age\n\nJohn Doe,30\n\nJane Doe,29\n\n");
		let iter = read_csv_fields(&mut reader, b',').unwrap();

		assert_eq!(
			check(iter),
			vec![
				vec!["name", "age"],
				vec!["John Doe", "30"],
				vec!["Jane Doe", "29"]
			]
		);
	}

	#[test]
	fn test_read_csv_fields_different_separator() {
		let mut reader = Cursor::new("name|age\nJohn Doe|30\nJane Doe|29");
		let iter = read_csv_fields(&mut reader, b'|').unwrap();

		assert_eq!(
			check(iter),
			vec![
				vec!["name", "age"],
				vec!["John Doe", "30"],
				vec!["Jane Doe", "29"]
			]
		);
	}
}
