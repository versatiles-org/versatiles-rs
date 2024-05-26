use crate::utils::geometry::{GeoProperties, GeoValue};
use anyhow::{Context, Result};
use std::{io::Read, path::Path};

/// Reads a CSV file from the given path and returns a vector of `GeoProperties`.
///
/// # Arguments
///
/// * `path` - A reference to the path of the CSV file.
///
/// # Returns
///
/// * `Result<Vec<GeoProperties>>` - A vector of `GeoProperties` or an error if the file could not be read.
pub fn read_csv_file(path: &Path) -> Result<Vec<GeoProperties>> {
	std::fs::File::open(path)
		.with_context(|| format!("Failed to open file at path: {:?}", path))
		.and_then(|file| parse_csv(&mut std::io::BufReader::new(file)))
}

/// Parses CSV data from a reader and returns a vector of `GeoProperties`.
///
/// # Arguments
///
/// * `reader` - A mutable reference to a reader that implements the `Read` trait.
///
/// # Returns
///
/// * `Result<Vec<GeoProperties>>` - A vector of `GeoProperties` or an error if the CSV data could not be parsed.
pub fn parse_csv(reader: &mut dyn Read) -> Result<Vec<GeoProperties>> {
	let mut data: Vec<GeoProperties> = Vec::new();

	let mut csv_reader = csv::Reader::from_reader(reader);
	let header: Vec<(usize, String)> = csv_reader
		.headers()
		.context("Failed to read CSV headers")?
		.iter()
		.map(|s| s.to_string())
		.enumerate()
		.collect();

	for record in csv_reader.records() {
		let record = record.context("Failed to read CSV record")?;
		let mut entry = GeoProperties::new();
		for (col_index, name) in header.iter() {
			entry.insert(
				name.to_string(),
				GeoValue::parse_str(
					record
						.get(*col_index)
						.with_context(|| format!("Failed to get value for column: {name}"))?,
				),
			);
		}
		data.push(entry);
	}

	Ok(data)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::{io::Cursor, vec};

	#[test]
	fn test_parse_csv() -> Result<()> {
		let csv_data = "name,age\nAlice,30\nBob,25";
		let mut reader = Cursor::new(csv_data);

		let result = parse_csv(&mut reader)?;
		assert_eq!(result.len(), 2);

		let alice = &result[0];
		assert_eq!(
			alice.get("name").unwrap(),
			&GeoValue::String("Alice".to_string())
		);
		assert_eq!(alice.get("age").unwrap(), &GeoValue::UInt(30));

		let bob = &result[1];
		assert_eq!(
			bob.get("name").unwrap(),
			&GeoValue::String("Bob".to_string())
		);
		assert_eq!(bob.get("age").unwrap(), &GeoValue::UInt(25));

		Ok(())
	}

	#[test]
	fn test_read_csv_file() -> Result<()> {
		let test_path = Path::new("../testdata/not-existing.csv");
		let result = read_csv_file(test_path);
		assert_eq!(
			result.unwrap_err().to_string(),
			"Failed to open file at path: \"../testdata/not-existing.csv\""
		);

		Ok(())
	}

	#[test]
	fn test_parse_csv_invalid_record() {
		let mut reader = Cursor::new("name,age\nAlice\nBob,25");
		let result = parse_csv(&mut reader);
		assert_eq!(result.unwrap_err().to_string(), "Failed to read CSV record");
	}

	#[test]
	fn test_parse_csv_new_line() {
		let mut reader = Cursor::new("name,age\nAlice,27\nBob,25\n");
		let result = parse_csv(&mut reader).unwrap();
		assert_eq!(
			result,
			vec![
				GeoProperties::from(vec![
					("name", GeoValue::from("Alice")),
					("age", GeoValue::from(27))
				]),
				GeoProperties::from(vec![
					("name", GeoValue::from("Bob")),
					("age", GeoValue::from(25))
				])
			]
		);
	}

	#[test]
	fn test_parse_csv_empty() -> Result<()> {
		let csv_data = "";
		let mut reader = Cursor::new(csv_data);

		let result = parse_csv(&mut reader)?;
		assert_eq!(result.len(), 0);

		Ok(())
	}
}
