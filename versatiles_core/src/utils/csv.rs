//! CSV utilities.
//!
//! Provides a small CSV reader with configurable separator, RFC4180-style quoted fields (double-quote escaping), and tolerant handling of `\n`/`\r\n` and empty lines.
//! Exposes `read_csv_iter` for consumers and keeps parsing helpers internal.

use std::{io::BufRead, path::Path};

use anyhow::{Context, Error, Result, anyhow, bail};
use versatiles_derive::context;

use crate::byte_iterator::ByteIterator;

/// Parses a quoted CSV field (`"..."`) with RFC&nbsp;4180-style escaping.
///
/// Two consecutive quotes (`""`) are decoded as a single literal `"` inside the field.
///
/// # Errors
/// Returns an error if the first byte is not `"` or if UTF‑8 decoding fails.
#[context("parsing quoted CSV field")]
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

/// Parses an unquoted CSV field until `separator` or line end.
///
/// # Arguments
/// * `separator` — The byte used to separate fields (e.g., `b','`).
///
/// # Errors
/// Returns an error if a leading `"` is encountered (quoted fields must use [`parse_quoted_csv_string`]) or if UTF‑8 decoding fails.
#[context("parsing unquoted CSV field (sep='{}')", separator as char)]
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

/// Low‑level iterator over CSV records.
///
/// Produces `(fields, byte_pos)` where `byte_pos` is the reader position *after* the line.
/// Handles both `\n` and `\r\n` line endings and skips blank lines.
///
/// # Arguments
/// * `reader` — Any `BufRead` source.
/// * `separator` — Field separator byte.
///
/// # Returns
/// An iterator over `Result<(Vec<String>, usize)>`.
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
						if (fields.len() == 1) && (fields.first().expect("len == 1").is_empty()) {
							fields.clear();
							break;
						}
						return Some(Ok((fields, iter.position())));
					}
					None => {
						if (fields.len() == 1) && (fields.first().expect("len == 1").is_empty()) {
							return None;
						}
						return Some(Ok((fields, iter.position())));
					}
					Some(e) if e == separator => break,
					// `parse_simple_csv_string` stops at a boundary, so it cannot land here.
					// `parse_quoted_csv_string` stops at its *closing quote*, and what follows
					// is whatever the file says — so a separator that does not match the file
					// arrives here. That is a diagnosable input error, not a broken invariant:
					// a semicolon-separated file with quoted fields read with the default `,`
					// is the ordinary way to reach it.
					Some(byte) => {
						return Some(Err(iter.format_error(&format!(
							"expected {:?} or end of line after a quoted field, found {:?}",
							separator as char, byte as char
						))));
					}
				}
			}
		}
	})
}

/// High‑level CSV iterator enforcing a constant column count.
///
/// Yields `(fields, line_no, byte_pos)`. After the first non‑empty line establishes the expected column count, subsequent lines must match it.
///
/// # Arguments
/// * `reader` — Any `BufRead` source.
/// * `separator` — Field separator byte (e.g., `b','`, `b'|'`).
///
/// # Errors
/// Yields an error if a line has a different number of fields, or if underlying parsing/UTF‑8 decoding fails.
///
/// # Example
/// ```
/// use std::io::Cursor;
/// use versatiles_core::utils::read_csv_iter;
/// # use anyhow::Result;
/// # fn main() -> Result<()> {
/// let input = "name,age\n\"Doe, Jane\",29\nJohn,30";
/// let mut it = read_csv_iter(Cursor::new(input), b',')?;
/// let (hdr, ln, _) = it.next().unwrap().unwrap();
/// assert_eq!(hdr, vec!["name","age"]);
/// assert_eq!(ln, 1);
/// # Ok(()) }
/// ```
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

/// Candidate separators tried when sniffing, in preference order.
///
/// Ties are broken by this order, so the most common separator wins when two
/// candidates split the header into the same number of fields.
pub const CSV_SEPARATOR_CANDIDATES: [u8; 4] = *b",;\t|";

/// A CSV file's first row, and the separator it was read with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsvHeader {
	/// Column names, in file order.
	pub columns: Vec<String>,
	/// The separator the columns were split on — either the one supplied by the
	/// caller, or the one detected.
	pub separator: u8,
}

/// Reads a CSV/TSV file's header row, without parsing the body.
///
/// Uses the same parser as [`read_csv_iter`], so the columns reported here are
/// exactly the columns a subsequent read will produce — quoting, embedded
/// separators and embedded newlines included. Only as many bytes as the first
/// record occupies are read.
///
/// # Arguments
/// * `path` — Path to the CSV/TSV file.
/// * `separator` — The field separator, or `None` to detect it.
///
/// # Detection
/// When `separator` is `None`, each candidate in [`CSV_SEPARATOR_CANDIDATES`] is
/// tried and the one splitting the header into the most fields wins; ties go to
/// the earlier candidate. A file no candidate splits is a single-column file, and
/// is reported with the first candidate as its separator.
///
/// # Errors
/// Returns an error if the file cannot be opened, if it has no header row, or —
/// when `separator` was given explicitly — if the header cannot be parsed with it.
///
/// # Example
/// ```no_run
/// use std::path::Path;
/// use versatiles_core::utils::read_csv_header;
/// # use anyhow::Result;
/// # fn main() -> Result<()> {
/// let header = read_csv_header(Path::new("cities.csv"), None)?;
/// println!("{:?} separated by {:?}", header.columns, header.separator as char);
/// # Ok(()) }
/// ```
#[context("Reading the CSV header of {path:?}")]
pub fn read_csv_header(path: &Path, separator: Option<u8>) -> Result<CsvHeader> {
	if let Some(separator) = separator {
		return Ok(CsvHeader {
			columns: read_first_record(path, separator)?,
			separator,
		});
	}

	// Try every candidate and keep the one that splits the header into the most
	// fields — that is the separator actually in the file. A candidate that is
	// not in the file yields one field, or fails outright on a quoted field,
	// which is what makes trying them viable at all.
	let mut best: Option<CsvHeader> = None;
	for separator in CSV_SEPARATOR_CANDIDATES {
		let Ok(columns) = read_first_record(path, separator) else {
			continue;
		};
		if best.as_ref().is_none_or(|b| columns.len() > b.columns.len()) {
			best = Some(CsvHeader { columns, separator });
		}
	}

	best.ok_or_else(|| anyhow!("could not read a header row with any known separator"))
}

/// Reads just the first record of `path` using `separator`.
fn read_first_record(path: &Path, separator: u8) -> Result<Vec<String>> {
	let file = std::fs::File::open(path).with_context(|| format!("failed to open {path:?}"))?;
	// `read_csv_iter` is lazy, so taking one item reads only the first record —
	// which may span several physical lines if a field is quoted.
	read_csv_iter(std::io::BufReader::new(file), separator)?
		.next()
		.ok_or_else(|| anyhow!("file has no header row"))?
		.map(|(fields, _line, _byte)| fields)
}

#[cfg(test)]
mod tests {
	use std::io::Cursor;

	use super::*;

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

	#[test]
	fn test_read_csv_iter_wrong_separator_after_quoted_field_errors() {
		// A quoted field ends at its closing quote, so the byte after it is
		// whatever the file says — here `,` while the reader was given `;`.
		let mut iter = read_csv_iter(Cursor::new("\"a\",b\n"), b';').unwrap();
		let err = iter.next().unwrap().unwrap_err();
		let msg = err.to_string();
		assert!(msg.contains("after a quoted field"), "unexpected message: {msg}");
		assert!(msg.contains("';'"), "message should name the expected separator: {msg}");
		assert!(msg.contains("','"), "message should name the byte found: {msg}");
	}

	#[test]
	fn test_read_csv_iter_semicolon_file_read_with_comma_errors() {
		// The ordinary German export: `;`-separated with quoted fields, read
		// with the `,` that `CsvReader` defaults to for `.csv`.
		let mut iter = read_csv_iter(Cursor::new("\"Name\";\"Ort\"\n"), b',').unwrap();
		assert!(iter.next().unwrap().is_err());
	}

	fn write_temp(name: &str, content: &str) -> assert_fs::NamedTempFile {
		use std::io::Write;

		let file = assert_fs::NamedTempFile::new(name).unwrap();
		std::fs::File::create(&file)
			.unwrap()
			.write_all(content.as_bytes())
			.unwrap();
		file
	}

	#[test]
	fn test_read_csv_header_explicit_separator() {
		let file = write_temp("h.csv", "name;age\nHans;35\n");
		let header = read_csv_header(file.path(), Some(b';')).unwrap();
		assert_eq!(header.columns, vec!["name", "age"]);
		assert_eq!(header.separator, b';');
	}

	#[test]
	fn test_read_csv_header_detects_semicolon() {
		let file = write_temp("h.csv", "name;age;city\nHans;35;Hamburg\n");
		let header = read_csv_header(file.path(), None).unwrap();
		assert_eq!(header.columns, vec!["name", "age", "city"]);
		assert_eq!(header.separator, b';');
	}

	#[test]
	fn test_read_csv_header_detects_comma_tab_and_pipe() {
		for (content, sep) in [
			("a,b,c\n1,2,3\n", b','),
			("a\tb\tc\n1\t2\t3\n", b'\t'),
			("a|b|c\n1|2|3\n", b'|'),
		] {
			let file = write_temp("h.csv", content);
			let header = read_csv_header(file.path(), None).unwrap();
			assert_eq!(header.columns, vec!["a", "b", "c"], "content {content:?}");
			assert_eq!(header.separator, sep, "content {content:?}");
		}
	}

	#[test]
	fn test_read_csv_header_detects_separator_past_quoted_fields() {
		// The case that used to abort the process: a quoted field read with a
		// candidate separator that is not the file's.
		let file = write_temp("h.csv", "\"Name, full\";\"Ort\"\n\"Doe, Jane\";Berlin\n");
		let header = read_csv_header(file.path(), None).unwrap();
		assert_eq!(header.columns, vec!["Name, full", "Ort"]);
		assert_eq!(header.separator, b';');
	}

	#[test]
	fn test_read_csv_header_single_column_file() {
		let file = write_temp("h.csv", "name\nHans\n");
		let header = read_csv_header(file.path(), None).unwrap();
		assert_eq!(header.columns, vec!["name"]);
	}

	#[test]
	fn test_read_csv_header_does_not_read_the_body() {
		// A body that cannot be parsed at all must not affect the header, which
		// is what "reads only the first row" has to mean in practice.
		let file = write_temp("h.csv", "a,b\nbroken,row,with,too,many,fields\n");
		let header = read_csv_header(file.path(), Some(b',')).unwrap();
		assert_eq!(header.columns, vec!["a", "b"]);
	}

	#[test]
	fn test_read_csv_header_missing_file_and_empty_file() {
		assert!(read_csv_header(Path::new("does_not_exist.csv"), Some(b',')).is_err());

		let file = write_temp("empty.csv", "");
		assert!(read_csv_header(file.path(), Some(b',')).is_err());
		assert!(read_csv_header(file.path(), None).is_err());
	}

	#[test]
	fn test_read_csv_header_matches_read_csv_iter() {
		// The point of the function: the columns it reports are exactly the
		// columns a subsequent full read produces.
		let content = "\"a;a\";b;\"c\nc\"\n1;2;3\n";
		let file = write_temp("h.csv", content);
		let header = read_csv_header(file.path(), None).unwrap();
		let mut iter = read_csv_iter(Cursor::new(content), header.separator).unwrap();
		let (first, _, _) = iter.next().unwrap().unwrap();
		assert_eq!(header.columns, first);
	}

	#[test]
	fn test_read_csv_iter_separator_can_be_detected_by_trying() {
		// Because a mismatched separator now errors instead of aborting the
		// process, "which separator is this?" is answerable by trial parse.
		let data = "\"a\";\"b\"\n1;2\n";
		let detected = (*b",;\t|")
			.into_iter()
			.find(|sep| read_csv_iter(Cursor::new(data), *sep).is_ok_and(|mut i| i.all(|r| r.is_ok())));
		assert_eq!(detected, Some(b';'));
	}
}
