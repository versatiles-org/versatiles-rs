use crate::geometry::{GeoProperties, GeoValue};
use anyhow::{Context, Result};
use std::{os::unix::fs::MetadataExt, path::Path};
use versatiles_core::progress::get_progress_bar;

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
	let file = std::fs::File::open(path)
		.with_context(|| format!("Failed to open file at path: {:?}", path))?;

	let size = path.metadata()?.size();

	let mut data: Vec<GeoProperties> = Vec::new();

	let mut csv_reader = csv::Reader::from_reader(file);
	let header: Vec<(usize, String)> = csv_reader
		.headers()
		.context("Failed to read CSV headers")?
		.iter()
		.map(|s| s.to_string())
		.enumerate()
		.collect();

	let mut progress = get_progress_bar("read csv", size);
	for record in csv_reader.records() {
		let record = record.context("Failed to read CSV record")?;
		progress.set_position(record.position().unwrap().byte());
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
	progress.finish();

	Ok(data)
}

#[cfg(test)]
mod tests {
	use super::*;
	use assert_fs::NamedTempFile;
	use std::vec;

	fn get_csv_reader(content: &str) -> NamedTempFile {
		let path = NamedTempFile::new("temp.csv").unwrap();
		std::fs::write(&path, content).unwrap();
		path
	}

	#[test]
	fn test_read_csv_file() -> Result<()> {
		let path = get_csv_reader("name,age\nAlice,30\nBob,25");

		let result = read_csv_file(&path)?;
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
	fn test_not_existing_file() -> Result<()> {
		let test_path = Path::new("../testdata/not-existing.csv");
		let result = read_csv_file(test_path);
		assert_eq!(
			result.unwrap_err().to_string(),
			"Failed to open file at path: \"../testdata/not-existing.csv\""
		);

		Ok(())
	}

	#[test]
	fn test_invalid_record() {
		let path = get_csv_reader("name,age\nAlice\nBob,25");
		let result = read_csv_file(&path);
		assert_eq!(result.unwrap_err().to_string(), "Failed to read CSV record");
	}

	#[test]
	fn test_new_line() {
		let path = get_csv_reader("name,age\nAlice,27\nBob,25\n");
		let result = read_csv_file(&path).unwrap();
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
	fn test_empty() -> Result<()> {
		let path = get_csv_reader("");

		let result = read_csv_file(&path)?;
		assert_eq!(result.len(), 0);

		Ok(())
	}
}
