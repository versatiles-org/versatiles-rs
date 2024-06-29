use super::csv_lines::CsvLines;
use std::io::{BufReader, Read};

pub struct CsvParser<R: Read> {
	reader: BufReader<R>,
	separator: char,
}

impl<R: Read> CsvParser<R> {
	pub fn new(reader: R, separator: char) -> Self {
		Self {
			reader: BufReader::new(reader),
			separator,
		}
	}

	pub fn lines(&mut self) -> CsvLines<R> {
		CsvLines::new(&mut self.reader, self.separator)
	}
}
