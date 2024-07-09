use crate::{
	geometry::{GeoProperties, GeoValue},
	utils::progress::get_progress_bar,
};
use anyhow::{Context, Result};
use std::{io::BufReader, os::unix::fs::MetadataExt, path::Path};
use versatiles_core::utils::read_csv_as_iterator;

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

	let mut reader = BufReader::new(file);
	let mut lines = read_csv_as_iterator(&mut reader, ',');
	let header: Vec<(usize, String)> = lines
		.next()
		.unwrap()
		.0
		.into_iter()
		.enumerate()
		.collect::<Vec<_>>();

	let mut progress = get_progress_bar("read csv", size);
	for (record, pos) in lines {
		let entry = GeoProperties::from_iter(header.iter().map(|(col_index, name)| {
			(
				name.to_string(),
				GeoValue::parse_str(
					record
						.get(*col_index)
						.with_context(|| format!("Failed to get value for column: {name}"))
						.unwrap(),
				),
			)
		}));

		data.push(entry);
		progress.set_position(pos as u64);
	}
	progress.finish();

	Ok(data)
}

#[cfg(test)]
mod tests {
	use super::*;
	use assert_fs::NamedTempFile;
	use std::{fs::File, io::Write};

	fn make_temp_csv(content: &str) -> Result<NamedTempFile> {
		let temp_file = NamedTempFile::new("test.csv")?;

		let mut file = File::create(&temp_file)?;
		writeln!(&mut file, "{content}")?;
		drop(file);

		Ok(temp_file)
	}

	#[test]
	fn test_read_csv_file() -> Result<()> {
		let file_path = make_temp_csv(
			"name,age,city\nJohn Doe,30,New York\nJane Smith,25,Los Angeles\nAlice Johnson,28,Chicago",
		)?;
		let data = read_csv_file(&file_path)?;

		assert_eq!(data.len(), 3);

		let john = &data[0];
		assert_eq!(john.get("name").unwrap(), &GeoValue::from("John Doe"));
		assert_eq!(john.get("age").unwrap(), &GeoValue::from(30));
		assert_eq!(john.get("city").unwrap(), &GeoValue::from("New York"));

		let jane = &data[1];
		assert_eq!(jane.get("name").unwrap(), &GeoValue::from("Jane Smith"));
		assert_eq!(jane.get("age").unwrap(), &GeoValue::from(25));
		assert_eq!(jane.get("city").unwrap(), &GeoValue::from("Los Angeles"));

		let alice = &data[2];
		assert_eq!(alice.get("name").unwrap(), &GeoValue::from("Alice Johnson"));
		assert_eq!(alice.get("age").unwrap(), &GeoValue::from(28));
		assert_eq!(alice.get("city").unwrap(), &GeoValue::from("Chicago"));

		Ok(())
	}

	#[test]
	fn test_read_empty_csv_file() -> Result<()> {
		let file_path = make_temp_csv("name,age,city")?;
		let data = read_csv_file(&file_path)?;
		assert!(data.is_empty());
		Ok(())
	}

	#[test]
	fn test_read_csv_file_missing_values() -> Result<()> {
		let file_path =
			make_temp_csv("name,age,city\nJohn Doe,,New York\n,25,Los Angeles\nAlice Johnson,28,")?;

		let data = read_csv_file(&file_path)?;

		assert_eq!(data.len(), 3);

		let john = &data[0];
		assert_eq!(john.get("name").unwrap(), &GeoValue::from("John Doe"));
		assert_eq!(john.get("age").unwrap(), &GeoValue::from(""));
		assert_eq!(john.get("city").unwrap(), &GeoValue::from("New York"));

		let jane = &data[1];
		assert_eq!(jane.get("name").unwrap(), &GeoValue::from(""));
		assert_eq!(jane.get("age").unwrap(), &GeoValue::from(25));
		assert_eq!(jane.get("city").unwrap(), &GeoValue::from("Los Angeles"));

		let alice = &data[2];
		assert_eq!(alice.get("name").unwrap(), &GeoValue::from("Alice Johnson"));
		assert_eq!(alice.get("age").unwrap(), &GeoValue::from(28));
		assert_eq!(alice.get("city").unwrap(), &GeoValue::from(""));

		Ok(())
	}

	#[test]
	fn test_read_csv_file_incorrect_path() {
		let path = Path::new("non_existent.csv");
		let result = read_csv_file(path);
		assert!(result.is_err());
	}
}
