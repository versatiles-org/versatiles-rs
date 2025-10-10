use crate::byte_iterator::ByteIterator;
use anyhow::{Error, Result, bail};
use std::io::BufRead;

fn parse_quoted_csv_string(iter: &mut ByteIterator) -> Result<String> {
	if iter.expect_next_byte()? != b'"' {
		bail!(iter.format_error("expected '\"' while parsing a string"));
	}

	let mut bytes: Vec<u8> = Vec::new();
	loop {
		match iter.consume() {
			Some(b'"') => match iter.peek() {
				Some(b'"') => {
					bytes.push(b'"');
					iter.advance();
				}
				_ => return String::from_utf8(bytes).map_err(Error::from),
			},
			Some(c) => bytes.push(c),
			None => bail!("unexpected end of file"),
		}
	}
}

fn parse_simple_csv_string(iter: &mut ByteIterator, separator: u8) -> Result<String> {
	if iter.expect_peeked_byte()? == b'"' {
		bail!(iter.format_error("unexpected '\"' while parsing a string"));
	}

	let mut bytes: Vec<u8> = Vec::new();
	loop {
		match iter.peek() {
			Some(s) if s == separator => return String::from_utf8(bytes).map_err(Error::from),
			Some(b'\r' | b'\n') | None => return String::from_utf8(bytes).map_err(Error::from),
			Some(c) => {
				bytes.push(c);
				iter.advance();
			}
		}
	}
}

fn read_csv_fields<'a>(
	reader: impl BufRead + Send + 'a,
	separator: u8,
) -> impl Iterator<Item = Result<(Vec<String>, usize)>> {
	let mut iter = ByteIterator::from_reader(reader, true);

	std::iter::from_fn(move || -> Option<Result<(Vec<String>, usize)>> {
		iter.peek()?;

		let mut fields = Vec::new();

		loop {
			let value = match iter.peek() {
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
				match iter.consume() {
					Some(b'\r') => {}
					Some(b'\n') => {
						if (fields.len() == 1) && (fields.first().unwrap().is_empty()) {
							fields.clear();
							break;
						}
						return Some(Ok((fields, iter.position())));
					}
					None => {
						if (fields.len() == 1) && (fields.first().unwrap().is_empty()) {
							return None;
						}
						return Some(Ok((fields, iter.position())));
					}
					Some(e) if e == separator => break,
					Some(_) => panic!(),
				}
			}
		}
	})
}

pub fn read_csv_iter<'a>(
	reader: impl BufRead + Send + 'a,
	separator: u8,
) -> Result<impl Iterator<Item = Result<(Vec<String>, usize, usize)>> + 'a> {
	let iter = read_csv_fields(reader, separator);
	let mut line_pos = 0usize;
	let mut option_len: Option<usize> = None;

	Ok(iter.map(move |entry| {
		entry.and_then(|(fields, byte_pos)| {
			if let Some(len) = option_len {
				if fields.len() != len {
					bail!("At byte {byte_pos}: line {line_pos} has different number of fields");
				}
			} else {
				option_len = Some(fields.len());
			}
			line_pos += 1;
			Ok((fields, line_pos, byte_pos))
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
			let mut reader = ByteIterator::from_reader(Cursor::new(input), true);
			let value = parse_simple_csv_string(&mut reader, b',').unwrap();
			assert_eq!(value, part1);
			assert_eq!(reader.into_string().unwrap(), part2);
		}

		test("name,age", "name", ",age");
		test("name\nage", "name", "\nage");
		test("\nage", "", "\nage");
		test(",age", "", ",age");
	}

	#[test]
	fn test_parse_quoted_csv_string() {
		fn test(input: &str, part1: &str, part2: &str) {
			let mut reader = ByteIterator::from_reader(Cursor::new(input), true);
			let value = parse_quoted_csv_string(&mut reader).unwrap();
			assert_eq!(value, part1);
			assert_eq!(reader.into_string().unwrap(), part2);
		}

		test("\"name\"rest", "name", "rest");
		test("\"na\"\"me\"rest", "na\"me", "rest");
		test("\"na\nme\"rest", "na\nme", "rest");
		test("\"na,me\"rest", "na,me", "rest");
	}

	fn check(iter: impl Iterator<Item = Result<(Vec<String>, usize)>>) -> Vec<Vec<String>> {
		iter.map(|e| e.unwrap().0).collect()
	}

	#[test]
	fn test_read_csv_fields_basic() {
		let mut reader = Cursor::new("name,age\nJohn Doe,30\r\nJane Doe,29");
		let iter = read_csv_fields(&mut reader, b',');

		assert_eq!(
			check(iter),
			vec![vec!["name", "age"], vec!["John Doe", "30"], vec!["Jane Doe", "29"]]
		);
	}

	#[test]
	fn test_read_csv_fields_with_quotes() {
		let mut reader = Cursor::new("name,age\n\"John, A. Doe\",30\r\n\"Jane Doe\",29");
		let iter = read_csv_fields(&mut reader, b',');

		assert_eq!(
			check(iter),
			vec![vec!["name", "age"], vec!["John, A. Doe", "30"], vec!["Jane Doe", "29"]]
		);
	}

	#[test]
	fn test_read_csv_fields_with_escaped_quotes() {
		let mut reader = Cursor::new("name,age\n\"John \"\"The Man\"\" Doe\",30\n\"Jane Doe\",29");
		let iter = read_csv_fields(&mut reader, b',');

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
		let iter = read_csv_fields(&mut reader, b',');

		assert_eq!(
			check(iter),
			vec![vec!["name", "age"], vec!["John Doe", "30"], vec!["Jane Doe", "29"]]
		);
	}

	#[test]
	fn test_read_csv_fields_different_separator() {
		let mut reader = Cursor::new("name|age\nJohn Doe|30\nJane Doe|29");
		let iter = read_csv_fields(&mut reader, b'|');

		assert_eq!(
			check(iter),
			vec![vec!["name", "age"], vec!["John Doe", "30"], vec!["Jane Doe", "29"]]
		);
	}

	#[test]
	fn test_read_csv_iter_basic() -> Result<()> {
		let data = "name,age\nJohn,30\nJane,25";
		let iter = read_csv_iter(Cursor::new(data), b',')?;
		let results: Vec<_> = iter.collect();
		assert_eq!(results.len(), 3);
		let (fields1, line1, _) = results[0].as_ref().unwrap();
		assert_eq!(fields1, &vec!["name".to_string(), "age".to_string()]);
		assert_eq!(*line1, 1);
		let (fields2, line2, _) = results[1].as_ref().unwrap();
		assert_eq!(fields2, &vec!["John".to_string(), "30".to_string()]);
		assert_eq!(*line2, 2);
		Ok(())
	}

	#[test]
	fn test_read_csv_iter_with_quotes() -> Result<()> {
		let data = "\"a,a\",b\nc,\"d,d\"";
		let mut iter = read_csv_iter(Cursor::new(data), b',')?;
		let (fields1, _, _) = iter.next().unwrap().unwrap();
		assert_eq!(fields1, vec!["a,a".to_string(), "b".to_string()]);
		let (fields2, _, _) = iter.next().unwrap().unwrap();
		assert_eq!(fields2, vec!["c".to_string(), "d,d".to_string()]);
		Ok(())
	}

	#[test]
	fn test_read_csv_iter_inconsistent_fields() {
		let data = "a,b\nc,d,e\n";
		let mut iter = read_csv_iter(Cursor::new(data), b',').unwrap();
		// First line OK
		assert!(iter.next().unwrap().is_ok());
		// Second line has inconsistent field count
		let err = iter.next().unwrap().unwrap_err();
		assert!(err.to_string().contains("different number of fields"));
	}

	#[test]
	fn test_read_csv_iter_empty_input() -> Result<()> {
		let data = "";
		let iter = read_csv_iter(Cursor::new(data), b',')?;
		let results: Vec<_> = iter.collect();
		assert!(results.is_empty());
		Ok(())
	}
}
