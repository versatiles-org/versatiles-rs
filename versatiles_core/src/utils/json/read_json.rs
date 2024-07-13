use super::{parse_json, JsonValue};
use anyhow::Result;
use std::io::BufRead;

pub fn read_ndjson(reader: impl BufRead) -> impl Iterator<Item = Result<JsonValue>> {
	reader.lines().filter_map(|line| {
		match line {
			Ok(line) if line.trim().is_empty() => None, // Skip empty or whitespace-only lines
			Ok(line) => Some(parse_json(&line).map_err(anyhow::Error::from)),
			Err(e) => Some(Err(anyhow::Error::from(e))),
		}
	})
}

pub fn read_json_array(
	reader: impl BufRead,
	property_path: Vec<String>,
) -> impl Iterator<Item = Result<JsonValue>> {
	reader.lines().filter_map(|line| {
		match line {
			Ok(line) if line.trim().is_empty() => None, // Skip empty or whitespace-only lines
			Ok(line) => Some(parse_json(&line).map_err(anyhow::Error::from)),
			Err(e) => Some(Err(anyhow::Error::from(e))),
		}
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Cursor;

	fn json_from_str<T: AsRef<str>>(s: T) -> JsonValue {
		parse_json(s.as_ref()).unwrap()
	}

	#[test]
	fn test_single_line() {
		let data = r#"{"key": "value"}"#;
		let reader = Cursor::new(data);
		let mut iter = read_ndjson(reader);

		assert_eq!(iter.next().unwrap().unwrap(), json_from_str(data));
		assert!(iter.next().is_none());
	}

	#[test]
	fn test_multiple_lines() {
		let data = r#"
        {"key1": "value1"}
        {"key2": "value2"}
        {"key3": "value3"}
        "#;
		let reader = Cursor::new(data.trim());
		let mut iter = read_ndjson(reader);

		assert_eq!(
			iter.next().unwrap().unwrap(),
			json_from_str(r#"{"key1": "value1"}"#)
		);
		assert_eq!(
			iter.next().unwrap().unwrap(),
			json_from_str(r#"{"key2": "value2"}"#)
		);
		assert_eq!(
			iter.next().unwrap().unwrap(),
			json_from_str(r#"{"key3": "value3"}"#)
		);
		assert!(iter.next().is_none());
	}

	#[test]
	fn test_empty_lines() {
		let data = r#"
        {"key1": "value1"}

        {"key2": "value2"}

        {"key3": "value3"}
        "#;
		let reader = Cursor::new(data.trim());
		let mut iter = read_ndjson(reader);

		assert_eq!(
			iter.next().unwrap().unwrap(),
			json_from_str(r#"{"key1": "value1"}"#)
		);
		assert_eq!(
			iter.next().unwrap().unwrap(),
			json_from_str(r#"{"key2": "value2"}"#)
		);
		assert_eq!(
			iter.next().unwrap().unwrap(),
			json_from_str(r#"{"key3": "value3"}"#)
		);
		assert!(iter.next().is_none());
	}

	#[test]
	fn test_invalid_json() {
		let data = r#"
        {"key1": "value1"}
        {invalid json}
        {"key2": "value2"}
        "#;
		let reader = Cursor::new(data.trim());
		let mut iter = read_ndjson(reader);

		assert_eq!(
			iter.next().unwrap().unwrap(),
			json_from_str(r#"{"key1": "value1"}"#)
		);
		assert!(iter.next().unwrap().is_err());
		assert_eq!(
			iter.next().unwrap().unwrap(),
			json_from_str(r#"{"key2": "value2"}"#)
		);
		assert!(iter.next().is_none());
	}

	#[test]
	fn test_mixed_valid_invalid_lines() {
		let data = r#"
        {"key1": "value1"}
        not a json
        {"key2": "value2"}
        "#;
		let reader = Cursor::new(data.trim());
		let mut iter = read_ndjson(reader);

		assert_eq!(
			iter.next().unwrap().unwrap(),
			json_from_str(r#"{"key1": "value1"}"#)
		);
		assert!(iter.next().unwrap().is_err());
		assert_eq!(
			iter.next().unwrap().unwrap(),
			json_from_str(r#"{"key2": "value2"}"#)
		);
		assert!(iter.next().is_none());
	}

	#[test]
	fn test_empty_input() {
		let data = "";
		let reader = Cursor::new(data);
		let mut iter = read_ndjson(reader);

		assert!(iter.next().is_none());
	}
}
