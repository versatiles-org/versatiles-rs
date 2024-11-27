use super::{parse_json_str, JsonValue};
use anyhow::{anyhow, Error, Result};
use futures::{future::ready, stream, Stream, StreamExt};
use std::io::{BufRead, Read};

pub fn read_json(mut reader: impl Read) -> Result<JsonValue> {
	let mut buffer = String::new();
	reader.read_to_string(&mut buffer)?;
	parse_json_str(&buffer)
}

fn process_line(line: std::io::Result<String>, index: usize) -> Option<Result<JsonValue>> {
	match line {
		Ok(line) if line.trim().is_empty() => None, // Skip empty or whitespace-only lines
		Ok(line) => Some(parse_json_str(&line).map_err(|e| anyhow!("line {}: {}", index + 1, e))),
		Err(e) => Some(Err(anyhow!("line {}: {}", index + 1, e))),
	}
}

pub fn read_ndjson_iter(reader: impl BufRead) -> impl Iterator<Item = Result<JsonValue>> {
	reader
		.lines()
		.enumerate()
		.filter_map(|(index, line)| process_line(line, index))
}

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
	use std::io::Cursor;

	fn json_from_str<T: AsRef<str>>(s: T) -> Result<JsonValue> {
		parse_json_str(s.as_ref())
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

		assert_eq!(
			iter.next().unwrap()?,
			json_from_str(r#"{"key1": "value1"}"#)?
		);
		assert_eq!(
			iter.next().unwrap()?,
			json_from_str(r#"{"key2": "value2"}"#)?
		);
		assert_eq!(
			iter.next().unwrap()?,
			json_from_str(r#"{"key3": "value3"}"#)?
		);
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

		assert_eq!(
			iter.next().unwrap()?,
			json_from_str(r#"{"key1": "value1"}"#)?
		);
		assert_eq!(
			iter.next().unwrap()?,
			json_from_str(r#"{"key2": "value2"}"#)?
		);
		assert_eq!(
			iter.next().unwrap()?,
			json_from_str(r#"{"key3": "value3"}"#)?
		);
		assert!(iter.next().is_none());
		Ok(())
	}

	#[test]
	fn test_invalid_json() -> Result<()> {
		let data = "{\"key1\": \"value1\"}\n{invalid json}\n{\"key2\": \"value2\"}\n";
		let reader = Cursor::new(data.trim());
		let vec = read_ndjson_iter(reader).collect::<Vec<_>>();

		assert_eq!(vec.len(), 3);
		assert_eq!(
			vec[0].as_ref().unwrap(),
			&json_from_str(r#"{"key1": "value1"}"#)?
		);
		assert_eq!(
			vec[1].as_ref().unwrap_err().to_string(),
			"line 2: parsing object, expected '\"' or '}' at position 1: {"
		);
		assert_eq!(
			vec[2].as_ref().unwrap(),
			&json_from_str(r#"{"key2": "value2"}"#)?
		);
		Ok(())
	}

	#[test]
	fn test_mixed_valid_invalid_lines() -> Result<()> {
		let data = "{\"key1\": \"value1\"}\nnot a json\n{\"key2\": \"value2\"}\n";
		let reader = Cursor::new(data.trim());
		let vec = read_ndjson_iter(reader).collect::<Vec<_>>();

		assert_eq!(vec.len(), 3);
		assert_eq!(
			vec[0].as_ref().unwrap(),
			&json_from_str(r#"{"key1": "value1"}"#)?
		);
		assert_eq!(
			vec[1].as_ref().unwrap_err().to_string(),
			"line 2: unexpected character while parsing tag 'null' at position 2: no"
		);
		assert_eq!(
			vec[2].as_ref().unwrap(),
			&json_from_str(r#"{"key2": "value2"}"#)?
		);
		Ok(())
	}

	#[test]
	fn test_empty_input() {
		let data = "";
		let reader = Cursor::new(data);
		let mut iter = read_ndjson_iter(reader);

		assert!(iter.next().is_none());
	}
}
