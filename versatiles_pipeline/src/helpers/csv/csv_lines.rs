use anyhow::{anyhow, Result};
use nom::{
	branch::alt,
	bytes::complete::{escaped_transform, tag, take_till},
	character::complete::{char, none_of},
	combinator::{map, value},
	error::Error,
	multi::separated_list0,
	sequence::delimited,
};
use std::io::{BufRead, BufReader, Read};

pub struct CsvLines<'a, R: Read> {
	pub reader: &'a mut BufReader<R>,
	pub separator: char,
	pub buffer: String,
	position: usize,
}

impl<'a, R: Read> CsvLines<'a, R> {
	pub fn new(reader: &'a mut BufReader<R>, separator: char) -> Self {
		Self {
			reader,
			separator,
			buffer: String::default(),
			position: 0,
		}
	}
}

impl<'a, R: Read> Iterator for CsvLines<'a, R> {
	type Item = (Result<Vec<String>>, usize);

	fn next(&mut self) -> Option<Self::Item> {
		self.buffer.clear();
		let read_bytes = self.reader.read_line(&mut self.buffer).ok()?;
		if read_bytes == 0 {
			return None;
		}
		self.position += read_bytes;

		let separator = self.separator;
		let parsed = separated_list0(
			char::<&str, Error<&str>>(separator),
			alt((
				delimited(
					char('\"'),
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
					char('\"'),
				),
				map(take_till(|c| c == separator), String::from),
			)),
		)(self.buffer.as_str());

		match parsed {
			Ok((_, fields)) => Some((Ok(fields), self.position)),
			Err(err) => Some((
				Err(anyhow!(
					"Error parsing line: {err:?} at byte position: {}",
					self.position
				)),
				self.position,
			)),
		}
	}
}
