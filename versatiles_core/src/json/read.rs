//! Utilities for reading newline-delimited JSON (NDJSON) from readers,
//! providing both synchronous iterator and asynchronous stream interfaces.
#![allow(dead_code)]
use super::JsonValue;
use anyhow::{Context, Error, Result, anyhow};
use futures::{Stream, StreamExt, future::ready, stream};
use std::io::BufRead;

/// Process a single line of NDJSON, parsing it into `JsonValue` or reporting errors.
///
/// Skips empty or whitespace-only lines.
///
/// # Parameters
/// - `line`: Result containing the line string or an I/O error.
/// - `index`: Zero-based line index (used for error context).
///
/// # Returns
/// - `Some(Ok(JsonValue))` if parsing succeeds.
/// - `Some(Err(_))` if parsing or I/O fails (with line context).
/// - `None` if the line is empty or only whitespace.
fn process_line(line: std::io::Result<String>, index: usize) -> Option<Result<JsonValue>> {
	match line {
		Ok(line) if line.trim().is_empty() => None, // Skip empty or whitespace-only lines
		Ok(line) => Some(JsonValue::parse_str(&line).with_context(|| format!("error in line {}", index + 1))),
		Err(e) => Some(Err(anyhow!("line {}: {}", index + 1, e))),
	}
}

/// Create a synchronous iterator over NDJSON values from a buffered reader.
///
/// Each non-empty line is parsed as JSON; empty lines are skipped.
/// Errors include line number context.
///
/// # Examples
///
/// ```no_run
/// use std::io::Cursor;
/// use versatiles_core::json::read_ndjson_iter;
/// let data = "{\"key\":1}\n{\"key\":2}\n";
/// let reader = Cursor::new(data);
/// for item in read_ndjson_iter(reader) {
///     println!("{:?}", item);
/// }
/// ```
pub fn read_ndjson_iter(reader: impl BufRead) -> impl Iterator<Item = Result<JsonValue>> {
	reader
		.lines()
		.enumerate()
		.filter_map(|(index, line)| process_line(line, index))
}

/// Create an asynchronous stream over NDJSON values from a buffered reader.
///
/// Lines are parsed concurrently using Tokio tasks and buffered by CPU count.
/// Empty lines are skipped, and errors include line number context.
///
/// # Examples
///
/// ```no_run
/// use std::io::Cursor;
/// use futures::StreamExt;
/// use versatiles_core::json::read_ndjson_stream;
/// #[tokio::main]
/// async fn main() {
///     let data = "{\"key\":1}\n{\"key\":2}\n";
///     let reader = Cursor::new(data);
///     let mut stream = read_ndjson_stream(reader);
///     while let Some(item) = stream.next().await {
///         println!("{:?}", item);
///     }
/// }
/// ```
pub fn read_ndjson_stream(reader: impl BufRead) -> impl Stream<Item = Result<JsonValue>> {
	stream::iter(reader.lines().enumerate())
		.map(|(index, line)| tokio::spawn(async move { process_line(line, index) }))
		.buffered(num_cpus::get())
		.filter_map(|f| {
			ready(match f {
				Ok(value) => value,
				Err(e) => Some(Err(Error::from(e))),
			})
		})
}

#[cfg(test)]
mod tests {
	use super::*;
	use futures::StreamExt;
	use std::io::Cursor;
	use tokio;

	fn join_errors(e: &Error) -> String {
		e.chain().map(std::string::ToString::to_string).collect::<Vec<String>>().join("\n")
	}

	fn json_from_str<T: AsRef<str>>(s: T) -> Result<JsonValue> {
		JsonValue::parse_str(s.as_ref())
	}

	#[test]
	fn test_single_line() -> Result<()> {
		let data = r#"{"key": "value"}"#;
		let reader = Cursor::new(data);
		let mut iter = read_ndjson_iter(reader);

		assert_eq!(iter.next().unwrap()?, json_from_str(data)?);
		assert!(iter.next().is_none());
		Ok(())
	}

	#[test]
	fn test_multiple_lines() -> Result<()> {
		let data = r#"
        {"key1": "value1"}
        {"key2": "value2"}
        {"key3": "value3"}
        "#;
		let reader = Cursor::new(data.trim());
		let mut iter = read_ndjson_iter(reader);

		assert_eq!(iter.next().unwrap()?, json_from_str(r#"{"key1": "value1"}"#)?);
		assert_eq!(iter.next().unwrap()?, json_from_str(r#"{"key2": "value2"}"#)?);
		assert_eq!(iter.next().unwrap()?, json_from_str(r#"{"key3": "value3"}"#)?);
		assert!(iter.next().is_none());
		Ok(())
	}

	#[test]
	fn test_empty_lines() -> Result<()> {
		let data = r#"
        {"key1": "value1"}

        {"key2": "value2"}

        {"key3": "value3"}
        "#;
		let reader = Cursor::new(data.trim());
		let mut iter = read_ndjson_iter(reader);

		assert_eq!(iter.next().unwrap()?, json_from_str(r#"{"key1": "value1"}"#)?);
		assert_eq!(iter.next().unwrap()?, json_from_str(r#"{"key2": "value2"}"#)?);
		assert_eq!(iter.next().unwrap()?, json_from_str(r#"{"key3": "value3"}"#)?);
		assert!(iter.next().is_none());
		Ok(())
	}

	#[test]
	fn test_invalid_json() -> Result<()> {
		let data = "{\"key1\": \"value1\"}\n{invalid json}\n{\"key2\": \"value2\"}\n";
		let reader = Cursor::new(data.trim());
		let vec = read_ndjson_iter(reader).collect::<Vec<_>>();

		assert_eq!(vec.len(), 3);
		assert_eq!(vec[0].as_ref().unwrap(), &json_from_str(r#"{"key1": "value1"}"#)?);
		assert_eq!(
			join_errors(vec[1].as_ref().unwrap_err()),
			"error in line 2\nwhile parsing JSON '{invalid json}'\nparsing object, expected '\"' or '}' at position 1: {"
		);
		assert_eq!(vec[2].as_ref().unwrap(), &json_from_str(r#"{"key2": "value2"}"#)?);
		Ok(())
	}

	#[test]
	fn test_mixed_valid_invalid_lines() -> Result<()> {
		let data = "{\"key1\": \"value1\"}\nnot a json\n{\"key2\": \"value2\"}\n";
		let reader = Cursor::new(data.trim());
		let vec = read_ndjson_iter(reader).collect::<Vec<_>>();

		assert_eq!(vec.len(), 3);
		assert_eq!(vec[0].as_ref().unwrap(), &json_from_str(r#"{"key1": "value1"}"#)?);
		assert_eq!(
			join_errors(vec[1].as_ref().unwrap_err()),
			"error in line 2\nwhile parsing JSON 'not a json'\nunexpected character while parsing tag 'null' at position 2: no"
		);
		assert_eq!(vec[2].as_ref().unwrap(), &json_from_str(r#"{"key2": "value2"}"#)?);
		Ok(())
	}

	#[test]
	fn test_empty_input() {
		let data = "";
		let reader = Cursor::new(data);
		let mut iter = read_ndjson_iter(reader);

		assert!(iter.next().is_none());
	}
	#[tokio::test]
	async fn test_read_stream_single_line() -> Result<()> {
		let data = r#"{"key": "value"}"#;
		let reader = Cursor::new(data);
		let stream = read_ndjson_stream(reader);
		let results: Vec<_> = stream.collect().await;
		assert_eq!(results.len(), 1);
		assert_eq!(results[0].as_ref().unwrap(), &json_from_str(data)?);
		Ok(())
	}

	#[tokio::test]
	async fn test_read_stream_mixed_valid_invalid() -> Result<()> {
		let data = "{\"key1\": \"value1\"}\nnot json\n{\"key2\": \"value2\"}\n";
		let reader = Cursor::new(data);
		let results: Vec<_> = read_ndjson_stream(reader).collect().await;
		assert_eq!(results.len(), 3);
		// First valid
		assert_eq!(results[0].as_ref().unwrap(), &json_from_str(r#"{"key1": "value1"}"#)?);
		// Second invalid error message contains line information
		let err = results[1].as_ref().unwrap_err();
		let msg = join_errors(err);
		assert!(msg.contains("error in line 2"));
		// Third valid
		assert_eq!(results[2].as_ref().unwrap(), &json_from_str(r#"{"key2": "value2"}"#)?);
		Ok(())
	}
}
