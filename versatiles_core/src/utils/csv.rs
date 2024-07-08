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

pub fn read_csv_as_iterator(
	reader: &mut dyn BufRead,
	separator: char,
) -> impl Iterator<Item = Vec<String>> + '_ {
	reader.lines().filter_map(move |line| {
		if line.is_err() {
			return None;
		}
		let line = line.unwrap();
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
		Some(result.unwrap().1)
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Cursor;

	#[test]
	fn test_read_csv_as_iterator_basic() {
		let data = "name,age\nJohn Doe,30\r\nJane Doe,29";
		let mut reader = Cursor::new(data);
		let separator = ',';

		let mut iter = read_csv_as_iterator(&mut reader, separator);

		assert_eq!(
			iter.next(),
			Some(vec!["name".to_string(), "age".to_string()])
		);
		assert_eq!(
			iter.next(),
			Some(vec!["John Doe".to_string(), "30".to_string()])
		);
		assert_eq!(
			iter.next(),
			Some(vec!["Jane Doe".to_string(), "29".to_string()])
		);
		assert_eq!(iter.next(), None);
	}

	#[test]
	fn test_read_csv_as_iterator_with_quotes() {
		let data = "name,age\n\"John, A. Doe\",30\r\n\"Jane Doe\",29";
		let mut reader = Cursor::new(data);
		let separator = ',';

		let mut iter = read_csv_as_iterator(&mut reader, separator);

		assert_eq!(
			iter.next(),
			Some(vec!["name".to_string(), "age".to_string()])
		);
		assert_eq!(
			iter.next(),
			Some(vec!["John, A. Doe".to_string(), "30".to_string()])
		);
		assert_eq!(
			iter.next(),
			Some(vec!["Jane Doe".to_string(), "29".to_string()])
		);
		assert_eq!(iter.next(), None);
	}

	#[test]
	fn test_read_csv_as_iterator_with_escaped_quotes() {
		let data = "name,age\n\"John \\\"The Man\\\" Doe\",30\n\"Jane Doe\",29";
		let mut reader = Cursor::new(data);
		let separator = ',';

		let mut iter = read_csv_as_iterator(&mut reader, separator);

		assert_eq!(
			iter.next(),
			Some(vec!["name".to_string(), "age".to_string()])
		);
		assert_eq!(
			iter.next(),
			Some(vec!["John \"The Man\" Doe".to_string(), "30".to_string()])
		);
		assert_eq!(
			iter.next(),
			Some(vec!["Jane Doe".to_string(), "29".to_string()])
		);
		assert_eq!(iter.next(), None);
	}

	#[test]
	fn test_read_csv_as_iterator_empty_lines() {
		let data = "name,age\n\nJohn Doe,30\n\nJane Doe,29\n\n";
		let mut reader = Cursor::new(data);
		let separator = ',';

		let mut iter = read_csv_as_iterator(&mut reader, separator);

		assert_eq!(
			iter.next(),
			Some(vec!["name".to_string(), "age".to_string()])
		);
		assert_eq!(
			iter.next(),
			Some(vec!["John Doe".to_string(), "30".to_string()])
		);
		assert_eq!(
			iter.next(),
			Some(vec!["Jane Doe".to_string(), "29".to_string()])
		);
		assert_eq!(iter.next(), None);
	}

	#[test]
	fn test_read_csv_as_iterator_different_separator() {
		let data = "name|age\nJohn Doe|30\nJane Doe|29";
		let mut reader = Cursor::new(data);
		let separator = '|';

		let mut iter = read_csv_as_iterator(&mut reader, separator);

		assert_eq!(
			iter.next(),
			Some(vec!["name".to_string(), "age".to_string()])
		);
		assert_eq!(
			iter.next(),
			Some(vec!["John Doe".to_string(), "30".to_string()])
		);
		assert_eq!(
			iter.next(),
			Some(vec!["Jane Doe".to_string(), "29".to_string()])
		);
		assert_eq!(iter.next(), None);
	}
}
