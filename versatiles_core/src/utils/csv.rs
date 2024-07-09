use anyhow::{anyhow, Result};
use nom::{
	branch::alt,
	bytes::complete::{escaped_transform, tag, take_till},
	character::complete::{char, none_of},
	combinator::value,
	error::Error,
	multi::separated_list0,
	sequence::delimited,
	Parser,
};
use std::io::BufRead;

pub struct Lines<B> {
	buf: B,
	pos: usize,
}

impl<B> Lines<B> {
	fn new(buf: B) -> Self {
		Self { buf, pos: 0 }
	}
}

impl<B: BufRead> Iterator for Lines<B> {
	type Item = Result<(String, usize)>;

	fn next(&mut self) -> Option<Result<(String, usize)>> {
		let mut buf = String::new();
		match self.buf.read_line(&mut buf) {
			Ok(0) => None,
			Ok(n) => {
				self.pos += n;
				if buf.ends_with('\n') {
					buf.pop();
					if buf.ends_with('\r') {
						buf.pop();
					}
				}
				Some(Ok((buf, n)))
			}
			Err(e) => Some(Err(anyhow!(e))),
		}
	}
}

pub fn read_csv_as_iterator<T>(
	reader: &mut T,
	separator: char,
) -> impl Iterator<Item = (Vec<String>, usize)> + '_
where
	T: BufRead,
{
	Lines::new(reader).filter_map(move |line| {
		if line.is_err() {
			return None;
		}
		let (line, pos) = line.unwrap();
		if line.is_empty() {
			return None;
		}
		let result = separated_list0(
			char::<&str, Error<&str>>(separator),
			alt((
				delimited(
					char('"'),
					escaped_transform(
						none_of("\\\""),
						'\\',
						alt((
							value("\\", tag("\\")),
							value("\"", tag("\"")),
							value("\n", tag("n")),
							value("\t", tag("t")),
						)),
					),
					char('"'),
				),
				take_till(|c| c == separator).map(|s: &str| s.to_string()),
			)),
		)(line.as_str());
		Some((result.unwrap().1, pos))
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Cursor;

	fn check(iter: &mut impl Iterator<Item = (Vec<String>, usize)>, result: &[&str]) {
		let entry = iter.next().unwrap();
		let record = entry.0.iter().map(|e| e.as_str()).collect::<Vec<&str>>();
		assert_eq!(record, result);
	}

	#[test]
	fn test_read_csv_as_iterator_basic() {
		let data = "name,age\nJohn Doe,30\r\nJane Doe,29";
		let mut reader = Cursor::new(data);
		let separator = ',';

		let mut iter = read_csv_as_iterator(&mut reader, separator);

		check(&mut iter, &["name", "age"]);
		check(&mut iter, &["John Doe", "30"]);
		check(&mut iter, &["Jane Doe", "29"]);
		assert_eq!(iter.next(), None);
	}

	#[test]
	fn test_read_csv_as_iterator_with_quotes() {
		let data = "name,age\n\"John, A. Doe\",30\r\n\"Jane Doe\",29";
		let mut reader = Cursor::new(data);
		let separator = ',';

		let mut iter = read_csv_as_iterator(&mut reader, separator);

		check(&mut iter, &["name", "age"]);
		check(&mut iter, &["John, A. Doe", "30"]);
		check(&mut iter, &["Jane Doe", "29"]);
		assert_eq!(iter.next(), None);
	}

	#[test]
	fn test_read_csv_as_iterator_with_escaped_quotes() {
		let data = "name,age\n\"John \\\"The Man\\\" Doe\",30\n\"Jane Doe\",29";
		let mut reader = Cursor::new(data);
		let separator = ',';

		let mut iter = read_csv_as_iterator(&mut reader, separator);

		check(&mut iter, &["name", "age"]);
		check(&mut iter, &["John \"The Man\" Doe", "30"]);
		check(&mut iter, &["Jane Doe", "29"]);
		assert_eq!(iter.next(), None);
	}

	#[test]
	fn test_read_csv_as_iterator_empty_lines() {
		let data = "name,age\n\nJohn Doe,30\n\nJane Doe,29\n\n";
		let mut reader = Cursor::new(data);
		let separator = ',';

		let mut iter = read_csv_as_iterator(&mut reader, separator);

		check(&mut iter, &["name", "age"]);
		check(&mut iter, &["John Doe", "30"]);
		check(&mut iter, &["Jane Doe", "29"]);
		assert_eq!(iter.next(), None);
	}

	#[test]
	fn test_read_csv_as_iterator_different_separator() {
		let data = "name|age\nJohn Doe|30\nJane Doe|29";
		let mut reader = Cursor::new(data);
		let separator = '|';

		let mut iter = read_csv_as_iterator(&mut reader, separator);

		check(&mut iter, &["name", "age"]);
		check(&mut iter, &["John Doe", "30"]);
		check(&mut iter, &["Jane Doe", "29"]);
		assert_eq!(iter.next(), None);
	}
}
